//! codex-vl: Vivling-related methods on `BottomPane`.
//!
//! These `impl BottomPane` blocks live in a dedicated file so that
//! upstream changes to `bottom_pane/mod.rs` (where the base struct and
//! its canonical methods live) do not need to be merged around our
//! additions. The four codex-vl fields (`vivling`, `vl_sidebar`,
//! `vl_lifecycle`, `loop_context_label`) still live on the struct
//! because Rust cannot add fields via extensions; keeping *methods*
//! isolated is the useful half of the separation.
//!
//! ## Codex-vl fields documented here (kept undecorated in `mod.rs`)
//!
//! - `vivling: crate::vivling::Vivling` — local terminal companion
//!   (Vivling) used by `/vivling` and `/vl` commands.
//! - `vl_sidebar: crate::vl::VivlingSidebar` — dedicated sidebar for
//!   Vivling chat/assist messages, toggled by Ctrl+J.
//! - `vl_lifecycle: Option<crate::vl::LifecycleState>` — lifecycle
//!   state for Vivling activity (sleeping/eating/working/animation
//!   text). Lazy-initialized via `ensure_vl_lifecycle`.
//! - `loop_context_label: Option<String>` — textual summary of active
//!   loop jobs surfaced in the chat composer footer.
//!
//! ## Boundary helpers extracted in iter C (2026-05-16)
//!
//! Iter C (`feat/bottom-pane-vl-boundary-iter-c`) extracted the four
//! remaining VL logic blocks from `bottom_pane/mod.rs` into the bridge
//! methods listed below. The upstream-facing file in `mod.rs` now
//! delegates each VL touch via a single one-line call, so the next
//! merge from `rust-v0.131.0` final has no Vivling logic to reconcile
//! inside its render / keymap / task-running / constructor paths.
//!
//! - C1: `codex_vl_push_render_extras` — render insert for sidebar +
//!   Vivling strip (formerly inline in
//!   `as_renderable_with_composer_right_reserve`).
//! - C2: `codex_vl_make_vivling` (associated fn) — factory for the
//!   `Vivling::unavailable()` baseline + `configure_runtime`
//!   (formerly inline in `BottomPane::new`).
//! - C2: `codex_vl_handle_input_event` — Ctrl+J toggle + sidebar
//!   scroll keymap intercept (formerly inline in
//!   `BottomPane::handle_key_event`).
//! - C2: `codex_vl_on_task_running` — task-running forward to the
//!   Vivling companion (formerly inline in
//!   `BottomPane::set_task_running`).

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;

use super::BottomPane;
use crate::legacy_core::config::Config;
use crate::render::renderable::FlexRenderable;
use crate::render::renderable::RenderableItem;
use crate::tui::FrameRequester;
use crate::vivling::VivlingAction;
use crate::vivling::VivlingCommandOutcome;
use crate::vivling::VivlingLoopEvent;

/// codex-vl boundary adapter: translate a `vivling::BondTone` (vivling domain)
/// to a `vl::lifecycle::LifecycleVoiceTone` (lifecycle layer). Keeping the
/// translation here lets `vl/lifecycle` stay free of any `crate::vivling::*`
/// import, so the layer remains testable in isolation.
fn vivling_tone_to_lifecycle(
    tone: crate::vivling::BondTone,
) -> crate::vl::lifecycle::LifecycleVoiceTone {
    match tone {
        crate::vivling::BondTone::Neutral => crate::vl::lifecycle::LifecycleVoiceTone::Neutral,
        crate::vivling::BondTone::Warm => crate::vl::lifecycle::LifecycleVoiceTone::Warm,
        crate::vivling::BondTone::Familiar => crate::vl::lifecycle::LifecycleVoiceTone::Familiar,
    }
}

impl BottomPane {
    pub(crate) fn configure_vivling(&mut self, config: &Config) {
        self.vivling
            .configure_runtime(self.frame_requester.clone(), self.animations_enabled);
        self.vivling.configure(
            config.codex_home.as_path(),
            config.cli_auth_credentials_store_mode,
        );
    }

