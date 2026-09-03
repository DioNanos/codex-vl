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
use crate::vivling::BrainTarget;
use crate::vivling::VivlingLoopTickRequest;
use crate::vl::VlEvent;
use crate::vl::loop_runtime::LoopJobPayload;

use super::formatting::LOOP_STATUS_BLOCKED;
use super::formatting::LOOP_STATUS_BLOCKED_OWNER;
use super::formatting::LOOP_STATUS_BLOCKED_REVIEW;
use super::formatting::LOOP_STATUS_BLOCKED_SIDE;
use super::formatting::LOOP_STATUS_DELEGATED_VIVLING;
use super::formatting::LOOP_STATUS_EXPIRED;
use super::formatting::LOOP_STATUS_INVALID_RUNNER_MODEL;
use super::formatting::LOOP_STATUS_PENDING_BUSY;
use super::formatting::LOOP_STATUS_PENDING_OCCURRENCE_MISSING;
use super::formatting::LOOP_STATUS_PROGRESS;
use super::formatting::LOOP_STATUS_RUNNER_DISPATCHED;
use super::formatting::LOOP_STATUS_SKIPPED_BUSY;
use super::formatting::LOOP_STATUS_SUBMITTED;
use super::jobs::validate_runner_model;
use super::state::loop_now_ms;
use super::state::loop_state_error;
use super::state::next_run_after_tick_ms;
use super::state::next_run_at_ms;
use super::state::one_shot_expired;
use super::state::retry_tick_runtime_state;
use crate::vl::delegated_loops::VivlingReadiness;
use crate::vl::delegated_loops::owner_from_resolution;
use crate::vl::delegated_loops::resolve_effective_owner;

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
    next_run_ms: Option<i64>,
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
            next_run_ms,
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

async fn claim_occurrence_for_dispatch(
    state_runtime: &codex_state::StateRuntime,
    app: &mut App,
    thread_id: ThreadId,
    job: &codex_state::ThreadLoopJob,
    retry_state: super::state::RetryTickRuntimeState,
) -> color_eyre::Result<bool> {
    let Some(occurrence_ms) = job.next_run_ms else {
        state_runtime
            .update_thread_loop_job_runtime(
                thread_id,
                &job.id,
                codex_state::ThreadLoopJobRuntimeUpdate {
                    next_run_ms: None,
                    last_run_ms: job.last_run_ms,
                    last_status: Some(LOOP_STATUS_PENDING_OCCURRENCE_MISSING.to_string()),
                    last_error: Some(
                        "tick has no persisted occurrence key; dispatch refused".to_string(),
                    ),
                    pending_tick: false,
                    updated_at_ms: loop_now_ms(),
                },
            )
            .await
            .map_err(loop_state_error)?;
        app.record_vivling_loop_job(
            "audit_rejected",
            &job.label,
            Some(job),
            super::types::LoopCommandSource::Agent,
        );
        return Ok(false);
    };

    let claimed = state_runtime
        .claim_loop_occurrence(&job.id, occurrence_ms, loop_now_ms())
        .await
        .map_err(loop_state_error)?;
    if claimed {
        return Ok(true);
    }

    state_runtime
        .update_thread_loop_job_runtime(
            thread_id,
            &job.id,
            codex_state::ThreadLoopJobRuntimeUpdate {
                next_run_ms: retry_state.next_run_ms,
                last_run_ms: job.last_run_ms,
                last_status: Some(LOOP_STATUS_SKIPPED_BUSY.to_string()),
                last_error: None,
                pending_tick: retry_state.pending_tick,
                updated_at_ms: loop_now_ms(),
            },
        )
        .await
        .map_err(loop_state_error)?;
    Ok(false)
}

