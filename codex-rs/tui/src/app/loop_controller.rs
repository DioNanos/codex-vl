use super::*;
use crate::app_event::LoopCommandRequest;
use crate::chatwidget::LoopPromptSubmissionOutcome;
use crate::vivling::VivlingLoopEventKind;
use crate::vivling::VivlingLoopEventSource;
use crate::vl::VlEvent;
use codex_app_server_protocol::DynamicToolCallOutputContentItem as AppServerDynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallResponse as AppServerDynamicToolCallResponse;

const MANAGE_LOOPS_TOOL_NAMESPACE: &str = "codex_app";
const MANAGE_LOOPS_TOOL_NAME: &str = "manage_loops";

const LOOP_STATUS_SUBMITTED: &str = "submitted";
const LOOP_STATUS_PENDING_BUSY: &str = "pending_busy";
const LOOP_STATUS_BLOCKED_REVIEW: &str = "blocked_review";
const LOOP_STATUS_BLOCKED_SIDE: &str = "blocked_side";
const LOOP_STATUS_BLOCKED_OWNER: &str = "blocked_owner";
const LOOP_STATUS_DELEGATED_VIVLING: &str = "delegated_vivling";
const LOOP_STATUS_PROGRESS: &str = "progress";
const LOOP_STATUS_BLOCKED: &str = "blocked";
const LOOP_STATUS_DONE: &str = "done";
const LOOP_STATUS_DISABLED: &str = "disabled";
const LOOP_STATUS_REMOVED: &str = "removed";

#[derive(Debug)]
struct LoopActionOutcome {
    success: bool,
    message: String,
    payload: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopCommandSource {
    User,
    Agent,
}

#[derive(Debug, serde::Deserialize)]
struct ManageLoopsToolArgs {
    action: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    interval: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    auto_remove_on_completion: Option<bool>,
    #[serde(default)]
    enabled: Option<bool>,
}

pub(super) fn loop_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn format_loop_interval(interval_seconds: i64) -> String {
    if interval_seconds % 3600 == 0 {
        format!("{}h", interval_seconds / 3600)
    } else if interval_seconds % 60 == 0 {
        format!("{}m", interval_seconds / 60)
    } else {
        format!("{interval_seconds}s")
    }
}

fn canonical_last_status(job: &codex_state::ThreadLoopJob) -> Option<String> {
    match job.last_status.as_deref() {
        Some("pending") => Some(LOOP_STATUS_PENDING_BUSY.to_string()),
        Some(status) if !status.trim().is_empty() => Some(status.to_string()),
        _ => None,
    }
}

fn loop_runtime_state(job: &codex_state::ThreadLoopJob) -> &'static str {
    if !job.enabled {
        "disabled"
    } else if job.pending_tick {
        "pending"
    } else if job.next_run_ms.is_some() {
        "scheduled"
    } else {
        "unscheduled"
    }
}

fn thread_loop_owner_summary(owner: &codex_state::ThreadLoopOwner) -> String {
    match owner.owner_kind.as_str() {
        codex_state::THREAD_LOOP_OWNER_KIND_VIVLING => format!(
            "vivling ({})",
            owner.owner_vivling_id.as_deref().unwrap_or("missing")
        ),
        _ => codex_state::THREAD_LOOP_OWNER_KIND_MAIN.to_string(),
    }
}

fn format_loop_job_line(job: &codex_state::ThreadLoopJob) -> String {
    let enabled = if job.enabled { "on" } else { "off" };
    let status = canonical_last_status(job).unwrap_or_else(|| "never".to_string());
    let cleanup = if job.auto_remove_on_completion {
        "auto-remove"
    } else {
        "keep"
    };
    format!(
        "{} [{}] every {} | runtime={} | status={} | {}",
        job.label,
        enabled,
        format_loop_interval(job.interval_seconds),
        loop_runtime_state(job),
        status,
        cleanup
    )
}

fn summarize_loop_goal(job: &codex_state::ThreadLoopJob) -> String {
    job.goal_text
        .as_deref()
        .filter(|goal| !goal.trim().is_empty())
        .unwrap_or(&job.prompt_text)
        .to_string()
}

