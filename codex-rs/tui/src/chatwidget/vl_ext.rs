//! codex-vl: Vivling-related pass-through methods on `ChatWidget`.
//!
//! Loop-job runtime management lives in `chatwidget/loop_jobs.rs`, so
//! this file focuses on the brain-profile and loop-owner forwards
//! that relay between the widget and `BottomPane`. Keeping them out of
//! `chatwidget.rs` means upstream edits to the main widget don't have
//! to be merged around our code.

use super::ChatWidget;
use crate::legacy_core::config::Config;

impl ChatWidget {
    pub(crate) fn assign_vivling_brain_profile(
        &mut self,
        profile: String,
    ) -> Result<String, String> {
        self.bottom_pane
            .assign_vivling_brain_profile(&self.config, profile)
    }

    pub(crate) fn mark_vivling_brain_runtime_error(&mut self, error: &str) -> Result<(), String> {
        self.bottom_pane
            .mark_vivling_brain_runtime_error(&self.config, error)
    }

    pub(crate) fn mark_vivling_brain_runtime_error_for(
        &mut self,
        vivling_id: &str,
        error: &str,
    ) -> Result<(), String> {
        self.bottom_pane
            .mark_vivling_brain_runtime_error_for(&self.config, vivling_id, error)
    }

    pub(crate) fn mark_vivling_brain_reply(&mut self, reply: &str) -> Result<(), String> {
        self.bottom_pane
            .mark_vivling_brain_reply(&self.config, reply)
    }

    pub(crate) fn mark_vivling_brain_reply_for(
        &mut self,
        vivling_id: &str,
        reply: &str,
    ) -> Result<(), String> {
        self.bottom_pane
            .mark_vivling_brain_reply_for(&self.config, vivling_id, reply)
    }

    pub(crate) fn active_vivling_loop_owner_identity(
        &mut self,
        config: &Config,
    ) -> Result<(String, String), String> {
        self.bottom_pane.active_vivling_loop_owner_identity(config)
    }

    pub(crate) fn prepare_vivling_loop_tick(
        &mut self,
        config: &Config,
        owner_vivling_id: &str,
        job: &codex_state::ThreadLoopJob,
    ) -> Result<crate::vivling::VivlingLoopTickRequest, String> {
        self.bottom_pane
            .prepare_vivling_loop_tick(config, owner_vivling_id, job)
    }
}
