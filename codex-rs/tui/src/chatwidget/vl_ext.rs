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

    pub(crate) fn push_vl_sidebar_message(
        &mut self,
        kind: crate::vl::VivlingLogKind,
        text: String,
        vivling_id: Option<String>,
    ) {
        self.bottom_pane
            .push_vl_sidebar_message(kind, text, vivling_id);
    }

    pub(crate) fn is_vivling_baby_or_juvenile(&self) -> bool {
        self.bottom_pane.is_vivling_baby_or_juvenile()
    }

    pub(crate) fn is_vl_sidebar_expanded(&self) -> bool {
        self.bottom_pane.is_vl_sidebar_expanded()
    }

    pub(crate) fn vl_lifecycle_tick(
        &mut self,
        is_baby_or_juvenile: bool,
        sidebar_collapsed: bool,
        loop_tick_running: bool,
    ) {
        if let Some(result) = self.bottom_pane.vl_lifecycle_tick(
            is_baby_or_juvenile,
            sidebar_collapsed,
            loop_tick_running,
        ) {
            self.bottom_pane.set_vivling_activity(result.activity);
            self.bottom_pane
                .set_vivling_animation_text(result.animation_text);
        }
    }

    pub(crate) fn vl_lifecycle_observe_worker_turn(&mut self) {
        self.bottom_pane.vl_lifecycle_observe_worker_turn();
    }
}
