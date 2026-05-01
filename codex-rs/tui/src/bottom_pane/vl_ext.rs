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
use crate::vivling::VivlingAction;
use crate::vivling::VivlingCommandOutcome;
use crate::vivling::VivlingLoopEvent;

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
        let home = self.vivling_codex_home();
        let lifecycle = self.vl_lifecycle.as_mut()?;
        let result = lifecycle.tick(is_baby_or_juvenile, sidebar_collapsed, loop_tick_running);
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

    pub(crate) fn set_vivling_animation_text(&self, text: String) {
        *self.vivling.animation_text.borrow_mut() = Some(text);
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
}
