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
            if !result.animation_text.is_empty() {
                self.bottom_pane
                    .set_vivling_animation_text(result.animation_text.clone());
            }
        }
    }

    pub(crate) fn vl_lifecycle_observe_worker_turn(&mut self) {
        self.bottom_pane.vl_lifecycle_observe_worker_turn();
    }

    /// codex-vl: render a Vivling-originated chat/assist message in the main history
    /// and also push it onto the dedicated Vivling sidebar log.
    pub(crate) fn add_vivling_message(&mut self, text: String, kind: crate::vl::VivlingLogKind) {
        use ratatui::style::Stylize;
        use ratatui::text::Line;
        let vivling_id = self.bottom_pane.active_vivling_id().map(|s| s.to_string());
        let is_life = kind == crate::vl::VivlingLogKind::Life;
        self.app_event_tx
            .send_vl(crate::vl::VlEvent::SidebarPushMessage {
                kind,
                text: text.clone(),
                vivling_id,
            });
        if is_life {
            self.request_redraw();
            return;
        }
        let mut lines: Vec<Line<'static>> = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if index == 0 {
                lines.push(vec!["Vivling: ".dim(), line.to_string().into()].into());
            } else {
                lines.push(vec!["          ".dim(), line.to_string().into()].into());
            }
        }
        if lines.is_empty() {
            lines.push("Vivling".dim().into());
        }
        self.add_plain_history_lines(lines);
        self.request_redraw();
    }

    /// codex-vl: refresh the Vivling live-context summary from the current chat state.
    pub(crate) fn sync_vivling_live_context(&mut self) {
        let run_state = if self.bottom_pane.is_task_running() {
            "running"
        } else {
            "idle"
        };
        let mut context = crate::vivling::VivlingLiveContext::default();
        context.thread_title = self.thread_name.clone();
        context.cwd = self
            .config
            .cwd
            .to_str()
            .map(std::string::ToString::to_string);
        context.model = self.config.model.clone();
        context.session_id = self.thread_id.map(|id| id.to_string());
        context.run_state = Some(run_state.to_string());
        context.active_agent_label = self
            .bottom_pane
            .active_agent_label()
            .map(std::string::ToString::to_string);
        self.bottom_pane.set_vivling_live_context(Some(context));
    }
}