    pub(crate) fn run_vivling_command(
        &mut self,
        config: &Config,
        action: VivlingAction,
    ) -> Result<VivlingCommandOutcome, String> {
        self.configure_vivling(config);
        let result = self.vivling.command(action, config.cwd.as_path());
        self.request_redraw();
        result
    }

    pub(crate) fn assign_vivling_brain_profile(
        &mut self,
        config: &Config,
        profile: String,
    ) -> Result<String, String> {
        self.configure_vivling(config);
        let result = self.vivling.assign_brain_profile(profile);
        self.request_redraw();
        result
    }

    pub(crate) fn mark_vivling_brain_runtime_error(
        &mut self,
        config: &Config,
        error: &str,
    ) -> Result<(), String> {
        self.configure_vivling(config);
        let result = self.vivling.mark_brain_runtime_error(error);
        self.request_redraw();
        result
    }

    pub(crate) fn mark_vivling_brain_runtime_error_for(
        &mut self,
        config: &Config,
        vivling_id: &str,
        error: &str,
    ) -> Result<(), String> {
        self.configure_vivling(config);
        let result = self.vivling.mark_brain_runtime_error_for(vivling_id, error);
        self.request_redraw();
        result
    }

    pub(crate) fn mark_vivling_brain_reply(
        &mut self,
        config: &Config,
        reply: &str,
    ) -> Result<(), String> {
        self.configure_vivling(config);
        let result = self.vivling.mark_brain_reply(reply);
        self.request_redraw();
        result
    }

    pub(crate) fn mark_vivling_brain_reply_for(
        &mut self,
        config: &Config,
        vivling_id: &str,
        reply: &str,
    ) -> Result<(), String> {
        self.configure_vivling(config);
        let result = self.vivling.mark_brain_reply_for(vivling_id, reply);
        self.request_redraw();
        result
    }

    pub(crate) fn record_vivling_brain_success(
        &mut self,
        config: &Config,
        kind: crate::vivling::VivlingBrainRequestKind,
    ) -> Result<(), String> {
        self.configure_vivling(config);
        let result = self.vivling.record_brain_success(kind);
        self.request_redraw();
        result
    }

    pub(crate) fn record_vivling_expression_result_for(
        &mut self,
        config: &Config,
        vivling_id: &str,
        reply: &crate::vivling::VivlingExpressionResult,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), String> {
        self.configure_vivling(config);
        let result = self
            .vivling
            .record_expression_result_for(vivling_id, reply, now);
        self.request_redraw();
        result
    }

    pub(crate) fn record_vivling_expression_failure_for(
        &mut self,
        config: &Config,
        vivling_id: &str,
    ) -> Result<(), String> {
        self.configure_vivling(config);
        let result = self.vivling.record_expression_failure_for(vivling_id);
        self.request_redraw();
        result
    }

    /// Memory V2 Step 12.B.D.3 — best-effort post-turn / post-loop
    /// trigger for the Expression channel. Forwards to the active
    /// Vivling's reservation + planner pipeline; returns `None`
    /// whenever there is nothing to dispatch (no active state,
    /// planner refused, throttle / dedup / budget / opt-out, …).
    pub(crate) fn try_dispatch_vivling_expression_refresh(
        &mut self,
        config: &Config,
    ) -> Option<crate::vivling::VivlingExpressionRequest> {
        self.configure_vivling(config);
        let request = self.vivling.try_dispatch_expression_refresh();
        if request.is_some() {
            self.request_redraw();
        }
        request
    }

    /// Memory V2 Step 12.B.H — force-refresh variant for the
    /// `/vivling crt-brain refresh` command. Bypasses throttle.
    pub(crate) fn try_dispatch_vivling_expression_refresh_forced(
        &mut self,
        config: &Config,
    ) -> Option<crate::vivling::VivlingExpressionRequest> {
        self.configure_vivling(config);
        let request = self.vivling.try_dispatch_expression_refresh_forced();
        if request.is_some() {
            self.request_redraw();
        }
        request
    }

