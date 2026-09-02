//! codex-vl loop_controller: Vivling brain delegation paths.
//!
//! Bodies of the previous `App` methods owning the Vivling-side loop
//! lifecycle:
//!
//! - `handle_loop_tick_finished` consumes `VivlingLoopTickResult` /
//!   error coming back from the brain (mapped to status updates,
//!   optional follow-up `LoopCommandRequest`, persistence and chat UI
//!   feedback).
//! - `tick_action_request` is the internal helper that decides which
//!   follow-up command (`disable`, `remove`, `trigger`, `update`) the
//!   tick reply should trigger.
//! - `run_assist` / `run_loop_tick` are the tokio spawn helpers that
//!   call into `app::vivling_background::run_vivling_*_request` and
//!   surface the reply via the relevant `VlEvent::*Finished` variant.
//!
//! All bodies are migrated verbatim from `mod.rs`. The facade methods
//! on `impl App` keep byte-identical `pub(super)` signatures and now
//! delegate here.
//!
//! Nested-module path: from `app::loop_controller::vivling_delegation`
//! the spawn helpers reach `app::vivling_background` via the explicit
//! `crate::app::vivling_background::*` path; `super::vivling_background`
//! would resolve to `app::loop_controller::vivling_background` which
//! does not exist.

use codex_protocol::ThreadId;
use std::future::Future;
use std::time::Duration;

use crate::app::App;
use crate::vivling::VivlingLoopTickResult;
use crate::vl::VlEvent;
use crate::vl::events::LoopCommandRequest;

use super::formatting::LOOP_STATUS_BLOCKED;
use super::formatting::LOOP_STATUS_BLOCKED_OWNER;
use super::formatting::LOOP_STATUS_DONE;
use super::formatting::LOOP_STATUS_NEEDS_APPROVAL;
use super::formatting::LOOP_STATUS_PROGRESS;
use super::formatting::LOOP_STATUS_TIMEOUT;
use super::jobs;
use super::parsing::parse_manage_loops_interval_seconds;
use super::parsing::parse_vivling_loop_status;
use super::state::loop_now_ms;
use super::state::loop_state_error;
use super::state::next_run_at_ms;
use super::types::LoopCommandSource;

async fn persist_managed_tick_result(
    state_runtime: &codex_state::StateRuntime,
    delegation: Option<codex_state::LoopDelegation>,
    status: &str,
    ts_ms: i64,
    clean: bool,
    noisy: bool,
    blocked: bool,
) -> color_eyre::Result<Option<codex_state::LoopDelegation>> {
    let Some(delegation) = delegation else {
        return Ok(None);
    };
    if delegation.override_main {
        return Ok(Some(delegation));
    }
    let parsed = codex_state::parse_recent_results(&delegation.recent_results_json);
    if let Some(diagnostic) = parsed.diagnostic.as_deref() {
        tracing::warn!(
            target: "codex_vl::loop_management",
            job_id = %delegation.job_id,
            "recent loop result history reset: {diagnostic}"
        );
    }
    let mut entries = parsed.entries;
    entries.push(codex_state::LoopResultEntry {
        ts_ms,
        status: status.to_string(),
        clean,
        noisy,
        blocked,
    });
    let recent_results_json = codex_state::RecentLoopResults::new(entries)
        .to_json()
        .map_err(|err| color_eyre::eyre::eyre!(err))?;
    let saved = state_runtime
        .upsert_loop_delegation(codex_state::LoopDelegationUpsertParams {
            thread_id: delegation.thread_id,
            job_id: delegation.job_id,
            loop_label: delegation.loop_label,
            vivling_id: delegation.vivling_id,
            strategy: delegation.strategy,
            ticks_managed: delegation.ticks_managed.saturating_add(1),
            recent_results_json,
            last_plan_approved: delegation.last_plan_approved,
            override_main: delegation.override_main,
            cooldown_until_ms: delegation.cooldown_until_ms,
            suspend_reason: delegation.suspend_reason,
            created_at_ms: delegation.created_at_ms,
            updated_at_ms: ts_ms,
        })
        .await
        .map_err(loop_state_error)?;
    Ok(Some(saved))
}

