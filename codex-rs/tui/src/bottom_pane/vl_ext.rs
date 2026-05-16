//! codex-vl: Vivling-related methods on `BottomPane`.
//!
//! These `impl BottomPane` blocks live in a dedicated file so that
//! upstream changes to `bottom_pane/mod.rs` (where the base struct and
//! its canonical methods live) do not need to be merged around our
//! additions. The field `vivling: Vivling` still lives on the struct
//! because Rust cannot add fields via extensions; keeping *methods*
//! isolated is the useful half of the separation.

use super::BottomPane;
use crate::legacy_core::config::Config;
use crate::render::renderable::FlexRenderable;
use crate::render::renderable::RenderableItem;
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
