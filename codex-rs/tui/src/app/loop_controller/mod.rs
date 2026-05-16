//! codex-vl loop_controller (iter B1..B5 split): facade module.
//!
//! The custom Vivling/loop runtime is being decomposed into focused
//! sub-modules to reduce blast radius on upstream merges. This file
//! keeps the existing `impl App` public surface (`pub(super)` methods)
//! and the still-not-extracted bodies — currently the `manage_loops`
//! dynamic-tool resolver path (`execute_manage_loops_dynamic_tool` +
//! `resolve_manage_loops_app_server_request`) and the `mod tests`
//! parser suite, both scheduled for sub-iters B6/B7.
//!
//! Sub-modules already isolated:
//! - `types` — `LoopActionOutcome`, `LoopCommandSource`.
//! - `parsing` — input parsers + `ManageLoopsToolArgs` +
//!   `is_manage_loops_dynamic_tool`.
//! - `formatting` — `LOOP_STATUS_*` constants + narrating helpers +
//!   JSON payload builders.
//! - `state` — `loop_now_ms`, `loop_state_error`.
//! - `events` — `refresh_jobs`, `handle_reload`.
//! - `jobs` — `run_command_request` (Add/Update/List/Show/Enable/
//!   Disable/Remove/Trigger/Owner CRUD dispatcher).
//! - `ticks` — `handle_tick` (former `App::handle_loop_tick` body) +
//!   `process_submission` (former `App::process_loop_submission`,
//!   includes Vivling owner-kind branch + main-path
//!   `submit_loop_prompt`).
//! - `vivling_delegation` — `handle_loop_tick_finished` (Vivling brain
//!   reply consumer with follow-up `LoopCommandRequest`),
//!   `tick_action_request` (internal helper), and the tokio spawn
//!   helpers `run_assist` / `run_loop_tick` that call into
//!   `crate::app::vivling_background::*`.

mod formatting;
mod parsing;
mod state;
mod types;

mod events;
mod jobs;
mod ticks;
mod vivling_delegation;

use super::*;
use crate::chatwidget::loop_jobs::LoopPromptSubmissionOutcome;
use crate::vivling::VivlingLoopEventKind;
use crate::vivling::VivlingLoopEventSource;
use crate::vl::events::LoopCommandRequest;
use codex_app_server_protocol::DynamicToolCallOutputContentItem as AppServerDynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallResponse as AppServerDynamicToolCallResponse;

use self::formatting::LOOP_STATUS_BLOCKED_REVIEW;
use self::formatting::LOOP_STATUS_BLOCKED_SIDE;
use self::formatting::LOOP_STATUS_PENDING_BUSY;
use self::formatting::LOOP_STATUS_SUBMITTED;
use self::formatting::canonical_last_status;
use self::formatting::loop_action_failure;
use self::formatting::loop_runtime_state;
use self::parsing::is_manage_loops_dynamic_tool;
use self::parsing::parse_manage_loops_tool_request;
use self::state::loop_state_error;
use self::types::LoopActionOutcome;
use self::types::LoopCommandSource;

fn loop_action_outcome_to_app_server_response(
    outcome: LoopActionOutcome,
) -> AppServerDynamicToolCallResponse {
    AppServerDynamicToolCallResponse {
        content_items: vec![AppServerDynamicToolCallOutputContentItem::InputText {
            text: outcome.payload.to_string(),
        }],
        success: outcome.success,
    }
}

fn loop_submission_status(outcome: LoopPromptSubmissionOutcome) -> Option<&'static str> {
    match outcome {
        LoopPromptSubmissionOutcome::Submitted => Some(LOOP_STATUS_SUBMITTED),
        LoopPromptSubmissionOutcome::BlockedMissingThread => None,
        LoopPromptSubmissionOutcome::BlockedSideConversation => Some(LOOP_STATUS_BLOCKED_SIDE),
        LoopPromptSubmissionOutcome::BlockedReviewMode => Some(LOOP_STATUS_BLOCKED_REVIEW),
        LoopPromptSubmissionOutcome::BlockedUserTurn => Some(LOOP_STATUS_PENDING_BUSY),
    }
}

impl App {
    fn record_vivling_loop_job(
        &mut self,
        action: &str,
        label: &str,
        job: Option<&codex_state::ThreadLoopJob>,
        source: LoopCommandSource,
    ) {
        let runtime_state = job.map(loop_runtime_state);
        let last_status =
            job.and_then(|job| canonical_last_status(job).as_deref().map(str::to_string));
        let goal = job.and_then(|job| job.goal_text.as_deref());
        self.chat_widget.record_vivling_loop_event(
            VivlingLoopEventKind::Config,
            match source {
                LoopCommandSource::User => VivlingLoopEventSource::User,
                LoopCommandSource::Agent => VivlingLoopEventSource::Agent,
            },
            action,
            label,
            runtime_state,
            last_status.as_deref(),
            goal,
        );
    }