fn delegation_params(
    delegation: &codex_state::LoopDelegation,
    strategy: codex_state::LoopDelegationStrategy,
    recent_results_json: String,
    cooldown_until_ms: Option<i64>,
    suspend_reason: Option<String>,
    updated_at_ms: i64,
) -> codex_state::LoopDelegationUpsertParams {
    codex_state::LoopDelegationUpsertParams {
        thread_id: delegation.thread_id,
        job_id: delegation.job_id.clone(),
        loop_label: delegation.loop_label.clone(),
        vivling_id: delegation.vivling_id.clone(),
        strategy,
        ticks_managed: delegation.ticks_managed,
        recent_results_json,
        last_plan_approved: delegation.last_plan_approved,
        override_main: delegation.override_main,
        cooldown_until_ms,
        suspend_reason,
        created_at_ms: delegation.created_at_ms,
        updated_at_ms,
    }
}

/// Re-evaluates the T5 state boundary after each tick and before the next
/// dispatch. Phase suspension is distinct from 3-fail demotion and both are
/// persisted in 0932-compatible fields added by 0935.
pub(super) async fn refresh_management_state(
    app: &mut App,
    state_runtime: &codex_state::StateRuntime,
    job: &codex_state::ThreadLoopJob,
    delegation: Option<codex_state::LoopDelegation>,
) -> color_eyre::Result<Option<codex_state::LoopDelegation>> {
    let Some(delegation) = delegation else {
        return Ok(None);
    };
    let parsed = codex_state::parse_recent_results(&delegation.recent_results_json);
    let metrics = codex_state::LoopMetrics::from_entries(&parsed.entries);
    let inputs = app
        .chat_widget
        .vivling_loop_management_gate_inputs(&app.config, &delegation.vivling_id)
        .ok();
    let (is_adult, brain_enabled, has_profile, bond, phase) =
        inputs.unwrap_or((false, false, false, 0, "unavailable"));
    let phase_invalid = phase == "unavailable";
    let last_three_failed = codex_state::has_consecutive_blocked(&parsed.entries, 3);
    let old_reason = delegation.suspend_reason.as_deref();
    let (strategy, cooldown_until_ms, suspend_reason, event) = if phase_invalid {
        (
            delegation.strategy,
            delegation.cooldown_until_ms,
            Some("phase".to_string()),
            (old_reason != Some("phase")).then_some("managed_suspended_phase"),
        )
    } else if old_reason == Some("phase") {
        (
            codex_state::loop_management_strategy(delegation.ticks_managed, bond, metrics),
            None,
            None,
            Some("managed_resumed_phase"),
        )
    } else if last_three_failed && old_reason != Some("3fail") {
        (
            codex_state::LoopDelegationStrategy::Suggest,
            Some(loop_now_ms().saturating_add(codex_state::LOOP_MANAGE_COOLDOWN_MS)),
            Some("3fail".to_string()),
            Some("managed_suspended_3fail"),
        )
    } else if old_reason == Some("3fail")
        && codex_state::can_resume_after_suspension(
            &parsed.entries,
            delegation.cooldown_until_ms,
            loop_now_ms(),
        )
    {
        (
            codex_state::loop_management_strategy(delegation.ticks_managed, bond, metrics),
            None,
            None,
            Some("managed_resumed_3fail"),
        )
    } else if old_reason == Some("3fail") {
        (
            codex_state::LoopDelegationStrategy::Suggest,
            delegation.cooldown_until_ms,
            delegation.suspend_reason.clone(),
            None,
        )
    } else {
        let strategy = if is_adult && brain_enabled && has_profile {
            codex_state::loop_management_strategy(delegation.ticks_managed, bond, metrics)
        } else {
            codex_state::LoopDelegationStrategy::Observe
        };
        (strategy, delegation.cooldown_until_ms, None, None)
    };
    let saved = state_runtime
        .upsert_loop_delegation(delegation_params(
            &delegation,
            strategy,
            delegation.recent_results_json.clone(),
            cooldown_until_ms,
            suspend_reason,
            loop_now_ms(),
        ))
        .await
        .map_err(loop_state_error)?;
    if let Some(event) = event {
        app.record_vivling_loop_job(event, &job.label, Some(job), LoopCommandSource::Agent);
    }
    Ok(Some(saved))
}