fn format_loop_job_details(job: &codex_state::ThreadLoopJob) -> String {
    let next_run = job
        .next_run_ms
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string());
    let last_run = job
        .last_run_ms
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string());
    let last_status = canonical_last_status(job).unwrap_or_else(|| "never".to_string());
    let last_error = job.last_error.as_deref().unwrap_or("none");
    let goal = summarize_loop_goal(job);
    format!(
        "label: {}\nenabled: {}\ninterval: {}\nruntime_state: {}\nlast_status: {}\npending: {}\nauto_remove_on_completion: {}\ncreated_by: {}\ngoal: {}\nprompt: {}\nnext_run_ms: {}\nlast_run_ms: {}\nlast_error: {}",
        job.label,
        job.enabled,
        format_loop_interval(job.interval_seconds),
        loop_runtime_state(job),
        last_status,
        job.pending_tick,
        job.auto_remove_on_completion,
        job.created_by,
        goal,
        job.prompt_text,
        next_run,
        last_run,
        last_error
    )
}

fn loop_job_json(job: &codex_state::ThreadLoopJob) -> serde_json::Value {
    serde_json::json!({
        "label": job.label,
        "enabled": job.enabled,
        "interval_seconds": job.interval_seconds,
        "prompt_text": job.prompt_text,
        "goal_text": job.goal_text,
        "auto_remove_on_completion": job.auto_remove_on_completion,
        "created_by": job.created_by,
        "run_policy": job.run_policy,
        "runtime_state": loop_runtime_state(job),
        "last_status": canonical_last_status(job),
        "last_error": job.last_error,
        "pending_tick": job.pending_tick,
        "next_run_ms": job.next_run_ms,
        "last_run_ms": job.last_run_ms,
    })
}

fn loop_success_payload(
    action: &str,
    thread_id: ThreadId,
    job: Option<&codex_state::ThreadLoopJob>,
    jobs: Option<Vec<serde_json::Value>>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "ok": true,
        "action": action,
        "thread_id": thread_id.to_string(),
    });
    let object = payload.as_object_mut().expect("payload must be object");
    if let Some(job) = job {
        object.insert("job".to_string(), loop_job_json(job));
    }
    if let Some(jobs) = jobs {
        object.insert("jobs".to_string(), serde_json::Value::Array(jobs));
    }
    payload
}

fn loop_error_payload(action: &str, thread_id: ThreadId, error: String) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "action": action,
        "thread_id": thread_id.to_string(),
        "error": error,
    })
}

fn loop_action_success(
    action: &str,
    thread_id: ThreadId,
    message: String,
    job: Option<&codex_state::ThreadLoopJob>,
    jobs: Option<Vec<serde_json::Value>>,
) -> LoopActionOutcome {
    LoopActionOutcome {
        success: true,
        message,
        payload: loop_success_payload(action, thread_id, job, jobs),
    }
}

fn loop_action_failure(action: &str, thread_id: ThreadId, message: String) -> LoopActionOutcome {
    LoopActionOutcome {
        success: false,
        payload: loop_error_payload(action, thread_id, message.clone()),
        message,
    }
}

pub(super) fn loop_state_error(err: anyhow::Error) -> color_eyre::Report {
    color_eyre::eyre::eyre!("{err}")
}

fn parse_manage_loops_interval_seconds(token: &str) -> Option<i64> {
    if token.len() < 2 {
        return None;
    }
    let (value, unit) = token.split_at(token.len() - 1);
    let value = value.parse::<i64>().ok()?;
    let seconds = match unit {
        "s" => value,
        "m" => value * 60,
        "h" => value * 3600,
        _ => return None,
    };
    ((30..=86_400).contains(&seconds)).then_some(seconds)
}

fn parse_vivling_loop_status(status: &str) -> anyhow::Result<&'static str> {
    match status.trim().to_ascii_lowercase().as_str() {
        LOOP_STATUS_PROGRESS => Ok(LOOP_STATUS_PROGRESS),
        LOOP_STATUS_BLOCKED => Ok(LOOP_STATUS_BLOCKED),
        LOOP_STATUS_DONE => Ok(LOOP_STATUS_DONE),
        other => Err(anyhow::anyhow!(
            "Vivling loop tick returned unsupported status `{other}`"
        )),
    }
}

fn parse_add_goal(raw_goal: Option<serde_json::Value>) -> anyhow::Result<Option<String>> {
    match raw_goal {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(goal)) if !goal.trim().is_empty() => Ok(Some(goal)),
        Some(serde_json::Value::String(_)) => {
            Err(anyhow::anyhow!("`goal` cannot be empty when provided"))
        }
        Some(_) => Err(anyhow::anyhow!("`goal` must be a string or null")),
    }
}