    fn record_vivling_loop_runtime(
        &mut self,
        label: &str,
        runtime_state: Option<&str>,
        last_status: Option<&str>,
        goal: Option<&str>,
        created_by: &str,
    ) {
        let source = if created_by == "user" {
            VivlingLoopEventSource::User
        } else {
            VivlingLoopEventSource::Agent
        };
        self.chat_widget.record_vivling_loop_event(
            VivlingLoopEventKind::Runtime,
            source,
            "run",
            label,
            runtime_state,
            last_status,
            goal,
        );
    }

    pub(super) async fn loop_state_runtime(
        &self,
    ) -> color_eyre::Result<std::sync::Arc<codex_state::StateRuntime>> {
        codex_state::StateRuntime::init(
            self.config.codex_home.to_path_buf(),
            self.config.model_provider_id.clone(),
        )
        .await
        .map_err(loop_state_error)
    }

    pub(super) async fn refresh_loop_jobs(
        &mut self,
        thread_id: ThreadId,
    ) -> color_eyre::Result<()> {
        events::refresh_jobs(self, thread_id).await
    }

    pub(super) async fn apply_loop_command_request(
        &mut self,
        thread_id: ThreadId,
        request: LoopCommandRequest,
        source: bool,
        emit_ui_feedback: bool,
    ) -> color_eyre::Result<()> {
        let source = if source {
            LoopCommandSource::Agent
        } else {
            LoopCommandSource::User
        };
        let outcome = jobs::run_command_request(self, thread_id, request, source).await?;
        if emit_ui_feedback {
            if outcome.success {
                self.chat_widget
                    .add_info_message(outcome.message, /*hint*/ None);
            } else {
                self.chat_widget.add_error_message(outcome.message);
            }
        }
        Ok(())
    }

    pub(super) async fn handle_reload_loop_jobs(
        &mut self,
        thread_id: ThreadId,
    ) -> color_eyre::Result<()> {
        events::handle_reload(self, thread_id).await
    }

    pub(super) async fn handle_loop_tick(
        &mut self,
        thread_id: ThreadId,
        job_id: String,
    ) -> color_eyre::Result<()> {
        ticks::handle_tick(self, thread_id, job_id).await
    }

    pub(super) async fn handle_vivling_loop_tick_finished(
        &mut self,
        thread_id: ThreadId,
        job_id: String,
        result: Result<crate::vivling::VivlingLoopTickResult, String>,
    ) -> color_eyre::Result<()> {
        vivling_delegation::handle_loop_tick_finished(self, thread_id, job_id, result).await
    }

    async fn execute_manage_loops_dynamic_tool(
        &mut self,
        thread_id: ThreadId,
        arguments: serde_json::Value,
    ) -> LoopActionOutcome {
        match parse_manage_loops_tool_request(arguments) {
            Ok(request) => {
                match jobs::run_command_request(self, thread_id, request, LoopCommandSource::Agent)
                    .await
                {
                    Ok(outcome) => outcome,
                    Err(err) => loop_action_failure("unknown", thread_id, err.to_string()),
                }
            }
            Err(err) => loop_action_failure(
                "unknown",
                thread_id,
                format!("manage_loops arguments invalid: {err}"),
            ),
        }
    }

    pub(super) async fn resolve_manage_loops_app_server_request(
        &mut self,
        app_server: &AppServerSession,
        request_id: codex_app_server_protocol::RequestId,
        params: codex_app_server_protocol::DynamicToolCallParams,
    ) -> color_eyre::Result<()> {
        let thread_id = ThreadId::from_string(&params.thread_id)?;
        let outcome = if is_manage_loops_dynamic_tool(params.namespace.as_deref(), &params.tool) {
            self.execute_manage_loops_dynamic_tool(thread_id, params.arguments)
                .await
        } else {
            loop_action_failure(
                "unknown",
                thread_id,
                format!(
                    "Dynamic tool `{}{}` is not available in TUI yet.",
                    params
                        .namespace
                        .as_deref()
                        .map(|namespace| format!("{namespace}::"))
                        .unwrap_or_default(),
                    params.tool
                ),
            )
        };
        app_server
            .resolve_server_request(
                request_id,
                serde_json::to_value(loop_action_outcome_to_app_server_response(outcome))?,
            )
            .await?;
        Ok(())
    }

    /// codex-vl: dispatch a Vivling brain assist request.
    pub(super) fn run_vivling_assist(&mut self, request: crate::vivling::VivlingAssistRequest) {
        vivling_delegation::run_assist(self, request);
    }

    /// codex-vl: dispatch a Vivling-managed loop tick.
    pub(super) fn run_vivling_loop_tick(
        &mut self,
        thread_id: ThreadId,
        job_id: String,
        request: crate::vivling::VivlingLoopTickRequest,
    ) {
        vivling_delegation::run_loop_tick(self, thread_id, job_id, request);
    }
}

