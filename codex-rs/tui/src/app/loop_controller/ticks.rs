//! codex-vl loop_controller: tick scheduling + submission handlers.
//!
//! Bodies of the previous `App::handle_loop_tick` and
//! `App::process_loop_submission`. Both migrate as free fns taking
//! `&mut App` so `mod.rs` keeps the `App::handle_loop_tick` facade
//! signature byte-identical and `App::process_loop_submission` can
//! disappear entirely (its only callers — `handle_loop_tick` and
//! `events::handle_reload` — switch to `ticks::process_submission`).
//!
//! Owner-kind branch (Vivling-delegated vs. main) is preserved verbatim:
//! `RunVivlingLoopTick` emission, `LOOP_STATUS_BLOCKED_OWNER` /
//! `LOOP_STATUS_DELEGATED_VIVLING` updates, `submit_loop_prompt`
//! fallback, and `record_vivling_loop_runtime` call sites are kept
//! identical to the pre-extract behaviour.

use codex_protocol::ThreadId;

use crate::app::App;
use crate::chatwidget::loop_jobs::LoopPromptSubmissionOutcome;
use crate::vl::VlEvent;
use crate::vl::loop_runtime::LoopJobPayload;

use super::formatting::LOOP_STATUS_BLOCKED;
use super::formatting::LOOP_STATUS_BLOCKED_OWNER;
use super::formatting::LOOP_STATUS_BLOCKED_REVIEW;
use super::formatting::LOOP_STATUS_BLOCKED_SIDE;
use super::formatting::LOOP_STATUS_DELEGATED_VIVLING;
use super::formatting::LOOP_STATUS_PENDING_BUSY;
use super::formatting::LOOP_STATUS_PROGRESS;
use super::formatting::LOOP_STATUS_SUBMITTED;
use super::state::loop_now_ms;
use super::state::loop_state_error;

fn loop_submission_status(outcome: LoopPromptSubmissionOutcome) -> Option<&'static str> {
    match outcome {
        LoopPromptSubmissionOutcome::Submitted => Some(LOOP_STATUS_SUBMITTED),
        LoopPromptSubmissionOutcome::BlockedMissingThread => None,
        LoopPromptSubmissionOutcome::BlockedSideConversation => Some(LOOP_STATUS_BLOCKED_SIDE),
        LoopPromptSubmissionOutcome::BlockedReviewMode => Some(LOOP_STATUS_BLOCKED_REVIEW),
        LoopPromptSubmissionOutcome::BlockedUserTurn => Some(LOOP_STATUS_PENDING_BUSY),
    }
}

struct InternalLoopTickOutcome {
    message: String,
    status: &'static str,
    next_run_ms: Option<i64>,
    pending_tick: bool,
    last_error: Option<String>,
}

fn execute_internal_payload(
    job: &codex_state::ThreadLoopJob,
    payload: &LoopJobPayload,
    now: i64,
) -> Option<InternalLoopTickOutcome> {
    let LoopJobPayload::InternalFn { fn_name, args } = payload else {
        return None;
    };
    let message = args
        .get("message")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Internal loop `{}` ran `{fn_name}`.", job.label));
    match fn_name.as_str() {
        "loop.status" | "loop.noop" => Some(InternalLoopTickOutcome {
            message,
            status: LOOP_STATUS_PROGRESS,
            next_run_ms: Some(now + (job.interval_seconds * 1000)),
            pending_tick: false,
            last_error: None,
        }),
        other => Some(InternalLoopTickOutcome {
            message: format!("Unsupported internal loop function `{other}`."),
            status: LOOP_STATUS_BLOCKED,
            next_run_ms: None,
            pending_tick: true,
            last_error: Some(format!("unsupported internal loop function `{other}`")),
        }),
    }
}