fn parse_update_goal(
    raw_goal: Option<serde_json::Value>,
) -> anyhow::Result<Option<Option<String>>> {
    match raw_goal {
        None => Ok(None),
        Some(serde_json::Value::Null) => Ok(Some(None)),
        Some(serde_json::Value::String(goal)) if !goal.trim().is_empty() => Ok(Some(Some(goal))),
        Some(serde_json::Value::String(_)) => {
            Err(anyhow::anyhow!("`goal` cannot be empty when provided"))
        }
        Some(_) => Err(anyhow::anyhow!("`goal` must be a string or null")),
    }
}

fn parse_manage_loops_tool_request(
    arguments: serde_json::Value,
) -> anyhow::Result<LoopCommandRequest> {
    let goal_argument = arguments
        .as_object()
        .and_then(|object| object.get("goal"))
        .cloned();
    let args: ManageLoopsToolArgs = serde_json::from_value(arguments)?;
    let action = args.action.trim().to_ascii_lowercase();
    match action.as_str() {
        "list" | "ls" => Ok(LoopCommandRequest::List),
        "show" => Ok(LoopCommandRequest::Show {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for show"))?,
        }),
        "enable" | "on" => Ok(LoopCommandRequest::Enable {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for enable"))?,
        }),
        "disable" | "off" => Ok(LoopCommandRequest::Disable {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for disable"))?,
        }),
        "remove" | "rm" => Ok(LoopCommandRequest::Remove {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for remove"))?,
        }),
        "trigger" => Ok(LoopCommandRequest::Trigger {
            label: args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for trigger"))?,
        }),
        "add" => {
            let label = args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for add"))?;
            let interval_seconds = parse_manage_loops_interval_seconds(
                args.interval
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("`interval` is required for add"))?,
            )
            .ok_or_else(|| anyhow::anyhow!("`interval` must be between 30s and 24h"))?;
            let prompt_text = args
                .prompt
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`prompt` is required for add"))?;
            Ok(LoopCommandRequest::Add {
                label,
                interval_seconds,
                prompt_text,
                goal_text: parse_add_goal(goal_argument)?,
                auto_remove_on_completion: args.auto_remove_on_completion,
            })
        }
        "update" => {
            let label = args
                .label
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("`label` is required for update"))?;
            let interval_seconds = match args.interval {
                Some(interval) => Some(
                    parse_manage_loops_interval_seconds(&interval)
                        .ok_or_else(|| anyhow::anyhow!("`interval` must be between 30s and 24h"))?,
                ),
                None => None,
            };
            let prompt_text = match args.prompt {
                Some(prompt) if !prompt.trim().is_empty() => Some(prompt),
                Some(_) => return Err(anyhow::anyhow!("`prompt` cannot be empty when provided")),
                None => None,
            };
            Ok(LoopCommandRequest::Update {
                label,
                interval_seconds,
                prompt_text,
                goal_text: parse_update_goal(goal_argument)?,
                auto_remove_on_completion: args.auto_remove_on_completion,
                enabled: args.enabled,
            })
        }
        other => Err(anyhow::anyhow!("unsupported manage_loops action `{other}`")),
    }
}

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
        let state_runtime = self.loop_state_runtime().await?;
        let jobs = state_runtime
            .list_thread_loop_jobs(thread_id)
            .await
            .map_err(loop_state_error)?;
        let owner = state_runtime
            .get_thread_loop_owner(thread_id)
            .await
            .map_err(loop_state_error)?;
        self.chat_widget
            .replace_loop_jobs_with_owner(thread_id, jobs, owner);
        Ok(())
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
        let outcome = self
            .run_loop_command_request(thread_id, request, source)
            .await?;
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
        if self.primary_thread_id != Some(thread_id)
            || self.chat_widget.thread_id() != Some(thread_id)
        {
            self.chat_widget.clear_loop_jobs();
            return Ok(());
        }

        let state_runtime = self.loop_state_runtime().await?;
        let jobs = state_runtime
            .list_thread_loop_jobs(thread_id)
            .await
            .map_err(loop_state_error)?;

        if let Some(pending_job) = jobs
            .iter()
            .find(|job| job.enabled && job.pending_tick)
            .cloned()
        {
            self.process_loop_submission(thread_id, pending_job).await?;
        }

        self.refresh_loop_jobs(thread_id).await
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
                    let _ = self
                        .run_loop_command_request(thread_id, request, LoopCommandSource::Agent)
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

    async fn run_loop_command_request(
        &mut self,
        thread_id: ThreadId,
        request: LoopCommandRequest,
        source: LoopCommandSource,
    ) -> color_eyre::Result<LoopActionOutcome> {
        if self.primary_thread_id != Some(thread_id) || self.active_thread_id != Some(thread_id) {
            return Ok(loop_action_failure(
                "guard",
                thread_id,
                "Loop commands are only available on the active primary thread.".to_string(),
            ));
        }

        let state_runtime = self.loop_state_runtime().await?;
        let outcome = match request {
            LoopCommandRequest::Add {
                label,
                interval_seconds,
                prompt_text,
                goal_text,
                auto_remove_on_completion,
            } => {
                let now = loop_now_ms();
                let goal_text = goal_text
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .or_else(|| Some(prompt_text.trim().to_string()));
                let auto_remove_on_completion = auto_remove_on_completion.unwrap_or(true);
                let created_by = match source {
                    LoopCommandSource::User => "user",
                    LoopCommandSource::Agent => "agent",
                }
                .to_string();
                let job = state_runtime
                    .create_or_replace_thread_loop_job(codex_state::ThreadLoopJobCreateParams {
                        id: Uuid::new_v4().to_string(),
                        thread_id,
                        label: label.clone(),
                        prompt_text,
                        goal_text: goal_text.clone(),
                        interval_seconds,
                        enabled: true,
                        run_policy: "queue_one".to_string(),
                        auto_remove_on_completion,
                        created_by,
                        next_run_ms: Some(now + (interval_seconds * 1000)),
                        created_at_ms: now,
                        updated_at_ms: now,
                    })
                    .await
                    .map_err(loop_state_error)?;
                self.record_vivling_loop_job("add", &label, Some(&job), source);
                loop_action_success(
                    "add",
                    thread_id,
                    format!(
                        "Loop `{label}` saved every {}.\ngoal: {}\nauto_remove_on_completion: {}",
                        format_loop_interval(interval_seconds),
                        goal_text.unwrap_or_else(|| "none".to_string()),
                        auto_remove_on_completion
                    ),
                    Some(&job),
                    None,
                )
            }
            LoopCommandRequest::Update {
                label,
                interval_seconds,
                prompt_text,
                goal_text,
                auto_remove_on_completion,
                enabled,
            } => {
                let Some(existing) = state_runtime
                    .get_thread_loop_job_by_label(thread_id, &label)
                    .await
                    .map_err(loop_state_error)?
                else {
                    return Ok(loop_action_failure(
                        "update",
                        thread_id,
                        format!("Loop `{label}` not found."),
                    ));
                };
                let now = loop_now_ms();
                let prompt_text = prompt_text.unwrap_or_else(|| existing.prompt_text.clone());
                let goal_text = match goal_text {
                    Some(next_goal) => next_goal,
                    None => existing.goal_text.clone(),
                };
                let interval_seconds = interval_seconds.unwrap_or(existing.interval_seconds);
                let enabled = enabled.unwrap_or(existing.enabled);
                let auto_remove_on_completion =
                    auto_remove_on_completion.unwrap_or(existing.auto_remove_on_completion);
                let job = state_runtime
                    .create_or_replace_thread_loop_job(codex_state::ThreadLoopJobCreateParams {
                        id: existing.id.clone(),
                        thread_id,
                        label: existing.label.clone(),
                        prompt_text,
                        goal_text,
                        interval_seconds,
                        enabled,
                        run_policy: existing.run_policy.clone(),
                        auto_remove_on_completion,
                        created_by: existing.created_by.clone(),
                        next_run_ms: if enabled {
                            Some(now + (interval_seconds * 1000))
                        } else {
                            None
                        },
                        created_at_ms: existing.created_at_ms,
                        updated_at_ms: now,
                    })
                    .await
                    .map_err(loop_state_error)?;
                self.record_vivling_loop_job("update", &label, Some(&job), source);
                loop_action_success(
                    "update",
                    thread_id,
                    format!("Loop `{label}` updated."),
                    Some(&job),
                    None,
                )
            }
            LoopCommandRequest::List => {
                let jobs = state_runtime
                    .list_thread_loop_jobs(thread_id)
                    .await
                    .map_err(loop_state_error)?;
                let message = if jobs.is_empty() {
                    "No loops configured for this thread.".to_string()
                } else {
                    jobs.iter()
                        .map(|job| {
                            format!(
                                "{}\ngoal: {}",
                                format_loop_job_line(job),
                                summarize_loop_goal(job)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                loop_action_success(
                    "list",
                    thread_id,
                    message,
                    None,
                    Some(jobs.iter().map(loop_job_json).collect()),
                )
            }
            LoopCommandRequest::Show { label } => {
                if let Some(job) = state_runtime
                    .get_thread_loop_job_by_label(thread_id, &label)
                    .await
                    .map_err(loop_state_error)?
                {
                    loop_action_success(
                        "show",
                        thread_id,
                        format_loop_job_details(&job),
                        Some(&job),
                        None,
                    )
                } else {
                    loop_action_failure("show", thread_id, format!("Loop `{label}` not found."))
                }
            }
            LoopCommandRequest::Enable { label } => {
                if let Some(job) = state_runtime
                    .get_thread_loop_job_by_label(thread_id, &label)
                    .await
                    .map_err(loop_state_error)?
                {
                    let now = loop_now_ms();
                    state_runtime
                        .set_thread_loop_job_enabled(
                            thread_id,
                            &label,
                            true,
                            Some(now + (job.interval_seconds * 1000)),
                            now,
                        )
                        .await
                        .map_err(loop_state_error)?;
                    state_runtime
                        .update_thread_loop_job_runtime(
                            thread_id,
                            &job.id,
                            codex_state::ThreadLoopJobRuntimeUpdate {
                                next_run_ms: Some(now + (job.interval_seconds * 1000)),
                                last_run_ms: job.last_run_ms,
                                last_status: None,
                                last_error: None,
                                pending_tick: false,
                                updated_at_ms: now,
                            },
                        )
                        .await
                        .map_err(loop_state_error)?;
                    let updated = state_runtime
                        .get_thread_loop_job_by_label(thread_id, &label)
                        .await
                        .map_err(loop_state_error)?
                        .expect("loop should still exist after enable");
                    self.record_vivling_loop_job("enable", &label, Some(&updated), source);
                    loop_action_success(
                        "enable",
                        thread_id,
                        format!("Loop `{label}` enabled."),
                        Some(&updated),
                        None,
                    )
                } else {
                    loop_action_failure("enable", thread_id, format!("Loop `{label}` not found."))
                }
            }
            LoopCommandRequest::Disable { label } => {
                if let Some(job) = state_runtime
                    .get_thread_loop_job_by_label(thread_id, &label)
                    .await
                    .map_err(loop_state_error)?
                {
                    let now = loop_now_ms();
                    state_runtime
                        .set_thread_loop_job_enabled(thread_id, &label, false, None, now)
                        .await
                        .map_err(loop_state_error)?;
                    state_runtime
                        .update_thread_loop_job_runtime(
                            thread_id,
                            &job.id,
                            codex_state::ThreadLoopJobRuntimeUpdate {
                                next_run_ms: None,
                                last_run_ms: job.last_run_ms,
                                last_status: Some(LOOP_STATUS_DISABLED.to_string()),
                                last_error: None,
                                pending_tick: false,
                                updated_at_ms: now,
                            },
                        )
                        .await
                        .map_err(loop_state_error)?;
                    let updated = state_runtime
                        .get_thread_loop_job_by_label(thread_id, &label)
                        .await
                        .map_err(loop_state_error)?
                        .expect("loop should still exist after disable");
                    self.record_vivling_loop_job("disable", &label, Some(&updated), source);
                    loop_action_success(
                        "disable",
                        thread_id,
                        format!("Loop `{label}` disabled."),
                        Some(&updated),
                        None,
                    )
                } else {
                    loop_action_failure("disable", thread_id, format!("Loop `{label}` not found."))
                }
            }
            LoopCommandRequest::Remove { label } => {
                if state_runtime
                    .get_thread_loop_job_by_label(thread_id, &label)
                    .await
                    .map_err(loop_state_error)?
                    .is_some()
                {
                    state_runtime
                        .delete_thread_loop_job(thread_id, &label)
                        .await
                        .map_err(loop_state_error)?;
                    self.record_vivling_loop_job("remove", &label, None, source);
                    LoopActionOutcome {
                        success: true,
                        message: format!("Loop `{label}` removed."),
                        payload: serde_json::json!({
                            "ok": true,
                            "action": "remove",
                            "thread_id": thread_id.to_string(),
                            "job": {
                                "label": label,
                                "runtime_state": "disabled",
                                "last_status": LOOP_STATUS_REMOVED,
                            }
                        }),
                    }
                } else {
                    loop_action_failure("remove", thread_id, format!("Loop `{label}` not found."))
                }
            }
            LoopCommandRequest::Trigger { label } => {
                let Some(job) = state_runtime
                    .get_thread_loop_job_by_label(thread_id, &label)
                    .await
                    .map_err(loop_state_error)?
                else {
                    return Ok(loop_action_failure(
                        "trigger",
                        thread_id,
                        format!("Loop `{label}` not found."),
                    ));
                };
                if !job.enabled {
                    return Ok(loop_action_failure(
                        "trigger",
                        thread_id,
                        format!("Loop `{label}` is disabled."),
                    ));
                }
                let now = loop_now_ms();
                state_runtime
                    .update_thread_loop_job_runtime(
                        thread_id,
                        &job.id,
                        codex_state::ThreadLoopJobRuntimeUpdate {
                            next_run_ms: None,
                            last_run_ms: job.last_run_ms,
                            last_status: canonical_last_status(&job),
                            last_error: None,
                            pending_tick: true,
                            updated_at_ms: now,
                        },
                    )
                    .await
                    .map_err(loop_state_error)?;
                let updated = state_runtime
                    .get_thread_loop_job_by_label(thread_id, &label)
                    .await
                    .map_err(loop_state_error)?
                    .expect("loop should still exist after trigger");
                self.record_vivling_loop_job("trigger", &label, Some(&updated), source);
                loop_action_success(
                    "trigger",
                    thread_id,
                    format!("Loop `{label}` queued for the next safe run."),
                    Some(&updated),
                    None,
                )
            }
            LoopCommandRequest::OwnerShow => {
                let owner = state_runtime
                    .get_thread_loop_owner(thread_id)
                    .await
                    .map_err(loop_state_error)?;
                loop_action_success(
                    "owner",
                    thread_id,
                    format!("Loop owner: {}.", thread_loop_owner_summary(&owner)),
                    None,
                    None,
                )
            }
            LoopCommandRequest::OwnerSetMain => {
                let owner = state_runtime
                    .set_thread_loop_owner(codex_state::ThreadLoopOwner {
                        thread_id,
                        owner_kind: codex_state::THREAD_LOOP_OWNER_KIND_MAIN.to_string(),
                        owner_vivling_id: None,
                        updated_at_ms: loop_now_ms(),
                    })
                    .await
                    .map_err(loop_state_error)?;
                loop_action_success(
                    "owner",
                    thread_id,
                    format!("Loop owner set to {}.", thread_loop_owner_summary(&owner)),
                    None,
                    None,
                )
            }
            LoopCommandRequest::OwnerSetVivling => {
                let (vivling_id, vivling_name): (String, String) = self
                    .chat_widget
                    .active_vivling_loop_owner_identity(&self.config)
                    .map_err(|err| color_eyre::eyre::eyre!(err))?;
                let owner = state_runtime
                    .set_thread_loop_owner(codex_state::ThreadLoopOwner {
                        thread_id,
                        owner_kind: codex_state::THREAD_LOOP_OWNER_KIND_VIVLING.to_string(),
                        owner_vivling_id: Some(vivling_id.clone()),
                        updated_at_ms: loop_now_ms(),
                    })
                    .await
                    .map_err(loop_state_error)?;
                loop_action_success(
                    "owner",
                    thread_id,
                    format!(
                        "Loop owner set to vivling `{vivling_name}` ({vivling_id}); runtime owner is {}.",
                        thread_loop_owner_summary(&owner)
                    ),
                    None,
                    None,
                )
            }
        };

        self.refresh_loop_jobs(thread_id).await?;
        Ok(outcome)
    }

    async fn execute_manage_loops_dynamic_tool(
        &mut self,
        thread_id: ThreadId,
        arguments: serde_json::Value,
    ) -> LoopActionOutcome {
        match parse_manage_loops_tool_request(arguments) {
            Ok(request) => match self
                .run_loop_command_request(thread_id, request, LoopCommandSource::Agent)
                .await
            {
                Ok(outcome) => outcome,
                Err(err) => loop_action_failure("unknown", thread_id, err.to_string()),
            },
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
        let outcome = if params.namespace.as_deref() == Some(MANAGE_LOOPS_TOOL_NAMESPACE)
            && params.tool == MANAGE_LOOPS_TOOL_NAME
        {
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
}

#[cfg(test)]
mod tests {
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