#[cfg(test)]
mod tests {
    use super::formatting::loop_action_success;
    use super::formatting::loop_job_json;
    use super::*;

    fn sample_job() -> codex_state::ThreadLoopJob {
        codex_state::ThreadLoopJob {
            id: "job-1".to_string(),
            thread_id: ThreadId::new(),
            label: "forge".to_string(),
            prompt_text: "check forge".to_string(),
            goal_text: Some("watch package pipeline".to_string()),
            interval_seconds: 300,
            enabled: true,
            run_policy: "queue_one".to_string(),
            auto_remove_on_completion: true,
            created_by: "agent".to_string(),
            next_run_ms: Some(1_700_000_300_000),
            last_run_ms: Some(1_700_000_000_000),
            last_status: Some("pending".to_string()),
            last_error: None,
            pending_tick: false,
            created_at_ms: 1_700_000_000_000,
            updated_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn parse_manage_loops_add_request() {
        let request = parse_manage_loops_tool_request(serde_json::json!({
            "action": "add",
            "label": "forge",
            "interval": "5m",
            "prompt": "check forge"
        }))
        .expect("valid request");

        assert_eq!(
            request,
            LoopCommandRequest::Add {
                label: "forge".to_string(),
                interval_seconds: 300,
                prompt_text: "check forge".to_string(),
                goal_text: None,
                auto_remove_on_completion: None,
            }
        );
    }

    #[test]
    fn manage_loops_dynamic_tool_accepts_flat_and_namespaced_aliases() {
        assert!(is_manage_loops_dynamic_tool(None, "manage_loops"));
        assert!(is_manage_loops_dynamic_tool(
            Some("codex_app"),
            "manage_loops"
        ));
        assert!(is_manage_loops_dynamic_tool(
            Some("functions"),
            "manage_loops"
        ));
        assert!(!is_manage_loops_dynamic_tool(
            Some("other_namespace"),
            "manage_loops"
        ));
        assert!(!is_manage_loops_dynamic_tool(None, "other_tool"));
    }

    #[test]
    fn parse_manage_loops_add_request_with_goal_and_cleanup() {
        let request = parse_manage_loops_tool_request(serde_json::json!({
            "action": "add",
            "label": "forge",
            "interval": "5m",
            "prompt": "check forge",
            "goal": "watch package pipeline",
            "auto_remove_on_completion": true
        }))
        .expect("valid request");

        assert_eq!(
            request,
            LoopCommandRequest::Add {
                label: "forge".to_string(),
                interval_seconds: 300,
                prompt_text: "check forge".to_string(),
                goal_text: Some("watch package pipeline".to_string()),
                auto_remove_on_completion: Some(true),
            }
        );
    }

    #[test]
    fn parse_manage_loops_update_request_supports_partial_updates() {
        let request = parse_manage_loops_tool_request(serde_json::json!({
            "action": "update",
            "label": "forge",
            "goal": null,
            "enabled": false
        }))
        .expect("valid request");

        assert_eq!(
            request,
            LoopCommandRequest::Update {
                label: "forge".to_string(),
                interval_seconds: None,
                prompt_text: None,
                goal_text: Some(None),
                auto_remove_on_completion: None,
                enabled: Some(false),
            }
        );
    }

    #[test]
    fn parse_manage_loops_trigger_request() {
        let request = parse_manage_loops_tool_request(serde_json::json!({
            "action": "trigger",
            "label": "forge"
        }))
        .expect("valid request");

        assert_eq!(
            request,
            LoopCommandRequest::Trigger {
                label: "forge".to_string(),
            }
        );
    }

    #[test]
    fn parse_manage_loops_rejects_short_interval() {
        let error = parse_manage_loops_tool_request(serde_json::json!({
            "action": "add",
            "label": "forge",
            "interval": "5s",
            "prompt": "check forge"
        }))
        .expect_err("interval should be rejected");

        assert!(error.to_string().contains("interval"));
    }

    #[test]
    fn loop_job_json_includes_runtime_state_and_normalized_status() {
        let json = loop_job_json(&sample_job());

        assert_eq!(json["runtime_state"], "scheduled");
        assert_eq!(json["last_status"], "pending_busy");
    }

    #[test]
    fn app_server_response_uses_json_payload() {
        let response = loop_action_outcome_to_app_server_response(loop_action_success(
            "show",
            ThreadId::new(),
            "ok".to_string(),
            Some(&sample_job()),
            None,
        ));

        let [AppServerDynamicToolCallOutputContentItem::InputText { text }] =
            response.content_items.as_slice()
        else {
            panic!("expected text payload");
        };
        let parsed: serde_json::Value =
            serde_json::from_str(text).expect("tool response should be JSON");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["action"], "show");
        assert_eq!(parsed["job"]["label"], "forge");
    }
}