pub(super) async fn handle_loop_tick_finished(
    app: &mut App,
    thread_id: ThreadId,
    job_id: String,
    result: Result<VivlingLoopTickResult, String>,
) -> color_eyre::Result<()> {
    // FIX-G — the tick completion path resolves ONLY the exact scope of the
    // finishing (thread_id, job_id), fail-closed: `None` (no scope, or a
    // scope of a different job) records `audit_rejected` and never falls
    // back to `Agent`.
    let managed_source = app.resolve_managed_tick_source(thread_id, &job_id);
    // The finished event is the cleanup boundary for every child tick scope:
    // success, provider error, timeout, and cancellation all pass here.
    app.clear_managed_loop_scope(thread_id, &job_id);
    let state_runtime = app.loop_state_runtime().await?;
    let Some(job) = state_runtime
        .get_thread_loop_job_by_id(thread_id, &job_id)
        .await
        .map_err(loop_state_error)?
    else {
        return Ok(());
    };
    let descriptor = state_runtime
        .get_loop_descriptor(&job.id)
        .await
        .map_err(loop_state_error)?;
    let is_child_agent = descriptor.as_ref().is_some_and(|descriptor| {
        descriptor.runner_kind == codex_state::LoopRunnerKind::ChildAgent
    });
    // The finished event is the terminal boundary for the child runner. Clear
    // the atomic guard before processing actions so parse/action failures also
    // cannot strand the job in-flight.
    if is_child_agent {
        state_runtime
            .finish_loop_tick(&job.id, loop_now_ms())
            .await
            .map_err(loop_state_error)?;
    }
    let owner = state_runtime
        .get_thread_loop_owner(thread_id)
        .await
        .map_err(loop_state_error)?;
    let owner_vivling_id = owner.owner_vivling_id.clone();
    let now = loop_now_ms();

    match result {
        Err(err) => {
            let failure_status = if is_child_agent {
                child_tick_failure_status(&err)
            } else {
                LOOP_STATUS_BLOCKED_OWNER
            };
            if !is_child_agent
                && let Some(vivling_id) = owner_vivling_id.as_deref()
                && let Err(persist_err) = app
                    .chat_widget
                    .mark_vivling_brain_runtime_error_for(vivling_id, &err)
            {
                tracing::warn!(
                    "failed to persist Vivling loop brain error for {vivling_id}: {persist_err}"
                );
            }
            // T3 fail-once (§4-bis 3): a failed one_shot tick is terminal —
            // the `failed` outcome is persisted together with the disarm in
            // the same atomic update and the job never re-arms (no retry,
            // `pending_tick` stays false; repeating means a new occurrence).
            let is_one_shot = descriptor
                .as_ref()
                .is_some_and(|descriptor| descriptor.schedule_kind == "one_shot");
            state_runtime
                .update_thread_loop_job_runtime(
                    thread_id,
                    &job.id,
                    codex_state::ThreadLoopJobRuntimeUpdate {
                        next_run_ms: None,
                        last_run_ms: job.last_run_ms,
                        last_status: Some(failure_status.to_string()),
                        last_error: Some(err.clone()),
                        pending_tick: !is_one_shot,
                        updated_at_ms: now,
                    },
                )
                .await
                .map_err(loop_state_error)?;
            let delegation = state_runtime
                .get_loop_delegation(thread_id, &job.id)
                .await
                .map_err(loop_state_error)?;
            persist_managed_tick_result(
                &state_runtime,
                delegation,
                failure_status,
                now,
                false,
                false,
                true,
            )
            .await?;
            let delegation = state_runtime
                .get_loop_delegation(thread_id, &job.id)
                .await
                .map_err(loop_state_error)?;
            refresh_management_state(app, &state_runtime, &job, delegation).await?;
            app.chat_widget
                .add_error_message(format!("Vivling loop `{}` failed: {err}", job.label));
            app.record_vivling_loop_runtime(
                &job.label,
                if is_one_shot {
                    Some("expired")
                } else {
                    Some("pending")
                },
                Some(failure_status),
                job.goal_text.as_deref().or(Some(job.prompt_text.as_str())),
                &job.created_by,
            );
            app.refresh_loop_jobs(thread_id).await?;
            // FIX-J (3) — the failed tick is finished in-process (no remove on
            // this path, the job is alive): persist-before-emit summary.
            if let Ok(Some(summary)) = super::notify::persist_summary_with_outcome(
                app,
                &state_runtime,
                &job,
                descriptor.as_ref(),
                None,
                super::summary::LoopManager::Main,
                "tick_failed".to_string(),
                super::summary::LoopTickOutcome::Failed,
                now.saturating_sub(job.last_run_ms.unwrap_or(now)),
            )
            .await
            {
                app.app_event_tx
                    .send_vl(crate::vl::VlEvent::LoopTickSummary { summary });
            }
            return Ok(());
        }
        Ok(result) => {
            if !is_child_agent
                && let Some(vivling_id) = owner_vivling_id.as_deref()
                && let Err(persist_err) = app
                    .chat_widget
                    .mark_vivling_brain_reply_for(vivling_id, &result.message)
            {
                tracing::warn!(
                    "failed to persist Vivling loop brain reply for {vivling_id}: {persist_err}"
                );
            }

            let status = parse_vivling_loop_status(&result.status)
                .map_err(|err| color_eyre::eyre::eyre!(err))?;
            let delegation = state_runtime
                .get_loop_delegation(thread_id, &job.id)
                .await
                .map_err(loop_state_error)?;
            let noisy = result
                .loop_action
                .as_ref()
                .is_some_and(|action| action.action.eq_ignore_ascii_case("trigger"));
            persist_managed_tick_result(
                &state_runtime,
                delegation,
                status,
                now,
                matches!(status, LOOP_STATUS_PROGRESS | LOOP_STATUS_DONE),
                noisy,
                status == LOOP_STATUS_BLOCKED,
            )
            .await?;
            let delegation = state_runtime
                .get_loop_delegation(thread_id, &job.id)
                .await
                .map_err(loop_state_error)?;
            refresh_management_state(app, &state_runtime, &job, delegation).await?;
            // FIX-J (3) — persist-before-mutate: the summary (and the R3
            // pending) is durable BEFORE the completion action can remove or
            // disable the job (auto_remove_on_completion defaults to true, so
            // DONE ticks typically remove it — the summary must not die with
            // the job). Manager derived from the resolved owner: refining it
            // with the carried resolution lands with FIX-H transport.
            let summary_outcome = if status == LOOP_STATUS_BLOCKED {
                super::summary::LoopTickOutcome::Failed
            } else {
                super::summary::LoopTickOutcome::Ok
            };
            let (manager, manager_reason) = match owner.owner_kind.as_str() {
                codex_state::THREAD_LOOP_OWNER_KIND_VIVLING => (
                    super::summary::LoopManager::Vivling,
                    "delegated".to_string(),
                ),
                _ => (
                    super::summary::LoopManager::Main,
                    "not_delegated".to_string(),
                ),
            };
            if let Ok(Some(summary)) = super::notify::persist_summary_with_outcome(
                app,
                &state_runtime,
                &job,
                descriptor.as_ref(),
                delegation.as_ref(),
                manager,
                manager_reason,
                summary_outcome,
                now.saturating_sub(job.last_run_ms.unwrap_or(now)),
            )
            .await
            {
                app.app_event_tx
                    .send_vl(crate::vl::VlEvent::LoopTickSummary { summary });
            }

            let action_request = tick_action_request(thread_id, &job, status, &result)
                .map_err(|err| color_eyre::eyre::eyre!(err))?;
            let mut skipped_runtime_update = false;

            if let Some(request) = action_request {
                if matches!(
                    &request,
                    LoopCommandRequest::Remove { .. } | LoopCommandRequest::Trigger { .. }
                ) {
                    skipped_runtime_update = true;
                }
                if let Some(source) = managed_source.clone() {
                    let _ = jobs::run_command_request(app, thread_id, request, source).await?;
                } else {
                    app.record_vivling_loop_job(
                        "audit_rejected",
                        &job.label,
                        Some(&job),
                        LoopCommandSource::Agent,
                    );
                }
            }

            // FASE5 5A — gated loop suggestion (NO-AUTO channel). Emessa solo se
            // il gate (Adult + brain + bond>=50 + exposure>=20 + conf>=0.60)
            // passa; MAI applicata qui — l'utente deve `/loop apply <id>`.
            if let Some(raw) = result.suggestion.as_ref() {
                let gate = app
                    .chat_widget
                    .vivling_suggestion_gate(&app.config, raw.confidence);
                if let Some(gate) = gate
                    && gate.passes()
                {
                    let sugg = crate::vl::suggestions::VivlingLoopSuggestion {
                        id: format!("sg-{}", uuid::Uuid::new_v4().simple()),
                        // FASE5 5A safety (audit): il target e' VINCOLATO al job del
                        // tick, MAI a raw.loop_label (LLM-controlled) -> niente label
                        // injection / edit di un loop non corrispondente.
                        loop_label: job.label.clone(),
                        kind: raw.kind,
                        reasoning: raw.reasoning.clone(),
                        confidence: raw.confidence,
                        proposed_action: raw.proposed_action.clone(),
                        created_at: chrono::Utc::now(),
                    };
                    app.app_event_tx
                        .send_vl(crate::vl::VlEvent::SuggestionReady { suggestion: sugg });
                }
            }

            app.chat_widget.add_info_message(
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
                    LOOP_STATUS_PROGRESS => {
                        // T3 — reschedule from the schedule descriptor: interval
                        // rolls forward, `at` picks the next wall-clock
                        // occurrence in the persisted tz, and a fired one_shot
                        // is terminal (next_run_ms stays None: disarmed).
                        let next_run_ms = match descriptor.as_ref() {
                            Some(descriptor) if descriptor.schedule_kind == "at" => next_run_at_ms(
                                &super::state::SchedulePlan {
                                    schedule_kind: "at",
                                    interval_seconds: updated_job.interval_seconds,
                                    schedule_at: descriptor.schedule_at.as_deref(),
                                    tz: descriptor.tz.as_deref(),
                                    one_shot_at_ms: descriptor.one_shot_at_ms,
                                },
                                now,
                            ),
                            _ => {
                                Some(now.saturating_add(
                                    updated_job.interval_seconds.saturating_mul(1000),
                                ))
                            }
                        };
                        (next_run_ms, false, None)
                    }
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
                app.record_vivling_loop_runtime(
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

            app.refresh_loop_jobs(thread_id).await?;
        }
    }

    Ok(())
}

fn child_tick_failure_status(error: &str) -> &'static str {
    let error = error.to_ascii_lowercase();
    if error.contains("approval") || error.contains("permission") || error.contains("interactive") {
        LOOP_STATUS_NEEDS_APPROVAL
    } else if error.contains("timed out") || error.contains("timeout") {
        LOOP_STATUS_TIMEOUT
    } else {
        LOOP_STATUS_BLOCKED_OWNER
    }
}

fn tick_action_request(
    _thread_id: ThreadId,
    job: &codex_state::ThreadLoopJob,
    status: &str,
    result: &VivlingLoopTickResult,
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
            let action = result
                .loop_action
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Vivling loop update action payload is missing"))?;
            let interval_seconds = match action.interval.as_deref() {
                Some(interval) => Some(parse_manage_loops_interval_seconds(interval).ok_or_else(
                    || anyhow::anyhow!("Vivling loop tick returned invalid interval `{interval}`"),
                )?),
                None => None,
            };
            let prompt_text = action
                .prompt
                .as_ref()
                .map(|prompt| prompt.trim().to_string())
                .filter(|prompt| !prompt.is_empty());
            LoopCommandRequest::Update {
                label: job.label.clone(),
                interval_seconds,
                prompt_text,
                goal_text: None,
                auto_remove_on_completion: None,
                enabled: None,
                runner_kind: None,
                runner_model: None,
                schedule_kind: None,
                schedule_at: None,
                one_shot_at_ms: None,
                tz: None,
                rearm_on_boot: None,
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

pub(super) fn run_assist(
    app: &mut App,
    thread_id: ThreadId,
    request: crate::vivling::VivlingAssistRequest,
) {
    let app_event_tx = app.app_event_tx.clone();
    let config = crate::app::vivling_background::config_with_session_model(
        &app.config,
        app.chat_widget.effective_collaboration_mode().model(),
    );
    let session_telemetry = app.session_telemetry.clone();
    tokio::spawn(async move {
        let vivling_id = request.vivling_id.clone();
        let kind = request.kind;
        let task = request.task.clone();
        let result = crate::app::vivling_background::run_vivling_assist_request(
            config,
            session_telemetry,
            request,
        )
        .await;
        app_event_tx.send_vl(VlEvent::VivlingAssistFinished {
            thread_id,
            vivling_id,
            kind,
            task,
            result,
        });
    });
}

pub(super) fn run_loop_tick(
    app: &mut App,
    thread_id: ThreadId,
    job_id: String,
    request: crate::vivling::VivlingLoopTickRequest,
    runner_model: Option<String>,
) {
    let app_event_tx = app.app_event_tx.clone();
    let model = runner_model.unwrap_or_else(|| {
        app.chat_widget
            .effective_collaboration_mode()
            .model()
            .to_string()
    });
    let config = crate::app::vivling_background::config_with_session_model(&app.config, &model);
    let session_telemetry = app.session_telemetry.clone();
    tokio::spawn(async move {
        const MAX_LOOP_TICK_DURATION: Duration = Duration::from_secs(300);
        let result = run_loop_tick_with_timeout(
            crate::app::vivling_background::run_vivling_loop_tick_request(
                config,
                session_telemetry,
                request,
            ),
            MAX_LOOP_TICK_DURATION,
        )
        .await;
        app_event_tx.send_vl(VlEvent::VivlingLoopTickFinished {
            thread_id,
            job_id,
            result,
        });
    });
}

async fn run_loop_tick_with_timeout<F, T>(future: F, timeout: Duration) -> Result<T, String>
where
    F: Future<Output = Result<T, String>>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => result,
        Err(_) => Err(format!(
            "loop tick timed out after {} seconds",
            timeout.as_secs()
        )),
    }
}

#[cfg(test)]
mod runner_tests {
    use super::child_tick_failure_status;
    use super::run_loop_tick_with_timeout;
    use crate::app::loop_controller::formatting::LOOP_STATUS_BLOCKED_OWNER;
    use crate::app::loop_controller::formatting::LOOP_STATUS_NEEDS_APPROVAL;
    use crate::app::loop_controller::formatting::LOOP_STATUS_TIMEOUT;
    use std::future::pending;
    use std::time::Duration;

    #[tokio::test]
    async fn timeout_cancels_the_child_future_and_returns_timeout_error() {
        let result =
            run_loop_tick_with_timeout(pending::<Result<(), String>>(), Duration::from_millis(1))
                .await;
        assert_eq!(
            result.expect_err("pending child must time out"),
            "loop tick timed out after 0 seconds"
        );
    }

    #[test]
    fn child_failures_surface_approval_and_timeout_boundaries() {
        assert_eq!(
            child_tick_failure_status("interactive approval is required"),
            LOOP_STATUS_NEEDS_APPROVAL
        );
        assert_eq!(
            child_tick_failure_status("loop tick timed out after 300 seconds"),
            LOOP_STATUS_TIMEOUT
        );
        assert_eq!(
            child_tick_failure_status("provider request failed"),
            LOOP_STATUS_BLOCKED_OWNER
        );
    }
}

/// Memory V2 Step 12.B.D.2 — spawn the async Expression LLM runner
/// and forward the reply via `VlEvent::VivlingExpressionFinished`.
/// `request.vivling_id` is cloned out before move so the Finished
/// event can address the right Vivling even when the model returns
/// an error.
pub(super) fn run_expression(app: &mut App, request: crate::vivling::VivlingExpressionRequest) {
    // codex-vl Step 12.C — gate singolo: un solo dispatch di espressione in
    // volo. Se uno è già in corso, skip best-effort (nessun finished verrà
    // emesso, quindi nessun clear pendente: begin e clear restano 1:1).
    if !app
        .chat_widget
        .try_begin_vivling_expression(crate::vivling::ExpressionKind::Crt)
    {
        return;
    }
    let app_event_tx = app.app_event_tx.clone();
    let config = crate::app::vivling_background::config_with_session_model(
        &app.config,
        app.chat_widget.effective_collaboration_mode().model(),
    );
    let session_telemetry = app.session_telemetry.clone();
    tokio::spawn(async move {
        let vivling_id = request.vivling_id.clone();
        let result = crate::app::vivling_background::run_vivling_expression_request(
            config,
            session_telemetry,
            request,
        )
        .await;
        app_event_tx.send_vl(VlEvent::VivlingExpressionFinished { vivling_id, result });
    });
}
