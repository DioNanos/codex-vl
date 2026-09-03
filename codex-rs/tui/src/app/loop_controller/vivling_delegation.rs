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
use super::state::next_run_after_tick_ms;
use super::state::retry_tick_runtime_state;
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
            strategy_override: delegation.strategy_override,
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
        strategy_override: delegation.strategy_override,
        override_main: delegation.override_main,
        cooldown_until_ms,
        suspend_reason,
        created_at_ms: delegation.created_at_ms,
        updated_at_ms,
    }
}

pub(super) fn managed_action_gate_is_green(
    app: &mut App,
    delegation: &codex_state::LoopDelegation,
) -> bool {
    strategy_allows_automatic_actions(delegation.strategy)
        && management_gate_is_green(app, delegation)
}

fn strategy_allows_automatic_actions(strategy: codex_state::LoopDelegationStrategy) -> bool {
    strategy == codex_state::LoopDelegationStrategy::Manage
}

pub(super) fn management_gate_is_green(
    app: &mut App,
    delegation: &codex_state::LoopDelegation,
) -> bool {
    app.chat_widget
        .vivling_loop_management_gate_inputs(&app.config, &delegation.vivling_id)
        .is_ok_and(|(is_adult, brain_enabled, has_profile, _bond, phase)| {
            is_adult && brain_enabled && has_profile && phase != "unavailable"
        })
}

fn has_suspend_reason(reason: Option<&str>, wanted: &str) -> bool {
    reason
        .into_iter()
        .flat_map(|value| value.split('+'))
        .any(|value| value == wanted)
}

fn add_suspend_reason(reason: Option<&str>, wanted: &str) -> String {
    let mut reasons = reason
        .into_iter()
        .flat_map(|value| value.split('+'))
        .filter(|value| !value.is_empty() && *value != wanted)
        .collect::<Vec<_>>();
    reasons.push(wanted);
    reasons.sort_unstable_by_key(|value| (*value != "3fail", *value != "phase"));
    reasons.join("+")
}

