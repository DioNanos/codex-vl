//! codex-vl loop_controller: job CRUD dispatcher.
//!
//! Body of the previous `App::run_loop_command_request` helper:
//! handles every `LoopCommandRequest` variant
//! (Add/Update/List/Show/Enable/Disable/Remove/Trigger + Owner show/set).
//! Stays as a free fn so the facade in `mod.rs` keeps the existing
//! `App::apply_loop_command_request` signature byte-identical.
//!
//! Child of `app::loop_controller`, so it has visibility into the
//! private `App::record_vivling_loop_job` helper kept in `mod.rs` and
//! the `pub(super)` helpers in sibling sub-modules.

use codex_protocol::ThreadId;
use uuid::Uuid;

use crate::app::App;
use crate::vl::events::LoopCommandRequest;

use super::formatting::LOOP_STATUS_DISABLED;
use super::formatting::LOOP_STATUS_REMOVED;
use super::formatting::canonical_last_status;
use super::formatting::format_loop_interval;
use super::formatting::format_loop_job_details;
use super::formatting::format_loop_job_line;
use super::formatting::loop_action_failure;
use super::formatting::loop_action_success;
use super::formatting::loop_job_json;
use super::formatting::summarize_loop_goal;
use super::formatting::thread_loop_owner_summary;
use super::state::loop_now_ms;
use super::state::loop_state_error;
use super::types::LoopActionOutcome;
use super::types::LoopCommandSource;

pub(super) async fn run_command_request(
    app: &mut App,
    thread_id: ThreadId,
    request: LoopCommandRequest,
    source: LoopCommandSource,
) -> color_eyre::Result<LoopActionOutcome> {
    if app.primary_thread_id != Some(thread_id) || app.active_thread_id != Some(thread_id) {
        return Ok(loop_action_failure(
            "guard",
            thread_id,
            "Loop commands are only available on the active primary thread.".to_string(),
        ));
    }

    let state_runtime = app.loop_state_runtime().await?;
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
            app.record_vivling_loop_job("add", &label, Some(&job), source);
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
            app.record_vivling_loop_job("update", &label, Some(&job), source);
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
                app.record_vivling_loop_job("enable", &label, Some(&updated), source);
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
                app.record_vivling_loop_job("disable", &label, Some(&updated), source);
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
                app.record_vivling_loop_job("remove", &label, None, source);
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
            app.record_vivling_loop_job("trigger", &label, Some(&updated), source);
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
            let (vivling_id, vivling_name): (String, String) = app
                .chat_widget
                .active_vivling_loop_owner_identity(&app.config)
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

    app.refresh_loop_jobs(thread_id).await?;
    Ok(outcome)
}