    /// Memory V2 Step 12.B.P — Ctrl+J discoverability check. Called
    /// from chatwidget after every `/vl` chat turn. Returns `true`
    /// on the FIRST turn that satisfies (turns ≥ 3, sidebar never
    /// opened, hint never shown). The persisted flag is set on the
    /// first `true` so the hint never repeats for this Vivling.
    pub(crate) fn should_emit_vivling_chat_panel_hint(&mut self, config: &Config) -> bool {
        self.configure_vivling(config);
        let sidebar_opened = self.vl_sidebar.is_expanded();
        self.vivling.should_emit_chat_panel_hint(sidebar_opened)
    }

    /// Memory V2 Step 12.B.L — one-shot bootstrap dispatch invoked
    /// from `codex_vl_pre_draw_tick`. Returns `Some(request)` only on
    /// the FIRST qualifying frame after a state load; subsequent
    /// frames short-circuit via `Vivling::startup_dispatched`.
    pub(crate) fn try_dispatch_vivling_bootstrap_expression(
        &mut self,
        config: &Config,
    ) -> Option<crate::vivling::VivlingExpressionRequest> {
        self.configure_vivling(config);
        let request = self.vivling.try_dispatch_bootstrap_expression();
        if request.is_some() {
            self.request_redraw();
        }
        request
    }

    /// Memory V2 Step 12.B.D.4 — loop-event variant. Layers
    /// Adult-only + 5min throttle + 50% budget headroom on top of
    /// the standard turn-driven pipeline.
    pub(crate) fn try_dispatch_vivling_loop_expression_refresh(
        &mut self,
        config: &Config,
    ) -> Option<crate::vivling::VivlingExpressionRequest> {
        self.configure_vivling(config);
        let request = self.vivling.try_dispatch_loop_expression_refresh();
        if request.is_some() {
            self.request_redraw();
        }
        request
    }

    pub(crate) fn active_vivling_loop_owner_identity(
        &mut self,
        config: &Config,
    ) -> Result<(String, String), String> {
        self.configure_vivling(config);
        self.vivling.active_loop_owner_identity()
    }

    pub(crate) fn prepare_vivling_loop_tick(
        &mut self,
        config: &Config,
        owner_vivling_id: &str,
        job: &codex_state::ThreadLoopJob,
    ) -> Result<crate::vivling::VivlingLoopTickRequest, String> {
        self.configure_vivling(config);
        self.vivling
            .prepare_loop_tick_request(owner_vivling_id, job)
    }

    pub(crate) fn record_vivling_loop_event(&mut self, config: &Config, event: VivlingLoopEvent) {
        self.configure_vivling(config);
        if let Err(err) = self.vivling.record_loop_event(event) {
            tracing::debug!("failed to record vivling loop event: {err}");
        }
        self.request_redraw();
    }

    pub(crate) fn record_vivling_turn_completed(&mut self, config: &Config, summary: Option<&str>) {
        self.configure_vivling(config);
        if let Err(err) = self.vivling.record_turn_completed(summary) {
            tracing::debug!("failed to record vivling work memory: {err}");
        }
        self.request_redraw();
    }

    /// Memory V2 §8.2 (Step 5.B) — feed an ordinary user-turn payload
    /// into the active Vivling's language detection window. Best-effort:
    /// any failure is debug-logged and swallowed so the user input flow
    /// never breaks on a missing Vivling or save error.
    pub(crate) fn record_vivling_user_language_sample(&mut self, config: &Config, text: &str) {
        self.configure_vivling(config);
        self.vivling.record_user_language_sample(text);
        self.request_redraw();
    }

    pub(crate) fn toggle_vl_sidebar(&mut self) {
        self.vl_sidebar.toggle();
        self.request_redraw();
    }

    pub(crate) fn scroll_vl_sidebar(&mut self, delta: i32) {
        self.vl_sidebar.scroll(delta);
        self.request_redraw();
    }

    pub(crate) fn push_vl_sidebar_message(
        &mut self,
        kind: crate::vl::VivlingLogKind,
        text: String,
        vivling_id: Option<String>,
    ) {
        self.vl_sidebar.push(kind, text, vivling_id);
        self.request_redraw();
    }

    pub(crate) fn active_vivling_id(&self) -> Option<&str> {
        self.vivling.active_vivling_id.as_deref()
    }

