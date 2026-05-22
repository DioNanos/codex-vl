use super::super::*;

use crate::vl::crt::CrtAnimationLedger;
use crate::vl::crt::FrameTarget;
use crate::vl::crt::PacingProbe;
use crate::vl::crt::VivlingCrtConfig;

impl Vivling {
    pub(crate) fn unavailable() -> Self {
        Self {
            codex_home: None,
            auth_mode: AuthCredentialsStoreMode::default(),
            state: None,
            active_vivling_id: None,
            frame_requester: None,
            animations_enabled: false,
            task_running: Cell::new(false),
            active_until: Cell::new(None),
            active_started_at: Cell::new(None),
            next_scheduled_frame_at: RefCell::new(None),
            animation_text: RefCell::new(None),
            animation_text_expires_at: Cell::new(None),
            activity: RefCell::new(None),
            live_context: RefCell::new(None),
            msa: None,
            crt_config: VivlingCrtConfig::default(),
            crt_animation_ledger: CrtAnimationLedger::new(),
            crt_frame_target: Cell::new(FrameTarget::detect(PacingProbe::from_std_env())),
            startup_dispatched: Cell::new(false),
            session_chat_turns: Cell::new(0),
        }
    }

    pub(crate) fn configure_runtime(
        &mut self,
        frame_requester: FrameRequester,
        animations_enabled: bool,
    ) {
        self.frame_requester = Some(frame_requester);
        self.animations_enabled = animations_enabled;
        // Re-detect frame pacing once we know the runtime is wired; the
        // probe is cheap enough to redo here.
        self.crt_frame_target
            .set(FrameTarget::detect(PacingProbe::from_std_env()));
    }

    pub(crate) fn configure(&mut self, codex_home: &Path, auth_mode: AuthCredentialsStoreMode) {
        let codex_home = codex_home.to_path_buf();
        let needs_reload = self.codex_home.as_ref() != Some(&codex_home);
        self.crt_config = VivlingCrtConfig::load_from_codex_home(&codex_home);
        self.codex_home = Some(codex_home);
        self.auth_mode = auth_mode;
        if self.msa.is_none() {
            self.msa = VivlingMsa::open().map(std::sync::Arc::new);
        }
        if needs_reload {
            let migrated = self.migrate_legacy_state_if_needed().ok().flatten();
            self.state = if migrated.is_some() {
                migrated
            } else {
                self.load_state().ok().flatten()
            };
            self.active_vivling_id = self.state.as_ref().map(|state| state.vivling_id.clone());
            self.maybe_backfill_msa_index();
            // Memory V2 Step 12.B.L — reset bootstrap flag whenever a
            // fresh state is loaded (codex_home toggle). The actual
            // dispatch happens from the chatwidget pre_draw_tick path,
            // which has access to the async runtime + app_event_tx
            // needed to spawn the background LLM task. Keeping the
            // flag here lets `Vivling` (sync, no tokio context) signal
            // "needs bootstrap" without owning the dispatch itself.
            self.startup_dispatched.set(false);
        }
    }

    fn maybe_backfill_msa_index(&self) {
        let Some(msa) = self.msa.as_deref() else {
            return;
        };
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let Some(idx) = msa.collection_for(&state.vivling_id) else {
            return;
        };
        if idx.stats().map(|stats| stats.num_chunks).unwrap_or(0) > 0 {
            return;
        }
        for capsule in &state.work_memory {
            msa.index_capsule(&state.vivling_id, capsule);
        }
    }

    pub(crate) fn should_render(&self) -> bool {
        self.visible_state().is_some()
    }

    pub(crate) fn set_task_running(&self, running: bool) {
        self.task_running.set(running);
        if running {
            self.mark_recent_activity(ACTIVE_FOOTER_TAIL);
        } else {
            self.request_frame();
        }
    }

    /// Memory V2 Step 12.B.P — Ctrl+J discoverability check. Called
    /// from chatwidget after every `/vl` chat turn. Returns `true`
    /// the FIRST time all three conditions hold:
    ///   1. session_chat_turns ≥ `HINT_THRESHOLD` (3)
    ///   2. user has never expanded the sidebar this session
    ///      (passed by caller via `sidebar_opened`)
    ///   3. `chat_hint_shown` on the active Vivling state is `false`
    /// On `true`, the persisted flag is set so the hint never fires
    /// again for this Vivling.
    pub(crate) fn should_emit_chat_panel_hint(&mut self, sidebar_opened: bool) -> bool {
        const HINT_THRESHOLD: u32 = 3;
        let turns = self.session_chat_turns.get().saturating_add(1);
        self.session_chat_turns.set(turns);
        if sidebar_opened {
            return false;
        }
        if turns < HINT_THRESHOLD {
            return false;
        }
        let already_shown = self
            .state
            .as_ref()
            .map(|s| s.chat_hint_shown)
            .unwrap_or(true);
        if already_shown {
            return false;
        }
        if let Some(state) = self.state.as_mut() {
            state.chat_hint_shown = true;
        }
        // Best-effort persist — failure simply means the hint may
        // show again on the next session, which is not a correctness
        // issue.
        let _ = self.save_state();
        true
    }

    pub(crate) fn set_live_context(&self, context: Option<VivlingLiveContext>) {
        if *self.live_context.borrow() == context {
            return;
        }
        *self.live_context.borrow_mut() = context;
        self.request_frame();
    }

    pub(crate) fn set_animation_text(&self, text: String) {
        self.set_animation_text_at(text, Instant::now());
    }

    pub(crate) fn set_animation_text_at(&self, text: String, now: Instant) {
        let text = text.trim().to_string();
        if text.is_empty() {
            self.clear_animation_text();
            return;
        }
        *self.animation_text.borrow_mut() = Some(text);
        self.animation_text_expires_at
            .set(Some(now + ANIMATION_TEXT_TTL));
        self.request_frame();
    }

    pub(crate) fn current_animation_text_at(&self, now: Instant) -> Option<String> {
        let expired = self
            .animation_text_expires_at
            .get()
            .is_some_and(|deadline| deadline <= now);
        if expired {
            self.clear_animation_text();
            return None;
        }
        self.animation_text.borrow().clone()
    }

    fn clear_animation_text(&self) {
        *self.animation_text.borrow_mut() = None;
        self.animation_text_expires_at.set(None);
        self.request_frame();
    }
}
