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
}