    pub(crate) fn ensure_vl_lifecycle(&mut self) {
        if self.vl_lifecycle.is_none() {
            let stats_path = self
                .vivling_codex_home()
                .map(|home| home.join("vivlings").join("live_stats.json"));
            let stats = stats_path
                .as_ref()
                .map(|p| crate::vl::VivlingLiveStats::load_from(p))
                .unwrap_or_default();
            self.vl_lifecycle = Some(crate::vl::LifecycleState::new(stats));
        }
    }

    pub(crate) fn vivling_codex_home(&self) -> Option<std::path::PathBuf> {
        self.vivling.codex_home.clone()
    }

    /// Test-only accessor for the active Vivling. Kept behind
    /// `#[cfg(test)]` so production callers must keep going through
    /// the typed bridge methods (`record_vivling_*`, `prepare_vivling_*`,
    /// `mark_vivling_*`, …) instead of poking at the field directly.
    #[cfg(test)]
    pub(crate) fn vivling_for_tests(&self) -> &crate::vivling::Vivling {
        &self.vivling
    }

    pub(crate) fn vl_lifecycle_tick(
        &mut self,
        is_baby_or_juvenile: bool,
        sidebar_collapsed: bool,
        loop_tick_running: bool,
    ) -> Option<crate::vl::TickResult> {
        self.ensure_vl_lifecycle();
        // codex-vl care-effects boundary adapter: read bond tone from the
        // vivling domain and translate to a lifecycle-local enum. The
        // `vl/lifecycle` layer never imports `crate::vivling::*` — this is
        // the single crossing point per tick. If no Vivling is hatched yet,
        // the default `LifecycleVoiceTone::Neutral` is used.
        let voice_tone = self
            .vivling
            .state
            .as_ref()
            .map(|state| vivling_tone_to_lifecycle(state.bond.tone()))
            .unwrap_or_default();
        let home = self.vivling_codex_home();
        let lifecycle = self.vl_lifecycle.as_mut()?;
        let result = lifecycle.tick(
            is_baby_or_juvenile,
            sidebar_collapsed,
            loop_tick_running,
            voice_tone,
        );
        if lifecycle.should_persist() {
            if let Some(home) = home {
                let path = home.join("vivlings").join("live_stats.json");
                if let Err(err) = lifecycle.stats.save_to(&path) {
                    tracing::debug!("failed to persist vivling live stats: {err}");
                }
                lifecycle.mark_persisted();
            }
        }
        Some(result)
    }

    pub(crate) fn vl_lifecycle_observe_worker_turn(&mut self) {
        self.ensure_vl_lifecycle();
        if let Some(lifecycle) = self.vl_lifecycle.as_mut() {
            lifecycle.observe_worker_turn();
        }
    }

    /// codex-vl: surface for `ChatWidget::replace_loop_jobs_with_owner` to
    /// publish the current loop summary into both footer UI and Vivling context.
    pub(crate) fn set_loop_context_label(&mut self, label: Option<String>) {
        let pane_changed = self.loop_context_label != label;
        let composer_changed = self.composer.set_loop_context_label(label.clone());
        if pane_changed || composer_changed {
            self.loop_context_label = label.clone();
            self.request_redraw();
        }
    }

    pub(crate) fn loop_context_label(&self) -> Option<&str> {
        self.loop_context_label.as_deref()
    }

    /// codex-vl: getter for the composer's active agent label, used by
    /// `ChatWidget::sync_vivling_live_context`.
    pub(crate) fn active_agent_label(&self) -> Option<&str> {
        self.composer.active_agent_label()
    }

    pub(crate) fn set_vivling_animation_text(&self, text: String) {
        self.vivling.set_animation_text(text);
    }

    pub(crate) fn set_vivling_activity(&self, activity: crate::vl::VivlingActivity) {
        *self.vivling.activity.borrow_mut() = Some(activity);
    }

    pub(crate) fn set_vivling_live_context(
        &self,
        context: Option<crate::vivling::VivlingLiveContext>,
    ) {
        self.vivling.set_live_context(context);
    }

    pub(crate) fn is_vivling_baby_or_juvenile(&self) -> bool {
        self.vivling.state.as_ref().map_or(false, |s| {
            let level = s.level;
            level < 60 // Baby: <30, Juvenile: 30-59
        })
    }