pub(super) async fn process_submission(
    app: &mut App,
    thread_id: ThreadId,
    job: codex_state::ThreadLoopJob,
) -> color_eyre::Result<()> {
    let state_runtime = app.loop_state_runtime().await?;
    // at-most-once dispatch: every path into submission (the timer and
    // the pending/restore path) claims the occurrence before owner resolution,
    // validation, or runner dispatch. Keeping the CAS here prevents a caller
    // that bypasses `handle_tick` from emitting a duplicate child event.
    let descriptor = state_runtime
        .get_loop_descriptor(&job.id)
        .await
        .map_err(loop_state_error)?;
    let is_one_shot = descriptor
        .as_ref()
        .is_some_and(|descriptor| descriptor.schedule_kind == "one_shot");
    let started_ms = loop_now_ms();
    let schedule_kind = descriptor
        .as_ref()
        .map(|descriptor| descriptor.schedule_kind.as_str())
        .unwrap_or("interval");
    let occurrence_ms = job.next_run_ms;
    let retry_state = retry_tick_runtime_state(schedule_kind, occurrence_ms);
    if job.pending_tick && occurrence_ms.is_none() {
        state_runtime
            .update_thread_loop_job_runtime(
                thread_id,
                &job.id,
                codex_state::ThreadLoopJobRuntimeUpdate {
                    next_run_ms: None,
                    last_run_ms: job.last_run_ms,
                    last_status: Some(LOOP_STATUS_PENDING_OCCURRENCE_MISSING.to_string()),
                    last_error: Some(
                        "pending tick has no persisted occurrence key; dispatch refused"
                            .to_string(),
                    ),
                    pending_tick: false,
                    updated_at_ms: loop_now_ms(),
                },
            )
            .await
            .map_err(loop_state_error)?;
        app.record_vivling_loop_job(
            "audit_rejected",
            &job.label,
            Some(&job),
            super::types::LoopCommandSource::Agent,
        );
        return Ok(());
    }
    // A one-shot has exactly one persisted occurrence. Once that occurrence
    // has completed (successfully or with a terminal failure), its runtime
    // row is disarmed and must not be turned into a fresh dispatch merely
    // because a later refresh calls this common submission path again. A
    // missing key on an otherwise unprocessed one-shot is also fail-closed.
    if is_one_shot && occurrence_ms.is_none() {
        if job.last_status.is_none() {
            state_runtime
                .update_thread_loop_job_runtime(
                    thread_id,
                    &job.id,
                    codex_state::ThreadLoopJobRuntimeUpdate {
                        next_run_ms: None,
                        last_run_ms: job.last_run_ms,
                        last_status: Some(LOOP_STATUS_PENDING_OCCURRENCE_MISSING.to_string()),
                        last_error: Some(
                            "one-shot has no persisted occurrence key; dispatch refused"
                                .to_string(),
                        ),
                        pending_tick: false,
                        updated_at_ms: loop_now_ms(),
                    },
                )
                .await
                .map_err(loop_state_error)?;
            app.record_vivling_loop_job(
                "audit_rejected",
                &job.label,
                Some(&job),
                super::types::LoopCommandSource::Agent,
            );
        }
        return Ok(());
    }
    if is_one_shot
        && let Some(scheduled_at_ms) = occurrence_ms
        && descriptor.as_ref().is_some_and(|descriptor| {
            one_shot_expired(
                &super::state::SchedulePlan {
                    schedule_kind: &descriptor.schedule_kind,
                    interval_seconds: job.interval_seconds,
                    schedule_at: descriptor.schedule_at.as_deref(),
                    tz: descriptor.tz.as_deref(),
                    one_shot_at_ms: descriptor.one_shot_at_ms,
                },
                loop_now_ms(),
            )
        })
        && !state_runtime
            .has_loop_occurrence(&job.id, scheduled_at_ms)
            .await
            .map_err(loop_state_error)?
    {
        state_runtime
            .update_thread_loop_job_runtime(
                thread_id,
                &job.id,
                codex_state::ThreadLoopJobRuntimeUpdate {
                    next_run_ms: None,
                    last_run_ms: job.last_run_ms,
                    last_status: Some(LOOP_STATUS_EXPIRED.to_string()),
                    last_error: None,
                    pending_tick: false,
                    updated_at_ms: loop_now_ms(),
                },
            )
            .await
            .map_err(loop_state_error)?;
        // An expired one-shot is a terminal outcome, not silence: the summary
        // and its pending are persisted like every other finished tick. No
        // owner resolution ran for a tick that never dispatched: the manager
        // is the nominal main.
        if let Ok(Some(summary)) = super::notify::persist_summary_with_outcome(
            app,
            &state_runtime,
            &job,
            descriptor.as_ref(),
            None,
            super::summary::LoopManager::Main,
            "expired".to_string(),
            Some(scheduled_at_ms),
            super::summary::LoopTickOutcome::OneShotExpired,
            started_ms,
        )
        .await
        {
            app.app_event_tx
                .send_vl(crate::vl::VlEvent::LoopTickSummary { summary });
        }
        return Ok(());
    }
    let thread_owner = state_runtime
        .get_thread_loop_owner(thread_id)
        .await
        .map_err(loop_state_error)?;
    let delegation = state_runtime
        .get_loop_delegation(thread_id, &job.id)
        .await
        .map_err(loop_state_error)?;
    let delegation =
        super::vivling_delegation::refresh_management_state(app, &state_runtime, &job, delegation)
            .await?;
    let delegated_vivling_id = delegation
        .as_ref()
        .map(|delegation| delegation.vivling_id.as_str())
        .or_else(|| thread_owner.owner_vivling_id.as_deref());
    let readiness = delegated_vivling_id
        .map(|vivling_id| {
            app.chat_widget
                .vivling_loop_owner_readiness(&app.config, vivling_id)
        })
        .unwrap_or(VivlingReadiness::NotRequested);
    let resolution = resolve_effective_owner(delegation.as_ref(), &thread_owner, readiness);
    tracing::debug!(
        target: "codex_vl::loop_delegation",
        label = %job.label,
        source = resolution.source.label(),
        requested = ?resolution.requested,
        readiness = resolution.readiness.label(),
        effective = ?resolution.effective,
        reason = resolution.reason,
        "resolved loop owner"
    );
    let owner = owner_from_resolution(&resolution, thread_id, loop_now_ms());
    // the summary manager comes from the tick's own owner
    // resolution (never re-derived after the fact).
    let manager = match &resolution.effective {
        crate::vl::delegated_loops::RequestedLoopOwner::Main => super::notify::LoopManager::Main,
        crate::vl::delegated_loops::RequestedLoopOwner::Vivling { .. } => {
            super::notify::LoopManager::Vivling
        }
    };
    let manager_reason = resolution.reason.to_string();
    let now = loop_now_ms();
    let payload = LoopJobPayload::from_storage_text(&job.prompt_text);
    // the schedule descriptor is read once and drives every reschedule
    // of this tick (internal payload, runner dispatch, and the terminal
    // update in handle_loop_tick_finished).
    let scheduled_next_run_ms = descriptor
        .as_ref()
        .map(|descriptor| {
            next_run_at_ms(
                &super::state::SchedulePlan {
                    schedule_kind: &descriptor.schedule_kind,
                    interval_seconds: job.interval_seconds,
                    schedule_at: descriptor.schedule_at.as_deref(),
                    tz: descriptor.tz.as_deref(),
                    one_shot_at_ms: descriptor.one_shot_at_ms,
                },
                now,
            )
        })
        .unwrap_or_else(|| Some(now.saturating_add(job.interval_seconds.saturating_mul(1000))));
    let rescheduled_next_run_ms = descriptor
        .as_ref()
        .map(|descriptor| {
            next_run_after_tick_ms(
                &super::state::SchedulePlan {
                    schedule_kind: &descriptor.schedule_kind,
                    interval_seconds: job.interval_seconds,
                    schedule_at: descriptor.schedule_at.as_deref(),
                    tz: descriptor.tz.as_deref(),
                    one_shot_at_ms: descriptor.one_shot_at_ms,
                },
                now,
            )
        })
        .unwrap_or_else(|| Some(now.saturating_add(job.interval_seconds.saturating_mul(1000))));
    if let Some(internal_outcome) =
        execute_internal_payload(&job, &payload, now, scheduled_next_run_ms)
    {
        if !claim_occurrence_for_dispatch(&state_runtime, app, thread_id, &job, retry_state).await?
        {
            return Ok(());
        }
        let (next_run_ms, pending_tick) = if internal_outcome.pending_tick {
            (retry_state.next_run_ms, retry_state.pending_tick)
        } else {
            (rescheduled_next_run_ms, false)
        };
        state_runtime
            .update_thread_loop_job_runtime(
                thread_id,
                &job.id,
                codex_state::ThreadLoopJobRuntimeUpdate {
                    next_run_ms,
                    last_run_ms: Some(now),
                    last_status: Some(internal_outcome.status.to_string()),
                    last_error: internal_outcome.last_error.clone(),
                    pending_tick,
                    updated_at_ms: now,
                },
            )
            .await
            .map_err(loop_state_error)?;
        app.chat_widget.add_info_message(
            format!("Loop `{}`: {}", job.label, internal_outcome.message),
            /*hint*/ None,
        );
        let runtime_state = if pending_tick {
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
        // the internal-payload tick is finished in-process.
        super::notify::record_sync_tick_summary(
            app,
            &state_runtime,
            thread_id,
            &job,
            started_ms,
            manager,
            manager_reason.clone(),
        )
        .await;
        return Ok(());
    }

    let runner_kind = descriptor
        .as_ref()
        .map(|descriptor| descriptor.runner_kind)
        .unwrap_or(codex_state::LoopRunnerKind::Main);
    let runner_model = descriptor
        .as_ref()
        .and_then(|descriptor| descriptor.runner_model.clone());

    if let Err(err) = validate_runner_model(app, runner_kind, runner_model.as_deref()) {
        state_runtime
            .update_thread_loop_job_runtime(
                thread_id,
                &job.id,
                codex_state::ThreadLoopJobRuntimeUpdate {
                    next_run_ms: retry_state.next_run_ms,
                    last_run_ms: job.last_run_ms,
                    last_status: Some(LOOP_STATUS_INVALID_RUNNER_MODEL.to_string()),
                    last_error: Some(err.to_string()),
                    pending_tick: retry_state.pending_tick,
                    updated_at_ms: now,
                },
            )
            .await
            .map_err(loop_state_error)?;
        // synchronous tick boundary: persist-before-emit summary
        // with the RESOLVED manager. Errors never fail the tick.
        super::notify::record_sync_tick_summary(
            app,
            &state_runtime,
            thread_id,
            &job,
            started_ms,
            manager,
            manager_reason.clone(),
        )
        .await;
        return Ok(());
    }

    if runner_kind == codex_state::LoopRunnerKind::ChildAgent {
        let began = state_runtime
            .try_begin_loop_tick(&job.id, now)
            .await
            .map_err(loop_state_error)?;
        if !began {
            state_runtime
                .update_thread_loop_job_runtime(
                    thread_id,
                    &job.id,
                    codex_state::ThreadLoopJobRuntimeUpdate {
                        next_run_ms: retry_state.next_run_ms,
                        last_run_ms: job.last_run_ms,
                        last_status: Some(LOOP_STATUS_SKIPPED_BUSY.to_string()),
                        last_error: None,
                        pending_tick: retry_state.pending_tick,
                        updated_at_ms: now,
                    },
                )
                .await
                .map_err(loop_state_error)?;
            // synchronous tick boundary: persist-before-emit summary
            // with the RESOLVED manager. Errors never fail the tick.
            super::notify::record_sync_tick_summary(
                app,
                &state_runtime,
                thread_id,
                &job,
                started_ms,
                manager,
                manager_reason.clone(),
            )
            .await;
            return Ok(());
        }

        let mut request = if owner.owner_kind == codex_state::THREAD_LOOP_OWNER_KIND_VIVLING {
            let Some(owner_vivling_id) = owner.owner_vivling_id.clone() else {
                let _ = state_runtime.finish_loop_tick(&job.id, now).await;
                state_runtime
                    .update_thread_loop_job_runtime(
                        thread_id,
                        &job.id,
                        codex_state::ThreadLoopJobRuntimeUpdate {
                            next_run_ms: retry_state.next_run_ms,
                            last_run_ms: job.last_run_ms,
                            last_status: Some(LOOP_STATUS_BLOCKED_OWNER.to_string()),
                            last_error: Some("Vivling loop owner is missing.".to_string()),
                            pending_tick: retry_state.pending_tick,
                            updated_at_ms: now,
                        },
                    )
                    .await
                    .map_err(loop_state_error)?;
                // synchronous tick boundary: persist-before-emit summary
                // with the RESOLVED manager. Errors never fail the tick.
                super::notify::record_sync_tick_summary(
                    app,
                    &state_runtime,
                    thread_id,
                    &job,
                    started_ms,
                    manager,
                    manager_reason.clone(),
                )
                .await;
                return Ok(());
            };
            match app
                .chat_widget
                .prepare_vivling_loop_tick(&app.config, &owner_vivling_id, &job)
            {
                Ok(request) => request,
                Err(err) => {
                    let _ = state_runtime.finish_loop_tick(&job.id, now).await;
                    state_runtime
                        .update_thread_loop_job_runtime(
                            thread_id,
                            &job.id,
                            codex_state::ThreadLoopJobRuntimeUpdate {
                                next_run_ms: retry_state.next_run_ms,
                                last_run_ms: job.last_run_ms,
                                last_status: Some(LOOP_STATUS_BLOCKED_OWNER.to_string()),
                                last_error: Some(err),
                                pending_tick: retry_state.pending_tick,
                                updated_at_ms: now,
                            },
                        )
                        .await
                        .map_err(loop_state_error)?;
                    // synchronous tick boundary: persist-before-emit summary
                    // with the RESOLVED manager. Errors never fail the tick.
                    super::notify::record_sync_tick_summary(
                        app,
                        &state_runtime,
                        thread_id,
                        &job,
                        started_ms,
                        manager,
                        manager_reason.clone(),
                    )
                    .await;
                    return Ok(());
                }
            }
        } else {
            VivlingLoopTickRequest {
                vivling_id: "main".to_string(),
                vivling_name: "main".to_string(),
                brain_target: BrainTarget::SessionDefault,
                loop_label: job.label.clone(),
                loop_goal: job
                    .goal_text
                    .clone()
                    .unwrap_or_else(|| payload.display_text()),
                prompt_text: payload.display_text(),
                auto_remove_on_completion: job.auto_remove_on_completion,
                prompt_context: format!(
                    "This loop tick runs as a separate model call (no tools). Return only the structured loop result; do not ask for interactive approval.\nLoop goal: {}\nLoop prompt: {}",
                    job.goal_text.as_deref().unwrap_or("none"),
                    payload.display_text()
                ),
            }
        };
        if !claim_occurrence_for_dispatch(&state_runtime, app, thread_id, &job, retry_state).await?
        {
            let _ = state_runtime.finish_loop_tick(&job.id, now).await;
            return Ok(());
        }

        // A persisted child runner is authoritative for this tick. Do not let
        // a Vivling profile silently replace its validated runner model.
        request.brain_target = BrainTarget::SessionDefault;
        if let Some(delegation) = delegation.as_ref() {
            let parsed = codex_state::parse_recent_results(&delegation.recent_results_json);
            let metrics = codex_state::LoopMetrics::from_entries(&parsed.entries);
            let allowed_actions = match delegation.strategy {
                codex_state::LoopDelegationStrategy::Manage => {
                    "Allowed managed actions at this tick: enrich prompt; disable/remove only according to auto_remove_on_completion; increase interval only when the Manage churn gate is green."
                }
                codex_state::LoopDelegationStrategy::Suggest => {
                    "Managed actions are suggestions only at this tick; do not request or perform mutations without explicit user confirmation."
                }
                codex_state::LoopDelegationStrategy::Observe => {
                    "Managed actions are disabled at this tick; report observations only and do not request mutations."
                }
            };
            request.prompt_context.push_str(&format!(
                "\n\n[managed loop context]\nticks_managed={} clean={} noisy={} blocked={}\n{} Never create, split, change owner, file/git, rearm, at, or one_shot.\n",
                delegation.ticks_managed,
                metrics.clean_submissions,
                metrics.noisy_churn,
                metrics.blocked_runs,
                allowed_actions,
            ));
        }

        tracing::info!(
            target: "codex_vl::loop_delegation",
            provider = %app.config.model_provider_id,
            model = %runner_model.as_deref().unwrap_or("<missing>"),
            label = %job.label,
            "dispatching loop tick as a separate model call (no tools)"
        );
        state_runtime
            .update_thread_loop_job_runtime(
                thread_id,
                &job.id,
                codex_state::ThreadLoopJobRuntimeUpdate {
                    next_run_ms: None,
                    last_run_ms: Some(now),
                    last_status: Some(LOOP_STATUS_RUNNER_DISPATCHED.to_string()),
                    last_error: None,
                    pending_tick: false,
                    updated_at_ms: now,
                },
            )
            .await
            .map_err(loop_state_error)?;
        app.register_managed_loop_scope(thread_id, &job.id, &job.label);
        app.app_event_tx.send_vl(VlEvent::RunVivlingLoopTick {
            thread_id,
            job_id: job.id.clone(),
            occurrence_ms,
            started_ms,
            request,
            runner_model,
            resolution: resolution.clone(),
        });
        app.record_vivling_loop_runtime(
            &job.label,
            Some("child_agent"),
            Some(LOOP_STATUS_RUNNER_DISPATCHED),
            job.goal_text.as_deref().or(Some(job.prompt_text.as_str())),
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
                        next_run_ms: retry_state.next_run_ms,
                        last_run_ms: job.last_run_ms,
                        last_status: Some(LOOP_STATUS_BLOCKED_OWNER.to_string()),
                        last_error: Some("Vivling loop owner is missing.".to_string()),
                        pending_tick: retry_state.pending_tick,
                        updated_at_ms: now,
                    },
                )
                .await
                .map_err(loop_state_error)?;
            // synchronous tick boundary: persist-before-emit summary
            // with the RESOLVED manager. Errors never fail the tick.
            super::notify::record_sync_tick_summary(
                app,
                &state_runtime,
                thread_id,
                &job,
                started_ms,
                manager,
                manager_reason.clone(),
            )
            .await;
            return Ok(());
        };
        match app
            .chat_widget
            .prepare_vivling_loop_tick(&app.config, &owner_vivling_id, &job)
        {
            Ok(mut request) => {
                if !claim_occurrence_for_dispatch(&state_runtime, app, thread_id, &job, retry_state)
                    .await?
                {
                    return Ok(());
                }
                // Feed del worker context (volatile bus) nel prompt
                // del loop tick, così il Vivling vede l'attività worker recente.
                if let Some(summary) = app.vivling_context_bus.worker_context_summary() {
                    request
                        .prompt_context
                        .push_str("\n\n[recent worker context]\n");
                    request.prompt_context.push_str(&summary);
                }
                state_runtime
                    .update_thread_loop_job_runtime(
                        thread_id,
                        &job.id,
                        codex_state::ThreadLoopJobRuntimeUpdate {
                            next_run_ms: retry_state.next_run_ms,
                            last_run_ms: Some(now),
                            last_status: Some(LOOP_STATUS_DELEGATED_VIVLING.to_string()),
                            last_error: None,
                            pending_tick: false,
                            updated_at_ms: now,
                        },
                    )
                    .await
                    .map_err(loop_state_error)?;
                app.register_managed_loop_scope(thread_id, &job.id, &job.label);
                app.app_event_tx.send_vl(VlEvent::RunVivlingLoopTick {
                    thread_id,
                    job_id: job.id.clone(),
                    occurrence_ms,
                    started_ms,
                    request,
                    runner_model: None,
                    resolution: resolution.clone(),
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
                            pending_tick: retry_state.pending_tick,
                            updated_at_ms: now,
                        },
                    )
                    .await
                    .map_err(loop_state_error)?;
                // synchronous tick boundary: persist-before-emit summary
                // with the RESOLVED manager. Errors never fail the tick.
                super::notify::record_sync_tick_summary(
                    app,
                    &state_runtime,
                    thread_id,
                    &job,
                    started_ms,
                    manager,
                    manager_reason.clone(),
                )
                .await;
                return Ok(());
            }
        }
    }

    if !claim_occurrence_for_dispatch(&state_runtime, app, thread_id, &job, retry_state).await? {
        return Ok(());
    }
    let submission = app.chat_widget.submit_loop_prompt(&job, &owner);

    let (next_run_ms, pending_tick, last_status) = match submission {
        LoopPromptSubmissionOutcome::Submitted => (
            rescheduled_next_run_ms,
            false,
            loop_submission_status(submission).map(str::to_string),
        ),
        LoopPromptSubmissionOutcome::BlockedUserTurn
        | LoopPromptSubmissionOutcome::BlockedReviewMode
        | LoopPromptSubmissionOutcome::BlockedSideConversation => (
            retry_state.next_run_ms,
            retry_state.pending_tick,
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
    // synchronous tick boundary: persist-before-emit summary
    // with the RESOLVED manager. Errors never fail the tick.
    super::notify::record_sync_tick_summary(
        app,
        &state_runtime,
        thread_id,
        &job,
        started_ms,
        manager,
        manager_reason.clone(),
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::LOOP_STATUS_PROGRESS;
    use super::execute_internal_payload;
    use super::loop_now_ms;
    use super::process_submission;
    use crate::app::loop_controller::summary::LoopTickOutcome;
    use crate::app::tests::make_test_app_with_channels;
    use crate::app::tests::test_thread_session;
    use crate::app_event::AppEvent;
    use crate::vl::VlEvent;
    use crate::vl::loop_runtime::LoopJobPayload;
    use codex_protocol::ThreadId;
    use codex_state::LoopDescriptorUpsertParams;
    use codex_state::LoopRunnerKind;
    use codex_state::SqliteConfig;
    use codex_state::StateRuntime;
    use codex_state::ThreadLoopJobCreateParams;
    use codex_utils_absolute_path::test_support::PathExt;
    use tempfile::tempdir;

    fn collect_loop_events(
        app_events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) -> (usize, Vec<LoopTickOutcome>) {
        let mut child_ticks = 0;
        let mut summaries = Vec::new();
        while let Ok(event) = app_events.try_recv() {
            match event {
                AppEvent::Vl(VlEvent::RunVivlingLoopTick { .. }) => child_ticks += 1,
                AppEvent::Vl(VlEvent::LoopTickSummary { summary }) => {
                    summaries.push(summary.outcome)
                }
                _ => {}
            }
        }
        (child_ticks, summaries)
    }

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

        let outcome = execute_internal_payload(&job, &payload, 1_000, Some(301_000))
            .expect("internal outcome");

        assert_eq!(outcome.message, "watching");
        assert_eq!(outcome.status, LOOP_STATUS_PROGRESS);
        assert_eq!(outcome.next_run_ms, Some(301_000));
        assert!(!outcome.pending_tick);
    }

    #[tokio::test]
    async fn double_timer_persists_skipped_busy_without_second_child_event() -> anyhow::Result<()> {
        let (mut app, mut app_events, _ops) = make_test_app_with_channels().await;
        let codex_home = tempdir()?;
        let state_runtime = StateRuntime::init(
            SqliteConfig::new_for_testing(codex_home.path().abs()),
            "test-provider".to_string(),
        )
        .await?;
        app.state_db = Some(state_runtime.clone());

        let thread_id = ThreadId::new();
        app.primary_thread_id = Some(thread_id);
        app.active_thread_id = Some(thread_id);
        let cwd = app.config.cwd.to_path_buf();
        app.chat_widget
            .handle_thread_session(test_thread_session(thread_id, cwd));
        let now = 1_700_000_000_000i64;
        let job = state_runtime
            .create_or_replace_thread_loop_job(ThreadLoopJobCreateParams {
                id: "job-double-timer".to_string(),
                thread_id,
                label: "double-timer".to_string(),
                prompt_text: "child tick".to_string(),
                goal_text: Some("test busy guard".to_string()),
                interval_seconds: 60,
                enabled: true,
                run_policy: "queue_one".to_string(),
                auto_remove_on_completion: true,
                created_by: "agent".to_string(),
                next_run_ms: Some(now),
                created_at_ms: now,
                updated_at_ms: now,
            })
            .await?;
        let model = app
            .chat_widget
            .model_catalog()
            .try_list_models()?
            .first()
            .map(|preset| preset.model.clone())
            .expect("test model catalog is not empty");
        state_runtime
            .upsert_loop_descriptor(LoopDescriptorUpsertParams {
                job_id: job.id.clone(),
                runner_kind: LoopRunnerKind::ChildAgent,
                runner_model: Some(model),
                runner_reasoning_effort: None,
                tz: None,
                schedule_kind: "interval".to_string(),
                schedule_at: None,
                one_shot_at_ms: None,
                rearm_on_boot: false,
                updated_at_ms: now,
            })
            .await?;

        // The first timer has already spawned its child and owns the guard.
        assert!(state_runtime.try_begin_loop_tick(&job.id, now + 1).await?);

        // A busy timer still emits exactly one SkippedBusy summary: the
        // occurrence was evaluated, but this is not a child dispatch. The
        // loop summary is not a dispatch, so filter by event variant.
        process_submission(&mut app, thread_id, job.clone())
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        let updated = state_runtime
            .get_thread_loop_job_by_id(thread_id, &job.id)
            .await?
            .expect("job should remain present");
        assert_eq!(updated.last_status.as_deref(), Some("skipped_busy"));
        assert!(updated.pending_tick);
        assert!(
            state_runtime
                .get_loop_descriptor(&job.id)
                .await?
                .expect("descriptor should remain present")
                .in_flight
        );
        let (child_ticks, summaries) = collect_loop_events(&mut app_events);
        assert_eq!(child_ticks, 0, "busy timer emitted a second child tick");
        assert_eq!(summaries, vec![LoopTickOutcome::SkippedBusy]);
        Ok(())
    }

    // Fail-once: a one_shot tick that fails is terminal — the
    // failed outcome is persisted with the disarm and no later timer can
    // resurrect the job (no second child event).
    #[tokio::test]
    async fn one_shot_failure_disarms_without_retry() -> anyhow::Result<()> {
        let (mut app, mut app_events, _ops) = make_test_app_with_channels().await;
        let codex_home = tempdir()?;
        let state_runtime = StateRuntime::init(
            SqliteConfig::new_for_testing(codex_home.path().abs()),
            "test-provider".to_string(),
        )
        .await?;
        app.state_db = Some(state_runtime.clone());

        let thread_id = ThreadId::new();
        app.primary_thread_id = Some(thread_id);
        app.active_thread_id = Some(thread_id);
        let cwd = app.config.cwd.to_path_buf();
        app.chat_widget
            .handle_thread_session(test_thread_session(thread_id, cwd));
        let now = loop_now_ms();
        let one_shot_at_ms = now + 60_000; // future, inside the grace window
        let job = state_runtime
            .create_or_replace_thread_loop_job(ThreadLoopJobCreateParams {
                id: "job-one-shot".to_string(),
                thread_id,
                label: "one-shot".to_string(),
                prompt_text: "child tick".to_string(),
                goal_text: Some("test fail-once".to_string()),
                interval_seconds: 60,
                enabled: true,
                run_policy: "queue_one".to_string(),
                auto_remove_on_completion: true,
                created_by: "agent".to_string(),
                next_run_ms: Some(one_shot_at_ms),
                created_at_ms: now,
                updated_at_ms: now,
            })
            .await?;
        let model = app
            .chat_widget
            .model_catalog()
            .try_list_models()?
            .first()
            .map(|preset| preset.model.clone())
            .expect("test model catalog is not empty");
        state_runtime
            .upsert_loop_descriptor(LoopDescriptorUpsertParams {
                job_id: job.id.clone(),
                runner_kind: LoopRunnerKind::ChildAgent,
                runner_model: Some(model),
                runner_reasoning_effort: None,
                tz: None,
                schedule_kind: "one_shot".to_string(),
                schedule_at: None,
                one_shot_at_ms: Some(one_shot_at_ms),
                rearm_on_boot: false,
                updated_at_ms: now,
            })
            .await?;

        // The one_shot occurrence fires once: the timer dispatches the child.
        super::handle_tick(&mut app, thread_id, job.id.clone())
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        let (child_ticks, _) = collect_loop_events(&mut app_events);
        assert_eq!(
            child_ticks, 1,
            "one-shot occurrence must dispatch one child tick"
        );

        // The child fails: fail-once disarms the one_shot (no pending retry)
        // while persisting the failed outcome.
        crate::app::loop_controller::vivling_delegation::handle_loop_tick_finished(
            &mut app,
            thread_id,
            job.id.clone(),
            Some(one_shot_at_ms),
            now,
            Err("boom".to_string()),
            crate::vl::delegated_loops::EffectiveLoopOwner {
                requested: crate::vl::delegated_loops::RequestedLoopOwner::Main,
                effective: crate::vl::delegated_loops::RequestedLoopOwner::Main,
                source: crate::vl::delegated_loops::LoopOwnerSource::ThreadOwner,
                readiness: crate::vl::delegated_loops::VivlingReadiness::NotRequested,
                reason: "not_delegated",
            },
        )
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        let updated = state_runtime
            .get_thread_loop_job_by_id(thread_id, &job.id)
            .await?
            .expect("job should remain present");
        assert!(
            updated.last_status.as_deref().is_some(),
            "failed outcome persisted"
        );
        assert!(
            !updated.pending_tick,
            "failed one_shot must not be re-armed"
        );
        assert_eq!(updated.next_run_ms, None, "failed one_shot is disarmed");

        // No later timer tick resurrects the failed one-shot.
        super::handle_tick(&mut app, thread_id, job.id.clone())
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        let (child_ticks, _) = collect_loop_events(&mut app_events);
        assert_eq!(
            child_ticks, 0,
            "failed one_shot must never dispatch a second child tick"
        );
        Ok(())
    }
}
