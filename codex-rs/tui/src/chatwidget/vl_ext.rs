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

    pub(crate) fn record_vivling_brain_success(
        &mut self,
        kind: crate::vivling::VivlingBrainRequestKind,
    ) -> Result<(), String> {
        self.bottom_pane
            .record_vivling_brain_success(&self.config, kind)
    }

    pub(crate) fn record_vivling_expression_result_for(
        &mut self,
        vivling_id: &str,
        reply: &crate::vivling::VivlingExpressionResult,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), String> {
        self.bottom_pane
            .record_vivling_expression_result_for(&self.config, vivling_id, reply, now)
    }

    pub(crate) fn record_vivling_expression_failure_for(
        &mut self,
        vivling_id: &str,
    ) -> Result<(), String> {
        self.bottom_pane
            .record_vivling_expression_failure_for(&self.config, vivling_id)
    }

    /// Memory V2 Step 12.B.D.3 — best-effort Expression refresh
    /// trigger called from `record_vivling_turn_completed` and the
    /// loop-event hook. When the planner + reservation succeeds,
    /// emits `VlEvent::RunVivlingExpression` so the background runner
    /// can dispatch the LLM call. All failures are silent (no UI
    /// surface) because the Expression channel is best-effort.
    pub(crate) fn maybe_trigger_vivling_expression_refresh(&mut self) {
        if let Some(request) = self
            .bottom_pane
            .try_dispatch_vivling_expression_refresh(&self.config)
        {
            self.app_event_tx
                .send_vl(crate::vl::VlEvent::RunVivlingExpression { request });
        }
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

    /// codex-vl: per-frame Vivling lifecycle tick bridge.
    ///
    /// Invoked from `ChatWidget::pre_draw_tick` so the upstream-heavy
    /// `chatwidget.rs` only needs a single-line hook. Keeps the three
    /// `vl_lifecycle_tick` arguments centralized here, matching the
    /// regression-pinned contract.
    pub(crate) fn codex_vl_pre_draw_tick(&mut self) {
        self.vl_lifecycle_tick(
            self.is_vivling_baby_or_juvenile(),
            !self.is_vl_sidebar_expanded(),
            false,
        );
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

    pub(crate) fn dispatch_remote_control_command(&mut self, args: &str) {
        let action = match crate::vl::remote_control::parse_action(args) {
            Ok(action) => action,
            Err(error) => {
                let (message, hint) = crate::vl::remote_control::parse_error_message(error);
                self.add_info_message(message.to_string(), Some(hint.to_string()));
                return;
            }
        };
        self.add_info_message(
            format!(
                "Remote control {} requested.",
                crate::vl::remote_control::action_label(action)
            ),
            /*hint*/ None,
        );
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            let text = crate::vl::remote_control::run_action(action).await;
            tx.send(crate::app_event::AppEvent::RemoteControlResult(text));
        });
    }

    pub(crate) fn add_remote_control_output(&mut self, text: String) {
        let lines = crate::vl::remote_control::render_output(&text);
        self.add_plain_history_lines(lines);
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
        if let Some(loop_context) = self.bottom_pane.loop_context_label() {
            context.task_progress = Some(loop_context.to_string());
        }
        self.bottom_pane.set_vivling_live_context(Some(context));
    }
}

#[cfg(test)]
mod tests {
    // codex-vl regression guard: `pre_draw_tick` must invoke the Vivling
    // lifecycle bridge so the lifecycle/animation/care path keeps ticking
    // each frame. Lost during the upstream chatwidget phase-5 refactor
    // merge — pin the contract so the call cannot disappear silently.
    //
    // Boundary extraction 2026-05-15: `pre_draw_tick` now calls the thin
    // bridge `self.codex_vl_pre_draw_tick()`, which in turn must invoke
    // `self.vl_lifecycle_tick(is_vivling_baby_or_juvenile, !sidebar, false)`.
    // We pin BOTH endpoints to catch either:
    //   (a) someone deleting the hook from `pre_draw_tick`, or
    //   (b) someone gutting `codex_vl_pre_draw_tick` so it no longer
    //       drives `vl_lifecycle_tick` with the canonical arguments.

    const CHATWIDGET_SOURCE: &str = include_str!("../chatwidget.rs");
    const VL_EXT_SOURCE: &str = include_str!("vl_ext.rs");

    #[test]
    fn pre_draw_tick_invokes_codex_vl_pre_draw_tick() {
        let body = extract_fn_body(CHATWIDGET_SOURCE, "pre_draw_tick")
            .expect("pre_draw_tick must exist in chatwidget.rs");
        assert!(
            body.contains("self.codex_vl_pre_draw_tick()"),
            "pre_draw_tick must call self.codex_vl_pre_draw_tick() to drive \
             the Vivling lifecycle each frame. Body was:\n{body}",
        );
    }

    #[test]
    fn codex_vl_pre_draw_tick_invokes_vl_lifecycle_tick_with_canonical_args() {
        let body = extract_fn_body(VL_EXT_SOURCE, "codex_vl_pre_draw_tick")
            .expect("codex_vl_pre_draw_tick must exist in vl_ext.rs");
        assert!(
            body.contains("self.vl_lifecycle_tick("),
            "codex_vl_pre_draw_tick must call self.vl_lifecycle_tick(...). \
             Body was:\n{body}",
        );
        assert!(
            body.contains("self.is_vivling_baby_or_juvenile()"),
            "codex_vl_pre_draw_tick must pass is_vivling_baby_or_juvenile(). \
             Body was:\n{body}",
        );
        assert!(
            body.contains("!self.is_vl_sidebar_expanded()"),
            "codex_vl_pre_draw_tick must pass !is_vl_sidebar_expanded(). \
             Body was:\n{body}",
        );
        assert!(
            body.contains("false"),
            "codex_vl_pre_draw_tick must pass the loop_tick_running=false \
             literal. Body was:\n{body}",
        );
    }

    fn extract_fn_body<'a>(source: &'a str, fn_name: &str) -> Option<&'a str> {
        let needle = format!("fn {fn_name}(");
        let start = source.find(&needle)?;
        let open = source[start..].find('{')? + start;
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
        None
    }
}