    pub(crate) fn is_vl_sidebar_expanded(&self) -> bool {
        self.vl_sidebar.is_expanded()
    }

    /// codex-vl init factory: build the `Vivling` companion in its
    /// `unavailable()` baseline and apply the runtime configuration needed
    /// before the first hatch/setup call. Extracted from `BottomPane::new`
    /// in iter C2 (bottom_pane VL boundary): the constructor in `mod.rs`
    /// now stores the result directly into the struct, so upstream changes
    /// to the rest of `BottomPane::new` do not need to be reconciled with
    /// our Vivling-init logic.
    pub(super) fn codex_vl_make_vivling(
        frame_requester: &FrameRequester,
        animations_enabled: bool,
    ) -> crate::vivling::Vivling {
        let mut vivling = crate::vivling::Vivling::unavailable();
        vivling.configure_runtime(frame_requester.clone(), animations_enabled);
        vivling
    }

    /// codex-vl input bridge: intercept Vivling-related key events
    /// (Ctrl+J sidebar toggle, sidebar scroll while expanded) before the
    /// composer/editor keymap can consume them. Returns `true` when the
    /// event has been consumed and the caller should short-circuit the
    /// rest of `handle_key_event` with `InputResult::None`; returns
    /// `false` when the event should continue through the normal handler.
    ///
    /// Extracted from `BottomPane::handle_key_event` in iter C2 (bottom_pane
    /// VL boundary): the keymap path in `mod.rs` now stays merge-safe by
    /// delegating the Vivling intercepts to this single bridge.
    pub(super) fn codex_vl_handle_input_event(&mut self, key_event: &KeyEvent) -> bool {
        // codex-vl: Ctrl+J toggles the Vivling sidebar (open/close).
        // Intercepted here so the editor keymap (which binds Ctrl+J to
        // insert_newline) does not consume it first.
        if matches!(key_event.kind, KeyEventKind::Press)
            && key_event.code == KeyCode::Char('j')
            && key_event.modifiers.contains(KeyModifiers::CONTROL)
            && !key_event
                .modifiers
                .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT)
            && self.composer_is_empty()
        {
            self.toggle_vl_sidebar();
            return true;
        }
        if self.vl_sidebar.is_expanded()
            && matches!(key_event.kind, KeyEventKind::Press | KeyEventKind::Repeat)
        {
            let scroll_delta = match key_event.code {
                KeyCode::PageUp if key_event.modifiers.is_empty() => Some(-5),
                KeyCode::PageDown if key_event.modifiers.is_empty() => Some(5),
                KeyCode::Up if key_event.modifiers == KeyModifiers::CONTROL => Some(-1),
                KeyCode::Down if key_event.modifiers == KeyModifiers::CONTROL => Some(1),
                _ => None,
            };
            if let Some(delta) = scroll_delta {
                self.scroll_vl_sidebar(delta);
                return true;
            }
        }
        false
    }

    /// codex-vl task-running bridge: forward the bottom-pane busy state to
    /// the Vivling companion so its idle/working animation stays in sync
    /// with the upstream task lifecycle. Extracted from
    /// `BottomPane::set_task_running` in iter C2 (bottom_pane VL boundary).
    pub(super) fn codex_vl_on_task_running(&self, running: bool) {
        self.vivling.set_task_running(running);
    }

    /// codex-vl render bridge: push the optional Vivling sidebar and Vivling
    /// strip into the bottom-pane render flex container. Both widgets self-
    /// report `desired_height = 0` when not visible (sidebar collapsed via
    /// Ctrl+J, or no Vivling hatched), so each `flex2.push` becomes a no-op
    /// in the inactive state.
    ///
    /// Extracted from `BottomPane::as_renderable_with_composer_right_reserve`
    /// in iter C1 (bottom_pane VL boundary): keeps the upstream render path
    /// in `mod.rs` reduced to a single delegate line so upstream merges that
    /// touch the render layout do not have to be reconciled with our custom
    /// inserts.
    pub(super) fn codex_vl_push_render_extras<'a>(&'a self, flex2: &mut FlexRenderable<'a>) {
        // codex-vl: Vivling chat sidebar opens above the strip when
        // Ctrl+J expands it. desired_height returns 0 while collapsed,
        // so this is a no-op when the panel is closed.
        if self.vl_sidebar.should_render() {
            flex2.push(/*flex*/ 0, RenderableItem::Borrowed(&self.vl_sidebar));
        }
        // codex-vl: Vivling strip sits between the inline previews/status
        // area and the composer. The Vivling renderer self-reports
        // desired_height = 0 when no visible Vivling is hatched, so this
        // is a no-op for users who never spawned one.
        if self.vivling.should_render() {
            flex2.push(/*flex*/ 0, RenderableItem::Borrowed(&self.vivling));
        }
    }
}

