use super::super::ShadowState;
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
            msa: None,
            crt_config: VivlingCrtConfig::default(),
            crt_animation_ledger: CrtAnimationLedger::new(),
            shadow: ShadowState::with_lifecycle(VivlingLifecyclePhase::Unavailable),
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
        self.shadow.crt_frame_target = FrameTarget::detect(PacingProbe::from_std_env());
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
            self.shadow.startup_dispatched = false;
        }
        // Step 12.C — mark configured: Unavailable -> Idle (idempotente).
        self.shadow.lifecycle.set_available();
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

    pub(crate) fn set_task_running(&mut self, running: bool) {
        // Step 12.C — la FSM di fase è ora sorgente di verità (flag legacy rimosso).
        {
            let phase = &mut self.shadow.lifecycle;
            phase.set_available(); // configure() precede sempre un task
            if running {
                // codex-vl — regressione presa da
                // footer_pose_animates_while_visible_and_idle: la FSM entra in
                // task PRIMA di mark_recent_activity, che già vedeva
                // is_task_running()=true e non seminava mai il clock della
                // pose (frame congelato). Il seed avviene qui, all'inizio
                // del task: il clock di animazione parte con la FSM.
                let now = std::time::Instant::now();
                let started_transition = phase.begin_task(now);
                if started_transition {
                    // Idempotente: true→true (task già in corso) non riazzera
                    // il clock della pose; solo la transizione reale lo semina.
                    self.shadow.active_started_at = Some(now);
                }
            } else {
                phase.end_task();
            }
        }
        if running {
            self.mark_recent_activity(ACTIVE_FOOTER_TAIL);
        } else {
            self.request_frame();
        }
    }

    /// Step 12.C — lettura della fase (Task 4 sposterà i call-site qui).
    pub(crate) fn is_task_running(&self) -> bool {
        self.shadow.lifecycle.is_task_running()
    }

    /// Apre un dispatch di espressione se nessuno è in volo. false (skip) altrimenti.
    pub(crate) fn try_begin_expression(&mut self, kind: ExpressionKind) -> bool {
        if self.shadow.expression_in_flight.is_some() {
            false
        } else {
            self.shadow.expression_in_flight = Some(kind);
            true
        }
    }

    /// Chiude il dispatch in volo (no-op se nessuno). Fail-safe: invocato da
    /// ENTRAMBI i completion handler (success + failure).
    pub(crate) fn finish_expression(&mut self) {
        self.shadow.expression_in_flight = None;
    }

    /// Step 12.D / test helper: stato del gate di espressione.
    #[allow(dead_code)]
    pub(crate) fn expression_in_flight(&self) -> bool {
        self.shadow.expression_in_flight.is_some()
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
    pub(crate) fn chat_panel_hint(&mut self, sidebar_opened: bool) -> Option<String> {
        const HINT_THRESHOLD: u32 = 3;
        let turns = self.shadow.session_chat_turns.saturating_add(1);
        self.shadow.session_chat_turns = turns;
        if sidebar_opened {
            return None;
        }
        if turns < HINT_THRESHOLD {
            return None;
        }
        let already_shown = self
            .state
            .as_ref()
            .map(|s| s.chat_hint_shown)
            .unwrap_or(true);
        if already_shown {
            return None;
        }
        // codex-vl: localize the hint via the same effective-language
        // resolution used for greetings and brain replies, so the
        // one-shot suggestion matches what the Vivling speaks.
        let system_lang = std::env::var("LANG").ok();
        let lang = self
            .state
            .as_ref()
            .map(|s| s.language_state.effective_language(system_lang.as_deref()))
            .unwrap_or_else(|| "en".to_string());
        let hint =
            codex_vivling_core::model::VivlingLanguageState::chat_panel_hint(&lang).to_string();
        if let Some(state) = self.state.as_mut() {
            state.chat_hint_shown = true;
        }
        // Best-effort persist — failure simply means the hint may
        // show again on the next session, which is not a correctness
        // issue.
        let _ = self.save_state();
        Some(hint)
    }

    pub(crate) fn set_live_context(&mut self, context: Option<VivlingLiveContext>) {
        if self.shadow.live_context == context {
            return;
        }
        self.shadow.live_context = context;
        self.request_frame();
    }

    /// codex-vl — CRT scene activity (ex direct field write
    /// in `BottomPane::set_vivling_activity`).
    pub(crate) fn set_activity(&mut self, activity: Option<crate::vl::VivlingActivity>) {
        self.shadow.activity = activity;
    }

    pub(crate) fn activity(&self) -> Option<crate::vl::VivlingActivity> {
        self.shadow.activity
    }

    pub(crate) fn set_animation_text(&mut self, text: String) {
        self.set_animation_text_at(text, Instant::now());
    }

    pub(crate) fn set_animation_text_at(&mut self, text: String, now: Instant) {
        let text = text.trim().to_string();
        if text.is_empty() {
            self.clear_animation_text();
            return;
        }
        self.shadow.animation_text = Some(text.clone());
        self.shadow.animation_text_expires_at = Some(now + ANIMATION_TEXT_TTL);
        self.request_frame();
    }

    pub(crate) fn current_animation_text_at(&self, now: Instant) -> Option<String> {
        let expired = self
            .shadow
            .animation_text_expires_at
            .is_some_and(|deadline| deadline <= now);
        if expired {
            // codex-vl: la pulizia è differita al `tick` (&mut); qui la
            // lettura resta pura perché il render path è `&self`.
            return None;
        }
        self.shadow.animation_text.clone()
    }

    fn clear_animation_text(&mut self) {
        self.shadow.animation_text = None;
        self.shadow.animation_text_expires_at = None;
        self.request_frame();
    }
}