pub(super) async fn handle_tick(
    app: &mut App,
    thread_id: ThreadId,
    job_id: String,
) -> color_eyre::Result<()> {
    if app.primary_thread_id != Some(thread_id) || app.chat_widget.thread_id() != Some(thread_id) {
        return Ok(());
    }

    let state_runtime = app.loop_state_runtime().await?;
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

    process_submission(app, thread_id, job).await?;
    app.refresh_loop_jobs(thread_id).await
}

pub(super) async fn process_submission(
    app: &mut App,
    thread_id: ThreadId,
    job: codex_state::ThreadLoopJob,
) -> color_eyre::Result<()> {
    let state_runtime = app.loop_state_runtime().await?;
    let owner = state_runtime
        .get_thread_loop_owner(thread_id)
        .await
        .map_err(loop_state_error)?;
    let now = loop_now_ms();
    let payload = LoopJobPayload::from_storage_text(&job.prompt_text);
    if let Some(internal_outcome) = execute_internal_payload(&job, &payload, now) {
        state_runtime
            .update_thread_loop_job_runtime(
                thread_id,
                &job.id,
                codex_state::ThreadLoopJobRuntimeUpdate {
                    next_run_ms: internal_outcome.next_run_ms,
                    last_run_ms: Some(now),
                    last_status: Some(internal_outcome.status.to_string()),
                    last_error: internal_outcome.last_error.clone(),
                    pending_tick: internal_outcome.pending_tick,
                    updated_at_ms: now,
                },
            )
            .await
            .map_err(loop_state_error)?;
        app.chat_widget.add_info_message(
            format!("Loop `{}`: {}", job.label, internal_outcome.message),
            /*hint*/ None,
        );
        let runtime_state = if internal_outcome.pending_tick {
            Some("pending")
        } else if internal_outcome.next_run_ms.is_some() {
            Some("scheduled")
        } else {
            Some("unscheduled")
        };
        app.record_vivling_loop_runtime(
            &job.label,
            runtime_state,
            Some(internal_outcome.status),
            job.goal_text
                .as_deref()
                .or_else(|| payload.prompt_text())
                .or(Some(job.prompt_text.as_str())),
            &job.created_by,
        );
        return Ok(());
    }
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
        match app
            .chat_widget
            .prepare_vivling_loop_tick(&app.config, &owner_vivling_id, &job)
        {
            Ok(mut request) => {
                // FASE5 5A — feed del worker context (volatile bus) nel prompt
                // del loop tick, così il Vivling vede l'attività worker recente.
                if let Some(summary) = app.vivling_context_bus.worker_context_summary() {
                    request.prompt_context.push_str("\n\n[recent worker context]\n");
                    request.prompt_context.push_str(&summary);
                }
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
                app.app_event_tx.send_vl(VlEvent::RunVivlingLoopTick {
                    thread_id,
                    job_id: job.id.clone(),
                    request,
                });
                app.record_vivling_loop_runtime(
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

    let submission = app.chat_widget.submit_loop_prompt(&job, &owner);

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
    app.record_vivling_loop_runtime(
        &job.label,
        runtime_state,
        last_status_for_event.as_deref(),
        goal,
        &job.created_by,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::LOOP_STATUS_PROGRESS;
    use super::execute_internal_payload;
    use crate::vl::loop_runtime::LoopJobPayload;

    #[test]
    fn internal_status_payload_schedules_next_tick() {
        let mut job = super::super::formatting::sample_job();
        job.prompt_text = LoopJobPayload::InternalFn {
            fn_name: "loop.status".to_string(),
            args: serde_json::json!({"message": "watching"}),
        }
        .to_storage_text()
        .unwrap();
        let payload = LoopJobPayload::from_storage_text(&job.prompt_text);

        let outcome = execute_internal_payload(&job, &payload, 1_000).expect("internal outcome");

        assert_eq!(outcome.message, "watching");
        assert_eq!(outcome.status, LOOP_STATUS_PROGRESS);
        assert_eq!(outcome.next_run_ms, Some(301_000));
        assert!(!outcome.pending_tick);
    }
}