#[cfg(test)]
mod tests {
    // codex-vl regression guard for the bottom_pane render boundary
    // (iter C1). `as_renderable_with_composer_right_reserve` must invoke
    // the thin bridge `self.codex_vl_push_render_extras(&mut flex2)`,
    // and the bridge body must keep borrowing both `self.vl_sidebar` and
    // `self.vivling` into the flex container via `flex2.push(...)`. We
    // pin BOTH endpoints to catch either:
    //   (a) someone deleting the hook from the render path, or
    //   (b) someone gutting `codex_vl_push_render_extras` so it no
    //       longer publishes the sidebar + strip widgets.

    const MOD_SOURCE: &str = include_str!("mod.rs");
    const VL_EXT_SOURCE: &str = include_str!("vl_ext.rs");

    #[test]
    fn render_body_invokes_codex_vl_push_render_extras() {
        let body = extract_fn_body(MOD_SOURCE, "as_renderable_with_composer_right_reserve")
            .expect("as_renderable_with_composer_right_reserve must exist in bottom_pane/mod.rs");
        assert!(
            body.contains("self.codex_vl_push_render_extras("),
            "as_renderable_with_composer_right_reserve must call \
             self.codex_vl_push_render_extras(&mut flex2) to publish the \
             Vivling sidebar + strip in the render path. Body was:\n{body}",
        );
    }

    #[test]
    fn codex_vl_push_render_extras_borrows_vl_sidebar_and_vivling() {
        let body = extract_fn_body(VL_EXT_SOURCE, "codex_vl_push_render_extras")
            .expect("codex_vl_push_render_extras must exist in vl_ext.rs");
        assert!(
            body.contains("self.vl_sidebar"),
            "codex_vl_push_render_extras must reference self.vl_sidebar. \
             Body was:\n{body}",
        );
        assert!(
            body.contains("self.vivling"),
            "codex_vl_push_render_extras must reference self.vivling. \
             Body was:\n{body}",
        );
        assert!(
            body.contains("flex2.push("),
            "codex_vl_push_render_extras must push children into flex2 via \
             flex2.push(...). Body was:\n{body}",
        );
    }

    #[test]
    fn new_body_invokes_codex_vl_make_vivling() {
        let body = extract_fn_body(MOD_SOURCE, "new")
            .expect("BottomPane::new must exist in bottom_pane/mod.rs");
        assert!(
            body.contains("Self::codex_vl_make_vivling("),
            "BottomPane::new must build the Vivling companion via \
             Self::codex_vl_make_vivling(&frame_requester, animations_enabled). \
             Body was:\n{body}",
        );
    }

    #[test]
    fn codex_vl_make_vivling_calls_unavailable_and_configure_runtime() {
        let body = extract_fn_body(VL_EXT_SOURCE, "codex_vl_make_vivling")
            .expect("codex_vl_make_vivling must exist in vl_ext.rs");
        assert!(
            body.contains("Vivling::unavailable()"),
            "codex_vl_make_vivling must start from Vivling::unavailable(). \
             Body was:\n{body}",
        );
        assert!(
            body.contains("configure_runtime("),
            "codex_vl_make_vivling must apply configure_runtime(...) to the \
             new Vivling. Body was:\n{body}",
        );
    }

    #[test]
    fn handle_key_event_invokes_codex_vl_handle_input_event() {
        let body = extract_fn_body(MOD_SOURCE, "handle_key_event")
            .expect("BottomPane::handle_key_event must exist in bottom_pane/mod.rs");
        assert!(
            body.contains("self.codex_vl_handle_input_event("),
            "BottomPane::handle_key_event must delegate Vivling intercepts \
             to self.codex_vl_handle_input_event(...) before the regular \
             keymap path. Body was:\n{body}",
        );
    }

