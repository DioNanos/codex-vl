//! codex-vl loop_controller (iter B1+B2 split): facade module.
//!
//! The custom Vivling/loop runtime is being decomposed into focused
//! sub-modules to reduce blast radius on upstream merges. This file
//! keeps the existing `impl App` public surface (`pub(super)` methods)
//! and the still-not-extracted bodies (`run_loop_command_request`,
//! `process_loop_submission`, `handle_vivling_loop_tick_finished`,
//! manage_loops dynamic-tool resolver, background spawn helpers). The
//! types/parsing/formatting/state/events sub-modules are now isolated.

mod formatting;
mod parsing;
mod state;
mod types;

mod events;
mod jobs;

use super::*;
use crate::chatwidget::loop_jobs::LoopPromptSubmissionOutcome;
use crate::vivling::VivlingLoopEventKind;
use crate::vivling::VivlingLoopEventSource;
use crate::vl::VlEvent;
use crate::vl::events::LoopCommandRequest;
use codex_app_server_protocol::DynamicToolCallOutputContentItem as AppServerDynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallResponse as AppServerDynamicToolCallResponse;

use self::formatting::LOOP_STATUS_BLOCKED;
use self::formatting::LOOP_STATUS_BLOCKED_OWNER;
use self::formatting::LOOP_STATUS_BLOCKED_REVIEW;
use self::formatting::LOOP_STATUS_BLOCKED_SIDE;
use self::formatting::LOOP_STATUS_DELEGATED_VIVLING;
use self::formatting::LOOP_STATUS_DONE;
use self::formatting::LOOP_STATUS_PENDING_BUSY;
use self::formatting::LOOP_STATUS_PROGRESS;
use self::formatting::LOOP_STATUS_SUBMITTED;
use self::formatting::canonical_last_status;
use self::formatting::loop_action_failure;
use self::formatting::loop_runtime_state;
use self::parsing::is_manage_loops_dynamic_tool;
use self::parsing::parse_manage_loops_interval_seconds;
use self::parsing::parse_manage_loops_tool_request;
use self::parsing::parse_vivling_loop_status;
use self::state::loop_now_ms;
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
        if self.primary_thread_id != Some(thread_id)
            || self.chat_widget.thread_id() != Some(thread_id)
        {
            return Ok(());
        }

        let state_runtime = self.loop_state_runtime().await?;
        let Some(job) = state_runtime
            .get_thread_loop_job_by_id(thread_id, &job_id)
            .await
            .map_err(loop_state_error)?
        else {
            return Ok(());
        };
        if !job.enabled {
            return Ok(());
        }

        self.process_loop_submission(thread_id, job).await?;
        self.refresh_loop_jobs(thread_id).await
    }

    async fn process_loop_submission(
        &mut self,
        thread_id: ThreadId,
        job: codex_state::ThreadLoopJob,
    ) -> color_eyre::Result<()> {
        let state_runtime = self.loop_state_runtime().await?;
        let owner = state_runtime
            .get_thread_loop_owner(thread_id)
            .await
            .map_err(loop_state_error)?;
        let now = loop_now_ms();
        if owner.owner_kind == codex_state::THREAD_LOOP_OWNER_KIND_VIVLING {
            let Some(owner_vivling_id) = owner.owner_vivling_id.clone() else {
                state_runtime
                    .update_thread_loop_job_runtime(
                        thread_id,
                        &job.id,
                        codex_state::ThreadLoopJobRuntimeUpdate {
                            next_run_ms: None,
                            last_run_ms: job.last_run_ms,
                            last_status: Some(LOOP_STATUS_BLOCKED_OWNER.to_string()),
                            last_error: Some("Vivling loop owner is missing.".to_string()),
                            pending_tick: true,
                            updated_at_ms: now,
                        },
                    )
                    .await
                    .map_err(loop_state_error)?;
                return Ok(());
            };
            match self
                .chat_widget
                .prepare_vivling_loop_tick(&self.config, &owner_vivling_id, &job)
            {
                Ok(request) => {
                    state_runtime
                        .update_thread_loop_job_runtime(
                            thread_id,
                            &job.id,
                            codex_state::ThreadLoopJobRuntimeUpdate {
                                next_run_ms: None,
                                last_run_ms: Some(now),
                                last_status: Some(LOOP_STATUS_DELEGATED_VIVLING.to_string()),
                                last_error: None,
                                pending_tick: false,
                                updated_at_ms: now,
                            },
                        )
                        .await
                        .map_err(loop_state_error)?;
                    self.app_event_tx.send_vl(VlEvent::RunVivlingLoopTick {
                        thread_id,
                        job_id: job.id.clone(),
                        request,
                    });
                    self.record_vivling_loop_runtime(
                        &job.label,
                        Some("delegated"),
                        Some(LOOP_STATUS_DELEGATED_VIVLING),
                        job.goal_text.as_deref().or(Some(job.prompt_text.as_str())),
                        &job.created_by,
                    );
                    return Ok(());
                }
                Err(err) => {
                    state_runtime
                        .update_thread_loop_job_runtime(
                            thread_id,
                            &job.id,
                            codex_state::ThreadLoopJobRuntimeUpdate {
                                next_run_ms: None,
                                last_run_ms: job.last_run_ms,
                                last_status: Some(LOOP_STATUS_BLOCKED_OWNER.to_string()),
                                last_error: Some(err),
                                pending_tick: true,
                                updated_at_ms: now,
                            },
                        )
                        .await
                        .map_err(loop_state_error)?;
                    return Ok(());
                }
            }
        }

        let submission = self.chat_widget.submit_loop_prompt(&job, &owner);

        let (next_run_ms, pending_tick, last_status) = match submission {
            LoopPromptSubmissionOutcome::Submitted => (
                Some(now + (job.interval_seconds * 1000)),
                false,
                loop_submission_status(submission).map(str::to_string),
            ),
            LoopPromptSubmissionOutcome::BlockedUserTurn
            | LoopPromptSubmissionOutcome::BlockedReviewMode
            | LoopPromptSubmissionOutcome::BlockedSideConversation => (
                None,
                true,
                loop_submission_status(submission).map(str::to_string),
            ),
            LoopPromptSubmissionOutcome::BlockedMissingThread => {
                return Ok(());
            }
        };
        let last_status_for_event = last_status.clone();

        state_runtime
            .update_thread_loop_job_runtime(
                thread_id,
                &job.id,
                codex_state::ThreadLoopJobRuntimeUpdate {
                    next_run_ms,
                    last_run_ms: if submission == LoopPromptSubmissionOutcome::Submitted {
                        Some(now)
                    } else {
                        job.last_run_ms
                    },
                    last_status,
                    last_error: None,
                    pending_tick,
                    updated_at_ms: now,
                },
            )
            .await
            .map_err(loop_state_error)?;
        let runtime_state = if pending_tick {
            Some("pending")
        } else if next_run_ms.is_some() {
            Some("scheduled")
        } else {
            Some("unscheduled")
        };
        let goal = job.goal_text.as_deref().or(Some(job.prompt_text.as_str()));
        self.record_vivling_loop_runtime(
            &job.label,
            runtime_state,
            last_status_for_event.as_deref(),
            goal,
            &job.created_by,
        );
        Ok(())
    }

    pub(super) async fn handle_vivling_loop_tick_finished(
        &mut self,
        thread_id: ThreadId,
        job_id: String,
        result: Result<crate::vivling::VivlingLoopTickResult, String>,
    ) -> color_eyre::Result<()> {
        let state_runtime = self.loop_state_runtime().await?;
        let Some(job) = state_runtime
            .get_thread_loop_job_by_id(thread_id, &job_id)
            .await
            .map_err(loop_state_error)?
        else {
            return Ok(());
        };
        let owner = state_runtime
            .get_thread_loop_owner(thread_id)
            .await
            .map_err(loop_state_error)?;
        let owner_vivling_id = owner.owner_vivling_id.clone();
        let now = loop_now_ms();

        match result {
            Err(err) => {
                if let Some(vivling_id) = owner_vivling_id.as_deref()
                    && let Err(persist_err) = self
                        .chat_widget
                        .mark_vivling_brain_runtime_error_for(vivling_id, &err)
                {
                    tracing::warn!(
                        "failed to persist Vivling loop brain error for {vivling_id}: {persist_err}"
                    );
                }
                state_runtime
                    .update_thread_loop_job_runtime(
                        thread_id,
                        &job.id,
                        codex_state::ThreadLoopJobRuntimeUpdate {
                            next_run_ms: None,
                            last_run_ms: job.last_run_ms,
                            last_status: Some(LOOP_STATUS_BLOCKED_OWNER.to_string()),
                            last_error: Some(err.clone()),
                            pending_tick: true,
                            updated_at_ms: now,
                        },
                    )
                    .await
                    .map_err(loop_state_error)?;
                self.chat_widget
                    .add_error_message(format!("Vivling loop `{}` failed: {err}", job.label));
                self.record_vivling_loop_runtime(
                    &job.label,
                    Some("pending"),
                    Some(LOOP_STATUS_BLOCKED_OWNER),
                    job.goal_text.as_deref().or(Some(job.prompt_text.as_str())),
                    &job.created_by,
                );
                self.refresh_loop_jobs(thread_id).await?;
                return Ok(());
            }
            Ok(result) => {
                if let Some(vivling_id) = owner_vivling_id.as_deref()
                    && let Err(persist_err) = self
                        .chat_widget
                        .mark_vivling_brain_reply_for(vivling_id, &result.message)
                {
                    tracing::warn!(
                        "failed to persist Vivling loop brain reply for {vivling_id}: {persist_err}"
                    );
                }

                let status = parse_vivling_loop_status(&result.status)
                    .map_err(|err| color_eyre::eyre::eyre!(err))?;
                let action_request = self
                    .vivling_loop_tick_action_request(thread_id, &job, status, &result)
                    .map_err(|err| color_eyre::eyre::eyre!(err))?;
                let mut skipped_runtime_update = false;

                if let Some(request) = action_request {
                    if matches!(
                        &request,
                        LoopCommandRequest::Remove { .. } | LoopCommandRequest::Trigger { .. }
                    ) {
                        skipped_runtime_update = true;
                    }
                    let _ = jobs::run_command_request(
                        self,
                        thread_id,
                        request,
                        LoopCommandSource::Agent,
                    )
                    .await?;
                }

                self.chat_widget.add_info_message(
                    format!("Vivling loop `{}`: {}", job.label, result.message),
                    /*hint*/ None,
                );

                let updated_job = state_runtime
                    .get_thread_loop_job_by_id(thread_id, &job.id)
                    .await
                    .map_err(loop_state_error)?;
                if let Some(updated_job) = updated_job
                    && !skipped_runtime_update
                {
                    let (next_run_ms, pending_tick, last_error) = match status {
                        LOOP_STATUS_PROGRESS => (
                            Some(now + (updated_job.interval_seconds * 1000)),
                            false,
                            None,
                        ),
                        LOOP_STATUS_BLOCKED => (None, true, Some(result.message.clone())),
                        LOOP_STATUS_DONE => (None, false, None),
                        _ => unreachable!(),
                    };
                    state_runtime
                        .update_thread_loop_job_runtime(
                            thread_id,
                            &updated_job.id,
                            codex_state::ThreadLoopJobRuntimeUpdate {
                                next_run_ms,
                                last_run_ms: Some(now),
                                last_status: Some(status.to_string()),
                                last_error,
                                pending_tick,
                                updated_at_ms: now,
                            },
                        )
                        .await
                        .map_err(loop_state_error)?;
                    let runtime_state = if !updated_job.enabled {
                        Some("disabled")
                    } else if pending_tick {
                        Some("pending")
                    } else if next_run_ms.is_some() {
                        Some("scheduled")
                    } else {
                        Some("unscheduled")
                    };
                    self.record_vivling_loop_runtime(
                        &updated_job.label,
                        runtime_state,
                        Some(status),
                        updated_job
                            .goal_text
                            .as_deref()
                            .or(Some(updated_job.prompt_text.as_str())),
                        &updated_job.created_by,
                    );
                }

                self.refresh_loop_jobs(thread_id).await?;
            }
        }
        Ok(())
    }

    fn vivling_loop_tick_action_request(
        &self,
        _thread_id: ThreadId,
        job: &codex_state::ThreadLoopJob,
        status: &str,
        result: &crate::vivling::VivlingLoopTickResult,
    ) -> anyhow::Result<Option<LoopCommandRequest>> {
        let action = result.loop_action.as_ref().and_then(|action| {
            let trimmed = action.action.trim().to_ascii_lowercase();
            (!trimmed.is_empty() && trimmed != "none").then_some(trimmed)
        });

        let action = match (status, action) {
            (LOOP_STATUS_DONE, None) if job.auto_remove_on_completion => Some("remove".to_string()),
            (LOOP_STATUS_DONE, None) => Some("disable".to_string()),
            (_, value) => value,
        };

        let Some(action) = action else {
            return Ok(None);
        };

        let request = match action.as_str() {
            "disable" => LoopCommandRequest::Disable {
                label: job.label.clone(),
            },
            "remove" => LoopCommandRequest::Remove {
                label: job.label.clone(),
            },
            "trigger" => LoopCommandRequest::Trigger {
                label: job.label.clone(),
            },
            "update" => {
                let action = result.loop_action.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Vivling loop update action payload is missing")
                })?;
                let interval_seconds = match action.interval.as_deref() {
                    Some(interval) => Some(
                        parse_manage_loops_interval_seconds(interval).ok_or_else(|| {
                            anyhow::anyhow!(
                                "Vivling loop tick returned invalid interval `{interval}`"
                            )
                        })?,
                    ),
                    None => None,
                };
                let prompt_text = action
                    .prompt
                    .as_ref()
                    .map(|prompt| prompt.trim().to_string())
                    .filter(|prompt| !prompt.is_empty());
                let goal_text = match action.goal.as_ref() {
                    Some(goal) if goal.trim().is_empty() => Some(None),
                    Some(goal) => Some(Some(goal.trim().to_string())),
                    None => None,
                };
                LoopCommandRequest::Update {
                    label: job.label.clone(),
                    interval_seconds,
                    prompt_text,
                    goal_text,
                    auto_remove_on_completion: None,
                    enabled: action.enabled,
                }
            }
            other => {
                return Err(anyhow::anyhow!(
                    "Vivling loop tick returned unsupported action `{other}`"
                ));
            }
        };
        Ok(Some(request))
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
    ///
    /// Spawns a background task that talks to the configured Vivling brain
    /// model via `vivling_background::run_vivling_assist_request` and
    /// surfaces the reply through `VlEvent::VivlingAssistFinished`.
    pub(super) fn run_vivling_assist(&mut self, request: crate::vivling::VivlingAssistRequest) {
        let app_event_tx = self.app_event_tx.clone();
        let config = self.config.clone();
        let session_telemetry = self.session_telemetry.clone();
        tokio::spawn(async move {
            let vivling_id = request.vivling_id.clone();
            let kind = request.kind.clone();
            let result = super::vivling_background::run_vivling_assist_request(
                config,
                session_telemetry,
                request,
            )
            .await;
            app_event_tx.send_vl(crate::vl::VlEvent::VivlingAssistFinished {
                vivling_id,
                kind,
                result,
            });
        });
    }

    /// codex-vl: dispatch a Vivling-managed loop tick.
    pub(super) fn run_vivling_loop_tick(
        &mut self,
        thread_id: ThreadId,
        job_id: String,
        request: crate::vivling::VivlingLoopTickRequest,
    ) {
        let app_event_tx = self.app_event_tx.clone();
        let config = self.config.clone();
        let session_telemetry = self.session_telemetry.clone();
        tokio::spawn(async move {
            let result = super::vivling_background::run_vivling_loop_tick_request(
                config,
                session_telemetry,
                request,
            )
            .await;
            app_event_tx.send_vl(crate::vl::VlEvent::VivlingLoopTickFinished {
                thread_id,
                job_id,
                result,
            });
        });
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