fn remove_suspend_reason(reason: Option<&str>, unwanted: &str) -> Option<String> {
    let mut reasons = reason
        .into_iter()
        .flat_map(|value| value.split('+'))
        .filter(|value| !value.is_empty() && *value != unwanted)
        .collect::<Vec<_>>();
    reasons.sort_unstable_by_key(|value| (*value != "3fail", *value != "phase"));
    (!reasons.is_empty()).then(|| reasons.join("+"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagementSuspensionDecision {
    strategy: codex_state::LoopDelegationStrategy,
    cooldown_until_ms: Option<i64>,
    suspend_reason: Option<String>,
    event: Option<&'static str>,
}

fn management_suspension_decision(
    current_strategy: codex_state::LoopDelegationStrategy,
    strategy_override: Option<codex_state::LoopDelegationStrategy>,
    derived_strategy: codex_state::LoopDelegationStrategy,
    old_reason: Option<&str>,
    old_cooldown_until_ms: Option<i64>,
    phase_invalid: bool,
    last_three_failed: bool,
    can_resume_three_fail: bool,
    now_ms: i64,
) -> ManagementSuspensionDecision {
    let had_phase = has_suspend_reason(old_reason, "phase");
    let had_three_fail = has_suspend_reason(old_reason, "3fail");
    let new_three_fail = last_three_failed && !had_three_fail;
    let reason_with_three_fail = if new_three_fail {
        Some(add_suspend_reason(old_reason, "3fail"))
    } else {
        old_reason.map(str::to_string)
    };
    let cooldown_until_ms = if new_three_fail {
        Some(now_ms.saturating_add(codex_state::LOOP_MANAGE_COOLDOWN_MS))
    } else {
        old_cooldown_until_ms
    };

    if phase_invalid {
        return ManagementSuspensionDecision {
            strategy: if has_suspend_reason(reason_with_three_fail.as_deref(), "3fail") {
                codex_state::LoopDelegationStrategy::Suggest
            } else {
                current_strategy
            },
            cooldown_until_ms,
            suspend_reason: Some(add_suspend_reason(
                reason_with_three_fail.as_deref(),
                "phase",
            )),
            event: (!had_phase).then_some("managed_suspended_phase"),
        };
    }

    if has_suspend_reason(reason_with_three_fail.as_deref(), "3fail") {
        if had_three_fail && can_resume_three_fail {
            return ManagementSuspensionDecision {
                strategy: strategy_override.unwrap_or(derived_strategy),
                cooldown_until_ms: None,
                suspend_reason: remove_suspend_reason(reason_with_three_fail.as_deref(), "3fail"),
                event: Some("managed_resumed_3fail"),
            };
        }
        return ManagementSuspensionDecision {
            strategy: codex_state::LoopDelegationStrategy::Suggest,
            cooldown_until_ms,
            suspend_reason: remove_suspend_reason(reason_with_three_fail.as_deref(), "phase"),
            event: new_three_fail.then_some("managed_suspended_3fail"),
        };
    }

    if had_phase {
        return ManagementSuspensionDecision {
            strategy: strategy_override.unwrap_or(derived_strategy),
            cooldown_until_ms: None,
            suspend_reason: None,
            event: Some("managed_resumed_phase"),
        };
    }

    ManagementSuspensionDecision {
        strategy: strategy_override.unwrap_or(derived_strategy),
        cooldown_until_ms: old_cooldown_until_ms,
        suspend_reason: None,
        event: None,
    }
}

/// Re-evaluates the managed-tick state boundary after each tick and before the next
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
    let now = loop_now_ms();
    let derived_strategy = if is_adult && brain_enabled && has_profile {
        codex_state::loop_management_strategy(delegation.ticks_managed, bond, metrics)
    } else {
        codex_state::LoopDelegationStrategy::Observe
    };
    let can_resume_three_fail = has_suspend_reason(delegation.suspend_reason.as_deref(), "3fail")
        && codex_state::can_resume_after_suspension(
            &parsed.entries,
            delegation.cooldown_until_ms,
            now,
        );
    let decision = management_suspension_decision(
        delegation.strategy,
        delegation.strategy_override,
        derived_strategy,
        delegation.suspend_reason.as_deref(),
        delegation.cooldown_until_ms,
        phase_invalid,
        last_three_failed,
        can_resume_three_fail,
        now,
    );
    let saved = state_runtime
        .upsert_loop_delegation(delegation_params(
            &delegation,
            decision.strategy,
            delegation.recent_results_json.clone(),
            decision.cooldown_until_ms,
            decision.suspend_reason,
            now,
        ))
        .await
        .map_err(loop_state_error)?;
    if let Some(event) = decision.event {
        app.record_vivling_loop_job(event, &job.label, Some(job), LoopCommandSource::Agent);
    }
    Ok(Some(saved))
}

pub(super) async fn handle_loop_tick_finished(
    app: &mut App,
    thread_id: ThreadId,
    job_id: String,
    occurrence_ms: Option<i64>,
    started_ms: i64,
    result: Result<VivlingLoopTickResult, String>,
    resolution: crate::vl::delegated_loops::EffectiveLoopOwner,
) -> color_eyre::Result<()> {
    // the tick completion path resolves ONLY the exact scope of the
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
    // The resolved attribution travels from the dispatch path: the summary
    // reports the owner decided at tick time, not a thread-owner re-derivation.
    let owner_vivling_id = match &resolution.effective {
        crate::vl::delegated_loops::RequestedLoopOwner::Vivling { vivling_id } => {
            Some(vivling_id.clone())
        }
        crate::vl::delegated_loops::RequestedLoopOwner::Main => None,
    };
    let (manager, manager_reason) = (
        match resolution.effective {
            crate::vl::delegated_loops::RequestedLoopOwner::Vivling { .. } => {
                super::summary::LoopManager::Vivling
            }
            crate::vl::delegated_loops::RequestedLoopOwner::Main => {
                super::summary::LoopManager::Main
            }
        },
        resolution.reason.to_string(),
    );
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
            // Fail-once: a failed one_shot tick is terminal —
            // the `failed` outcome is persisted together with the disarm in
            // the same atomic update and the job never re-arms (no retry,
            // `pending_tick` stays false; repeating means a new occurrence).
            let is_one_shot = descriptor
                .as_ref()
                .is_some_and(|descriptor| descriptor.schedule_kind == "one_shot");
            let schedule_kind = descriptor
                .as_ref()
                .map(|descriptor| descriptor.schedule_kind.as_str())
                .unwrap_or("interval");
            let retry_state = retry_tick_runtime_state(schedule_kind, occurrence_ms);
            state_runtime
                .update_thread_loop_job_runtime(
                    thread_id,
                    &job.id,
                    codex_state::ThreadLoopJobRuntimeUpdate {
                        next_run_ms: retry_state.next_run_ms,
                        last_run_ms: job.last_run_ms,
                        last_status: Some(failure_status.to_string()),
                        last_error: Some(err.clone()),
                        pending_tick: retry_state.pending_tick,
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
            // the failed tick is finished in-process (no remove on
            // this path, the job is alive): persist-before-emit summary.
            if let Some(job_after) = state_runtime
                .get_thread_loop_job_by_id(thread_id, &job.id)
                .await
                .map_err(loop_state_error)?
            {
                let _ = super::notify::persist_summary_with_outcome(
                    app,
                    &state_runtime,
                    &job_after,
                    descriptor.as_ref(),
                    None,
                    manager,
                    "tick_failed".to_string(),
                    occurrence_ms,
                    super::summary::LoopTickOutcome::Failed,
                    started_ms,
                )
                .await;
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

            let status = match parse_vivling_loop_status(&result.status) {
                Ok(status) => status,
                Err(parse_err) => {
                    // A malformed payload is still an outcome: the runtime row
                    // records the error and a failed summary is persisted
                    // before the handler returns in silence.
                    state_runtime
                        .update_thread_loop_job_runtime(
                            thread_id,
                            &job.id,
                            codex_state::ThreadLoopJobRuntimeUpdate {
                                next_run_ms: None,
                                last_run_ms: job.last_run_ms,
                                last_status: Some(LOOP_STATUS_BLOCKED.to_string()),
                                last_error: Some(parse_err.to_string()),
                                pending_tick: false,
                                updated_at_ms: now,
                            },
                        )
                        .await
                        .map_err(loop_state_error)?;
                    if let Ok(Some(summary)) = super::notify::persist_summary_with_outcome(
                        app,
                        &state_runtime,
                        &job,
                        descriptor.as_ref(),
                        None,
                        manager.clone(),
                        format!("malformed payload: {parse_err}"),
                        occurrence_ms,
                        super::summary::LoopTickOutcome::Failed,
                        started_ms,
                    )
                    .await
                    {
                        app.app_event_tx
                            .send_vl(crate::vl::VlEvent::LoopTickSummary { summary });
                    }
                    app.refresh_loop_jobs(thread_id).await?;
                    return Ok(());
                }
            };
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
            let delegation = state_runtime
                .get_loop_delegation(thread_id, &job.id)
                .await
                .map_err(loop_state_error)?;
            let action_request = if managed_source.is_some()
                && delegation
                    .as_ref()
                    .is_some_and(|delegation| managed_action_gate_is_green(app, delegation))
            {
                match tick_action_request(thread_id, &job, status, &result) {
                    Ok(request) => request,
                    Err(parse_err) => {
                        // A malformed action payload is still an outcome: the
                        // runtime row records the error and a failed summary
                        // is persisted before the handler returns in silence.
                        state_runtime
                            .update_thread_loop_job_runtime(
                                thread_id,
                                &job.id,
                                codex_state::ThreadLoopJobRuntimeUpdate {
                                    next_run_ms: None,
                                    last_run_ms: job.last_run_ms,
                                    last_status: Some(LOOP_STATUS_BLOCKED.to_string()),
                                    last_error: Some(parse_err.to_string()),
                                    pending_tick: false,
                                    updated_at_ms: now,
                                },
                            )
                            .await
                            .map_err(loop_state_error)?;
                        if let Ok(Some(summary)) = super::notify::persist_summary_with_outcome(
                            app,
                            &state_runtime,
                            &job,
                            descriptor.as_ref(),
                            delegation.as_ref(),
                            manager.clone(),
                            format!("malformed action: {parse_err}"),
                            occurrence_ms,
                            super::summary::LoopTickOutcome::Failed,
                            started_ms,
                        )
                        .await
                        {
                            app.app_event_tx
                                .send_vl(crate::vl::VlEvent::LoopTickSummary { summary });
                        }
                        app.refresh_loop_jobs(thread_id).await?;
                        return Ok(());
                    }
                }
            } else {
                None
            };

            // Update the runtime row before building the summary. A successful
            // recurring tick must report the post-tick next occurrence; the
            // pre-dispatch snapshot is only safe before destructive actions.
            let (next_run_ms, pending_tick, last_error) = match status {
                LOOP_STATUS_PROGRESS => {
                    // A missing descriptor has no authoritative schedule: fail closed
                    // instead of inventing an interval cadence.
                    let next_run_ms = descriptor.as_ref().and_then(|descriptor| {
                        super::state::next_run_after_tick_ms(
                            &super::state::SchedulePlan {
                                schedule_kind: &descriptor.schedule_kind,
                                interval_seconds: job.interval_seconds,
                                schedule_at: descriptor.schedule_at.as_deref(),
                                tz: descriptor.tz.as_deref(),
                                one_shot_at_ms: descriptor.one_shot_at_ms,
                            },
                            now,
                        )
                    });
                    (next_run_ms, false, None)
                }
                LOOP_STATUS_BLOCKED => (None, true, Some(result.message.clone())),
                LOOP_STATUS_DONE => (None, false, None),
                _ => unreachable!(),
            };
            state_runtime
                .update_thread_loop_job_runtime(
                    thread_id,
                    &job.id,
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
            let job_after = state_runtime
                .get_thread_loop_job_by_id(thread_id, &job.id)
                .await
                .map_err(loop_state_error)?;

            // persist-before-mutate: the summary (and the pending row
            // pending) is durable BEFORE the completion action can remove or
            // disable the job (auto_remove_on_completion defaults to true, so
            // DONE ticks typically remove it — the summary must not die with
            // the job). Manager taken from the carried resolution, exactly as
            // resolved at dispatch time.
            let summary_outcome = if status == LOOP_STATUS_BLOCKED {
                super::summary::LoopTickOutcome::Failed
            } else {
                super::summary::LoopTickOutcome::Ok
            };
            if let Some(job_after) = job_after.as_ref() {
                let _ = super::notify::persist_summary_with_outcome(
                    app,
                    &state_runtime,
                    job_after,
                    descriptor.as_ref(),
                    delegation.as_ref(),
                    manager,
                    manager_reason,
                    occurrence_ms,
                    summary_outcome,
                    started_ms,
                )
                .await;
            }

            if let Some(request) = action_request {
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
            } else if managed_source.is_some()
                && (result.loop_action.is_some() || status == LOOP_STATUS_DONE)
            {
                app.record_vivling_loop_job(
                    "audit_rejected",
                    &job.label,
                    Some(&job),
                    managed_source.clone().unwrap_or(LoopCommandSource::Agent),
                );
            }

            // Gated loop suggestion (NO-AUTO channel). Emessa solo se
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
                        // Safety: il target e' VINCOLATO al job del
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

            if let Some(updated_job) = job_after {
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
    occurrence_ms: Option<i64>,
    started_ms: i64,
    request: crate::vivling::VivlingLoopTickRequest,
    runner_model: Option<String>,
    resolution: crate::vl::delegated_loops::EffectiveLoopOwner,
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
            occurrence_ms,
            started_ms,
            result,
            resolution,
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

#[cfg(test)]
mod strategy_tests {
    use super::management_suspension_decision;
    use super::strategy_allows_automatic_actions;

    #[test]
    fn only_manage_allows_automatic_actions() {
        assert!(!strategy_allows_automatic_actions(
            codex_state::LoopDelegationStrategy::Observe
        ));
        assert!(!strategy_allows_automatic_actions(
            codex_state::LoopDelegationStrategy::Suggest
        ));
        assert!(strategy_allows_automatic_actions(
            codex_state::LoopDelegationStrategy::Manage
        ));
    }

    #[test]
    fn three_fail_reason_survives_phase_pause_until_clean_cooldown_resume() {
        let suspended = management_suspension_decision(
            codex_state::LoopDelegationStrategy::Manage,
            Some(codex_state::LoopDelegationStrategy::Manage),
            codex_state::LoopDelegationStrategy::Manage,
            Some("3fail"),
            Some(100),
            true,
            false,
            false,
            200,
        );
        assert_eq!(suspended.suspend_reason.as_deref(), Some("3fail+phase"));
        assert_eq!(
            suspended.strategy,
            codex_state::LoopDelegationStrategy::Suggest
        );

        let phase_valid_before_resume = management_suspension_decision(
            suspended.strategy,
            Some(codex_state::LoopDelegationStrategy::Manage),
            codex_state::LoopDelegationStrategy::Manage,
            suspended.suspend_reason.as_deref(),
            suspended.cooldown_until_ms,
            false,
            false,
            false,
            300,
        );
        assert_eq!(
            phase_valid_before_resume.suspend_reason.as_deref(),
            Some("3fail")
        );
        assert_eq!(
            phase_valid_before_resume.strategy,
            codex_state::LoopDelegationStrategy::Suggest
        );

        let resumed = management_suspension_decision(
            phase_valid_before_resume.strategy,
            Some(codex_state::LoopDelegationStrategy::Manage),
            codex_state::LoopDelegationStrategy::Observe,
            phase_valid_before_resume.suspend_reason.as_deref(),
            phase_valid_before_resume.cooldown_until_ms,
            false,
            false,
            true,
            400,
        );
        assert_eq!(resumed.suspend_reason, None);
        assert_eq!(resumed.cooldown_until_ms, None);
        assert_eq!(
            resumed.strategy,
            codex_state::LoopDelegationStrategy::Manage
        );
    }
}

/// Memory V2 Step 12.B.D.2 — spawn the async Expression LLM runner
/// and forward the reply via `VlEvent::VivlingExpressionFinished`.
/// `request.vivling_id` is cloned out before move so the Finished
/// event can address the right Vivling even when the model returns
/// an error.
pub(super) fn run_expression(app: &mut App, request: crate::vivling::VivlingExpressionRequest) {
    // codex-vl Step 12.C — single gate: only one expression dispatch in
    // flight at a time. If one is already running, skip best-effort (no
    // finished event will be emitted, so nothing stays pending: begin and
    // clear stay 1:1).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::make_test_app_with_channels;
    use crate::app::tests::test_thread_session;
    use codex_state::SqliteConfig;
    use codex_state::StateRuntime;
    use codex_state::ThreadLoopJobCreateParams;
    use codex_utils_absolute_path::test_support::PathExt;
    use tempfile::tempdir;

    async fn app_with_state() -> anyhow::Result<(
        App,
        std::sync::Arc<StateRuntime>,
        ThreadId,
        tempfile::TempDir,
    )> {
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
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
        Ok((app, state_runtime, thread_id, codex_home))
    }

    async fn create_interval_job(
        state_runtime: &StateRuntime,
        thread_id: ThreadId,
        label: &str,
    ) -> anyhow::Result<codex_state::ThreadLoopJob> {
        state_runtime
            .create_or_replace_thread_loop_job(ThreadLoopJobCreateParams {
                id: format!("job-{label}"),
                thread_id,
                label: label.to_string(),
                prompt_text: "tick".to_string(),
                goal_text: Some("malformed payload test".to_string()),
                interval_seconds: 60,
                enabled: true,
                run_policy: "queue_one".to_string(),
                auto_remove_on_completion: false,
                created_by: "agent".to_string(),
                next_run_ms: Some(1_700_000_000_000),
                created_at_ms: 1_700_000_000_000,
                updated_at_ms: 1_700_000_000_000,
            })
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))
    }

    // A malformed status payload is still an outcome: the runtime row is
    // updated with the error and a failed summary is persisted, instead of
    // exiting the handler in silence.
    #[tokio::test]
    async fn malformed_status_reaches_the_runtime_update_and_the_summary() -> anyhow::Result<()> {
        let (mut app, _state_runtime, _thread_id, _codex_home) = app_with_state().await?;
        let job = create_interval_job(
            &_app_state(&app),
            app.primary_thread_id.unwrap(),
            "malformed",
        )
        .await?;

        let result = crate::vivling::VivlingLoopTickResult {
            status: "banana".to_string(),
            message: "garbled reply".to_string(),
            loop_action: None,
            suggestion: None,
        };
        let thread_id = app.primary_thread_id.unwrap();
        handle_loop_tick_finished(
            &mut app,
            thread_id,
            job.id.clone(),
            /*occurrence_ms*/ Some(1_700_000_000_000),
            /*started_ms*/ 1_700_000_000_000,
            Ok(result),
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

        let updated = _app_state(&app)
            .get_thread_loop_job_by_id(app.primary_thread_id.unwrap(), &job.id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?
            .expect("job still present");
        assert_eq!(
            updated.last_status.as_deref(),
            Some("blocked"),
            "the malformed payload must land on the persisted row"
        );
        assert!(
            updated
                .last_error
                .as_deref()
                .is_some_and(|err| err.contains("unsupported status")),
            "the parse reason must reach the persisted row"
        );

        let summaries = _app_state(&app)
            .count_loop_notifications(&job.id, codex_state::LOOP_NOTIFICATION_KIND_SUMMARY)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        assert_eq!(summaries, 1, "exactly one failed summary for the tick");

        // The summary must report a real duration (finished_at_ms - started_ms),
        // not a small delta mistaken for an absolute instant: that bug reads
        // as roughly the current epoch instant, i.e. a multi-decade duration.
        let pending = _app_state(&app)
            .list_pending_loop_notifications()
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        let summary_json = &pending
            .iter()
            .find(|row| row.job_id == job.id)
            .expect("a pending row for this job")
            .summary_json;
        let parsed: serde_json::Value =
            serde_json::from_str(summary_json).expect("summary_json must parse");
        let duration_ms = parsed["duration_ms"]
            .as_i64()
            .expect("duration_ms must be present");
        let expected_duration_ms = loop_now_ms() - 1_700_000_000_000;
        assert!(
            (duration_ms - expected_duration_ms).abs() < 5_000,
            "duration_ms must reflect started_ms ({expected_duration_ms} +/- 5s), got {duration_ms}"
        );
        Ok(())
    }

    // The completion path reports the carried resolved attribution, not a
    // thread-owner re-derivation: with a per-loop delegation governing the
    // tick while the thread owner stays `main`, the summary names the
    // delegating path. Under the old thread-owner re-derivation the same
    // summary said `main (not_delegated)` — red before the transport.
    #[tokio::test]
    async fn finish_summary_reports_the_carried_resolution_not_the_thread_owner()
    -> anyhow::Result<()> {
        let (mut app, _state_runtime, _thread_id, _codex_home) = app_with_state().await?;
        let job = create_interval_job(&_app_state(&app), app.primary_thread_id.unwrap(), "carried")
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;

        let result = crate::vivling::VivlingLoopTickResult {
            status: "blocked".to_string(),
            message: "delegated tick failed".to_string(),
            loop_action: None,
            suggestion: None,
        };
        let thread_id = app.primary_thread_id.unwrap();
        handle_loop_tick_finished(
            &mut app,
            thread_id,
            job.id.clone(),
            /*occurrence_ms*/ Some(1_700_000_000_000),
            /*started_ms*/ 1_700_000_000_000,
            Ok(result),
            crate::vl::delegated_loops::EffectiveLoopOwner {
                requested: crate::vl::delegated_loops::RequestedLoopOwner::Vivling {
                    vivling_id: "vivling-1".to_string(),
                },
                effective: crate::vl::delegated_loops::RequestedLoopOwner::Vivling {
                    vivling_id: "vivling-1".to_string(),
                },
                source: crate::vl::delegated_loops::LoopOwnerSource::Delegation,
                readiness: crate::vl::delegated_loops::VivlingReadiness::Runnable,
                reason: "delegated",
            },
        )
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

        let pending = _app_state(&app)
            .list_pending_loop_notifications()
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        assert_eq!(
            pending.len(),
            1,
            "anomalous completion keeps its pending row"
        );
        assert!(
            pending[0].summary_json.contains("\"manager\":\"vivling\""),
            "the carried resolution must reach the persisted summary"
        );
        assert!(
            pending[0].summary_json.contains("delegated"),
            "the carried resolution reason must reach the persisted summary"
        );
        Ok(())
    }

    fn _app_state(app: &App) -> &std::sync::Arc<codex_state::StateRuntime> {
        app.state_db.as_ref().expect("state handle in test")
    }
}