    #[test]
    fn codex_vl_handle_input_event_handles_ctrl_j_toggle_and_sidebar_scroll() {
        let body = extract_fn_body(VL_EXT_SOURCE, "codex_vl_handle_input_event")
            .expect("codex_vl_handle_input_event must exist in vl_ext.rs");
        // Ctrl+J toggle invariants.
        assert!(
            body.contains("KeyCode::Char('j')"),
            "codex_vl_handle_input_event must intercept KeyCode::Char('j'). \
             Body was:\n{body}",
        );
        assert!(
            body.contains("KeyModifiers::CONTROL"),
            "codex_vl_handle_input_event must require KeyModifiers::CONTROL \
             on the Ctrl+J intercept. Body was:\n{body}",
        );
        assert!(
            body.contains("self.toggle_vl_sidebar()"),
            "codex_vl_handle_input_event must invoke self.toggle_vl_sidebar() \
             on the Ctrl+J intercept. Body was:\n{body}",
        );
        // Sidebar scroll invariants.
        assert!(
            body.contains("self.vl_sidebar.is_expanded()"),
            "codex_vl_handle_input_event must gate the scroll branch on \
             self.vl_sidebar.is_expanded(). Body was:\n{body}",
        );
        assert!(
            body.contains("KeyCode::PageUp"),
            "codex_vl_handle_input_event must handle KeyCode::PageUp scroll. \
             Body was:\n{body}",
        );
        assert!(
            body.contains("KeyCode::PageDown"),
            "codex_vl_handle_input_event must handle KeyCode::PageDown scroll. \
             Body was:\n{body}",
        );
        assert!(
            body.contains("self.scroll_vl_sidebar("),
            "codex_vl_handle_input_event must invoke self.scroll_vl_sidebar(...) \
             on the scroll branch. Body was:\n{body}",
        );
    }

    #[test]
    fn set_task_running_invokes_codex_vl_on_task_running() {
        let body = extract_fn_body(MOD_SOURCE, "set_task_running")
            .expect("BottomPane::set_task_running must exist in bottom_pane/mod.rs");
        assert!(
            body.contains("self.codex_vl_on_task_running("),
            "BottomPane::set_task_running must forward the running flag to \
             self.codex_vl_on_task_running(running). Body was:\n{body}",
        );
    }

    #[test]
    fn codex_vl_on_task_running_invokes_vivling_set_task_running() {
        let body = extract_fn_body(VL_EXT_SOURCE, "codex_vl_on_task_running")
            .expect("codex_vl_on_task_running must exist in vl_ext.rs");
        assert!(
            body.contains("self.vivling.set_task_running("),
            "codex_vl_on_task_running must call self.vivling.set_task_running(running). \
             Body was:\n{body}",
        );
    }

    /// Locate the body of `fn <fn_name>` in the given source, tolerating an
    /// optional generic parameter list (e.g. `fn foo<'a>(...)`). The returned
    /// slice is the text between the outermost `{` and matching `}` of the
    /// first matching function definition.
    fn extract_fn_body<'a>(source: &'a str, fn_name: &str) -> Option<&'a str> {
        let needle = format!("fn {fn_name}");
        let mut cursor = 0usize;
        loop {
            let hit = source[cursor..].find(&needle)?;
            let name_end = cursor + hit + needle.len();
            let after = source.as_bytes().get(name_end).copied()?;
            // Accept only true matches: the character following the name must
            // be `(` (regular fn) or `<` (generic fn). Otherwise this was a
            // partial-name match — keep searching.
            if matches!(after, b'(' | b'<') {
                let open = source[name_end..].find('{')? + name_end;
                let bytes = source.as_bytes();
                let mut depth = 0i32;
                for (idx, &b) in bytes.iter().enumerate().skip(open) {
                    match b {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                return Some(&source[open + 1..idx]);
                            }
                        }
                        _ => {}
                    }
                }
                return None;
            }
            cursor = name_end;
        }
    }
}
