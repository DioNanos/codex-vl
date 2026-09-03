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
use codex_protocol::openai_models::ModelPreset;
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
use super::state::next_run_at_ms;
use super::types::LoopActionOutcome;
use super::types::LoopCommandScope;
use super::types::LoopCommandSource;

fn managed_request_label(request: &LoopCommandRequest) -> Option<&str> {
    match request {
        LoopCommandRequest::Update { label, .. }
        | LoopCommandRequest::Disable { label }
        | LoopCommandRequest::Remove { label } => Some(label),
        _ => None,
    }
}

fn managed_request_is_allowed(request: &LoopCommandRequest) -> bool {
    match request {
        LoopCommandRequest::Disable { .. } | LoopCommandRequest::Remove { .. } => true,
        LoopCommandRequest::Update {
            interval_seconds,
            prompt_text,
            goal_text,
            auto_remove_on_completion,
            enabled,
            runner_kind,
            runner_model,
            schedule_kind,
            schedule_at,
            one_shot_at_ms,
            tz,
            ..
        } => {
            (interval_seconds.is_some() || prompt_text.is_some())
                && goal_text.is_none()
                && auto_remove_on_completion.is_none()
                && enabled.is_none()
                && runner_kind.is_none()
                && runner_model.is_none()
                && schedule_kind.is_none()
                && schedule_at.is_none()
                && one_shot_at_ms.is_none()
                && tz.is_none()
        }
        _ => false,
    }
}

fn managed_scope_matches(
    scope: &LoopCommandScope,
    thread_id: ThreadId,
    job_id: &str,
    label: &str,
) -> bool {
    scope.thread_id == thread_id && scope.job_id == job_id && scope.label == label
}

pub(super) fn validate_runner_model(
    app: &App,
    runner_kind: codex_state::LoopRunnerKind,
    runner_model: Option<&str>,
) -> anyhow::Result<()> {
    if runner_kind == codex_state::LoopRunnerKind::ChildAgent && runner_model.is_none() {
        return Err(anyhow::anyhow!(
            "invalid_runner_model: child_agent requires `runner_model`"
        ));
    }
    let Some(model) = runner_model else {
        return Ok(());
    };
    let available = app
        .chat_widget
        .model_catalog()
        .try_list_models()
        .map_err(|err| anyhow::anyhow!("invalid_runner_model: {err:?}"))?;
    validate_runner_model_against_catalog(
        runner_kind,
        Some(model),
        &available,
        &app.config.model_provider_id,
    )
}

fn validate_runner_model_against_catalog(
    runner_kind: codex_state::LoopRunnerKind,
    runner_model: Option<&str>,
    available: &[ModelPreset],
    provider: &str,
) -> anyhow::Result<()> {
    if runner_kind == codex_state::LoopRunnerKind::ChildAgent && runner_model.is_none() {
        return Err(anyhow::anyhow!(
            "invalid_runner_model: child_agent requires `runner_model`"
        ));
    }
    let Some(model) = runner_model else {
        return Ok(());
    };
    if available
        .iter()
        .any(|preset| preset.model == model || preset.id == model)
    {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "invalid_runner_model: provider `{provider}` does not advertise `{model}`"
        ))
    }
}

pub(super) async fn run_command_request(
    app: &mut App,
    thread_id: ThreadId,
    request: LoopCommandRequest,
    source: LoopCommandSource,
) -> color_eyre::Result<LoopActionOutcome> {
    if app.primary_thread_id != Some(thread_id) || app.active_thread_id != Some(thread_id) {
        app.record_vivling_loop_job(
            "audit_rejected",
            managed_request_label(&request).unwrap_or("unknown"),
            None,
            source.clone(),
        );
        return Ok(loop_action_failure(
            "guard",
            thread_id,
            "Loop commands are only available on the active primary thread.".to_string(),
        ));
    }

    if let LoopCommandSource::Managed(scope) = &source {
        if scope.thread_id != thread_id || !managed_request_is_allowed(&request) {
            app.record_vivling_loop_job(
                "audit_rejected",
                managed_request_label(&request).unwrap_or("unknown"),
                None,
                source.clone(),
            );
            return Ok(loop_action_failure(
                "scope",
                thread_id,
                "Managed loop action rejected by scope or allowlist.".to_string(),
            ));
        }
    }

    let state_runtime = app.loop_state_runtime().await?;
    if let LoopCommandSource::Managed(scope) = &source {
        let Some(label) = managed_request_label(&request) else {
            app.record_vivling_loop_job("audit_rejected", "<none>", None, source.clone());
            return Ok(loop_action_failure(
                "scope",
                thread_id,
                "Managed loop action has no loop label.".to_string(),
            ));
        };
        let Some(job) = state_runtime
            .get_thread_loop_job_by_label(thread_id, label)
            .await
            .map_err(loop_state_error)?
        else {
            app.record_vivling_loop_job("audit_rejected", label, None, source.clone());
            return Ok(loop_action_failure(
                "scope",
                thread_id,
                format!("Managed loop `{label}` is not in scope."),
            ));
        };
        if !managed_scope_matches(scope, thread_id, &job.id, &job.label) {
            app.record_vivling_loop_job("audit_rejected", label, Some(&job), source.clone());
            return Ok(loop_action_failure(
                "scope",
                thread_id,
                "Managed loop action names a different job.".to_string(),
            ));
        }
        if matches!(&request, LoopCommandRequest::Disable { .. }) && job.auto_remove_on_completion {
            app.record_vivling_loop_job("audit_rejected", label, Some(&job), source.clone());
            return Ok(loop_action_failure(
                "scope",
                thread_id,
                "Managed completion policy requires remove, not disable.".to_string(),
            ));
        }
        if matches!(&request, LoopCommandRequest::Remove { .. }) && !job.auto_remove_on_completion {
            app.record_vivling_loop_job("audit_rejected", label, Some(&job), source.clone());
            return Ok(loop_action_failure(
                "scope",
                thread_id,
                "Managed completion policy requires disable, not remove.".to_string(),
            ));
        }
        let Some(delegation) = state_runtime
            .get_loop_delegation(thread_id, &job.id)
            .await
            .map_err(loop_state_error)?
        else {
            app.record_vivling_loop_job("audit_rejected", label, Some(&job), source.clone());
            return Ok(loop_action_failure(
                "strategy",
                thread_id,
                "Managed loop actions require a persisted delegation.".to_string(),
            ));
        };
        if !super::vivling_delegation::managed_action_gate_is_green(app, &delegation) {
            app.record_vivling_loop_job("audit_rejected", label, Some(&job), source.clone());
            return Ok(loop_action_failure(
                "strategy",
                thread_id,
                "Managed loop actions require an active Manage strategy and green gate."
                    .to_string(),
            ));
        }
    }
    let outcome = match request {
        LoopCommandRequest::Add {
            label,
            interval_seconds,
            prompt_text,
            goal_text,
            auto_remove_on_completion,
            runner_kind,
            runner_model,
            schedule_kind,
            schedule_at,
            one_shot_at_ms,
            tz,
            rearm_on_boot,
        } => {
            validate_runner_model(&app, runner_kind, runner_model.as_deref())
                .map_err(loop_state_error)?;
            let now = loop_now_ms();
            let goal_text = goal_text
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .or_else(|| Some(prompt_text.trim().to_string()));
            let auto_remove_on_completion = auto_remove_on_completion.unwrap_or(true);
            let created_by = match &source {
                LoopCommandSource::User => "user",
                LoopCommandSource::Agent | LoopCommandSource::Managed(_) => "agent",
            }
            .to_string();
            // the first run comes from the schedule descriptor values
            // (interval | at | one_shot), computed by the pure scheduler.
            let next_run_ms = next_run_at_ms(
                &super::state::SchedulePlan {
                    schedule_kind: &schedule_kind,
                    interval_seconds,
                    schedule_at: schedule_at.as_deref(),
                    tz: tz.as_deref(),
                    one_shot_at_ms,
                },
                now,
            );
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
                    next_run_ms,
                    created_at_ms: now,
                    updated_at_ms: now,
                })
                .await
                .map_err(loop_state_error)?;
            state_runtime
                .upsert_loop_descriptor(codex_state::LoopDescriptorUpsertParams {
                    job_id: job.id.clone(),
                    runner_kind,
                    runner_model,
                    runner_reasoning_effort: None,
                    tz: tz.clone(),
                    schedule_kind: schedule_kind.clone(),
                    schedule_at: schedule_at.clone(),
                    one_shot_at_ms,
                    rearm_on_boot: rearm_on_boot.unwrap_or(false),
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
            runner_kind,
            runner_model,
            schedule_kind,
            schedule_at,
            one_shot_at_ms,
            tz,
            rearm_on_boot,
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
            let existing_descriptor = state_runtime
                .get_loop_descriptor(&existing.id)
                .await
                .map_err(loop_state_error)?;
            let runner_kind = runner_kind
                .or_else(|| {
                    existing_descriptor
                        .as_ref()
                        .map(|descriptor| descriptor.runner_kind)
                })
                .unwrap_or(codex_state::LoopRunnerKind::Main);
            let runner_model = match runner_model {
                Some(model) => model,
                None => existing_descriptor
                    .as_ref()
                    .and_then(|descriptor| descriptor.runner_model.clone()),
            };
            let descriptor = existing_descriptor;
            validate_runner_model(&app, runner_kind, runner_model.as_deref())
                .map_err(loop_state_error)?;
            if let LoopCommandSource::Managed(_scope) = &source {
                if let Some(requested_interval) = interval_seconds
                    && requested_interval <= existing.interval_seconds
                {
                    return Ok(loop_action_failure(
                        "scope",
                        thread_id,
                        "Managed interval changes must increase cadence only on a churn gate."
                            .to_string(),
                    ));
                }
                if interval_seconds.is_some() {
                    let Some(delegation) = state_runtime
                        .get_loop_delegation(thread_id, &existing.id)
                        .await
                        .map_err(loop_state_error)?
                    else {
                        return Ok(loop_action_failure(
                            "scope",
                            thread_id,
                            "Managed interval change requires a persisted delegation.".to_string(),
                        ));
                    };
                    let parsed = codex_state::parse_recent_results(&delegation.recent_results_json);
                    let metrics = codex_state::LoopMetrics::from_entries(&parsed.entries);
                    let Ok((is_adult, brain_enabled, has_profile, bond, phase)) = app
                        .chat_widget
                        .vivling_loop_management_gate_inputs(&app.config, &delegation.vivling_id)
                    else {
                        return Ok(loop_action_failure(
                            "scope",
                            thread_id,
                            "Managed interval change could not verify its gate inputs.".to_string(),
                        ));
                    };
                    let strategy = codex_state::loop_management_strategy(
                        delegation.ticks_managed,
                        bond,
                        metrics,
                    );
                    if !is_adult
                        || !brain_enabled
                        || !has_profile
                        || phase == "unavailable"
                        || strategy != codex_state::LoopDelegationStrategy::Manage
                        || metrics.noisy_churn == 0
                    {
                        return Ok(loop_action_failure(
                            "scope",
                            thread_id,
                            "Managed interval change rejected: Manage/churn gate is not green."
                                .to_string(),
                        ));
                    }
                }
            }
            let prompt_text = match (&source, prompt_text) {
                (LoopCommandSource::Managed(_), Some(enrichment)) => format!(
                    "{}\n\n[managed prompt enrichment]\n{}",
                    existing.prompt_text, enrichment
                ),
                (_, Some(prompt_text)) => prompt_text,
                (_, None) => existing.prompt_text.clone(),
            };
            let goal_text = match goal_text {
                Some(next_goal) => next_goal,
                None => existing.goal_text.clone(),
            };
            let interval_seconds = interval_seconds.unwrap_or(existing.interval_seconds);
            let enabled = enabled.unwrap_or(existing.enabled);
            let auto_remove_on_completion =
                auto_remove_on_completion.unwrap_or(existing.auto_remove_on_completion);
            // the schedule triplet is updated atomically: when
            // `schedule_kind` is provided the triplet from the command wins
            // as a whole, otherwise the persisted one stays untouched.
            let (schedule_kind, schedule_at, one_shot_at_ms, tz) = if let Some(kind) = schedule_kind
            {
                (kind, schedule_at, one_shot_at_ms, tz)
            } else {
                let persisted = descriptor
                    .as_ref()
                    .map(|descriptor| {
                        (
                            descriptor.schedule_kind.clone(),
                            descriptor.schedule_at.clone(),
                            descriptor.one_shot_at_ms,
                            descriptor.tz.clone(),
                        )
                    })
                    .unwrap_or_else(|| ("interval".to_string(), None, None, None));
                (persisted.0, persisted.1, persisted.2, persisted.3)
            };
            let next_run_ms = if enabled {
                next_run_at_ms(
                    &super::state::SchedulePlan {
                        schedule_kind: &schedule_kind,
                        interval_seconds,
                        schedule_at: schedule_at.as_deref(),
                        tz: tz.as_deref(),
                        one_shot_at_ms,
                    },
                    now,
                )
            } else {
                None
            };
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
                    next_run_ms,
                    created_at_ms: existing.created_at_ms,
                    updated_at_ms: now,
                })
                .await
                .map_err(loop_state_error)?;
            state_runtime
                .upsert_loop_descriptor(codex_state::LoopDescriptorUpsertParams {
                    job_id: job.id.clone(),
                    runner_kind,
                    runner_model,
                    runner_reasoning_effort: descriptor
                        .as_ref()
                        .and_then(|descriptor| descriptor.runner_reasoning_effort.clone()),
                    tz: tz.clone(),
                    schedule_kind: schedule_kind.clone(),
                    schedule_at: schedule_at.clone(),
                    one_shot_at_ms,
                    rearm_on_boot: rearm_on_boot
                        .or(descriptor
                            .as_ref()
                            .map(|descriptor| descriptor.rearm_on_boot))
                        .unwrap_or(false),
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
                // re-enable resumes from the schedule descriptor (`at`
                // picks the next wall-clock occurrence; a one-shot past its
                // instant stays disarmed: terminal expired).
                let descriptor = state_runtime
                    .get_loop_descriptor(&job.id)
                    .await
                    .map_err(loop_state_error)?;
                let next_run_ms = next_run_at_ms(
                    &super::state::SchedulePlan {
                        schedule_kind: descriptor
                            .as_ref()
                            .map(|descriptor| descriptor.schedule_kind.as_str())
                            .unwrap_or("interval"),
                        interval_seconds: job.interval_seconds,
                        schedule_at: descriptor
                            .as_ref()
                            .and_then(|descriptor| descriptor.schedule_at.as_deref()),
                        tz: descriptor
                            .as_ref()
                            .and_then(|descriptor| descriptor.tz.as_deref()),
                        one_shot_at_ms: descriptor
                            .as_ref()
                            .and_then(|descriptor| descriptor.one_shot_at_ms),
                    },
                    now,
                );
                state_runtime
                    .set_thread_loop_job_enabled(thread_id, &label, true, next_run_ms, now)
                    .await
                    .map_err(loop_state_error)?;
                state_runtime
                    .update_thread_loop_job_runtime(
                        thread_id,
                        &job.id,
                        codex_state::ThreadLoopJobRuntimeUpdate {
                            next_run_ms,
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
            if let Some(job) = state_runtime
                .get_thread_loop_job_by_label(thread_id, &label)
                .await
                .map_err(loop_state_error)?
            {
                state_runtime
                    .delete_thread_loop_job_with_dependents(thread_id, &label)
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
        LoopCommandRequest::Delegate { label, owner_kind } => {
            let owner_kind = owner_kind.trim().to_ascii_lowercase();
            if !matches!(owner_kind.as_str(), "main" | "vivling") {
                return Ok(loop_action_failure(
                    "delegate",
                    thread_id,
                    "`owner` must be `main` or `vivling`.".to_string(),
                ));
            }
            let Some(job) = state_runtime
                .get_thread_loop_job_by_label(thread_id, &label)
                .await
                .map_err(loop_state_error)?
            else {
                return Ok(loop_action_failure(
                    "delegate",
                    thread_id,
                    format!("Loop `{label}` not found."),
                ));
            };
            let existing = state_runtime
                .get_loop_delegation(thread_id, &job.id)
                .await
                .map_err(loop_state_error)?;
            let (vivling_id, vivling_name) = if owner_kind == "vivling" {
                app.chat_widget
                    .active_vivling_loop_owner_identity(&app.config)
                    .map_err(|err| color_eyre::eyre::eyre!(err))?
            } else if let Some(existing) = existing.as_ref() {
                (existing.vivling_id.clone(), "persisted Vivling".to_string())
            } else if let Some(vivling_id) = state_runtime
                .get_thread_loop_owner(thread_id)
                .await
                .map_err(loop_state_error)?
                .owner_vivling_id
            {
                (vivling_id, "thread owner Vivling".to_string())
            } else {
                app.chat_widget
                    .active_vivling_loop_owner_identity(&app.config)
                    .map_err(|err| color_eyre::eyre::eyre!(err))?
            };
            let now = loop_now_ms();
            let saved = state_runtime
                .upsert_loop_delegation(codex_state::LoopDelegationUpsertParams {
                    thread_id,
                    job_id: job.id.clone(),
                    loop_label: job.label.clone(),
                    vivling_id,
                    strategy: existing
                        .as_ref()
                        .map(|delegation| delegation.strategy)
                        .unwrap_or(codex_state::LoopDelegationStrategy::Observe),
                    ticks_managed: existing
                        .as_ref()
                        .map(|delegation| delegation.ticks_managed)
                        .unwrap_or_default(),
                    recent_results_json: existing
                        .as_ref()
                        .map(|delegation| delegation.recent_results_json.clone())
                        .unwrap_or_else(|| "[]".to_string()),
                    last_plan_approved: existing
                        .as_ref()
                        .and_then(|delegation| delegation.last_plan_approved),
                    strategy_override: existing
                        .as_ref()
                        .and_then(|delegation| delegation.strategy_override),
                    override_main: owner_kind == "main",
                    cooldown_until_ms: existing
                        .as_ref()
                        .and_then(|delegation| delegation.cooldown_until_ms),
                    suspend_reason: existing
                        .as_ref()
                        .and_then(|delegation| delegation.suspend_reason.clone()),
                    created_at_ms: existing
                        .as_ref()
                        .map(|delegation| delegation.created_at_ms)
                        .unwrap_or(now),
                    updated_at_ms: now,
                })
                .await
                .map_err(loop_state_error)?;
            app.record_vivling_loop_job("delegate", &label, Some(&job), source);
            loop_action_success(
                "delegate",
                thread_id,
                format!("Loop `{label}` delegated to {owner_kind} ({vivling_name})."),
                Some(&job),
                Some(vec![serde_json::json!({
                    "delegation": {
                        "job_id": saved.job_id,
                        "vivling_id": saved.vivling_id,
                        "strategy": saved.strategy.as_str(),
                        "override_main": saved.override_main,
                    }
                })]),
            )
        }
        LoopCommandRequest::Undelegate { label } => {
            let Some(job) = state_runtime
                .get_thread_loop_job_by_label(thread_id, &label)
                .await
                .map_err(loop_state_error)?
            else {
                return Ok(loop_action_failure(
                    "undelegate",
                    thread_id,
                    format!("Loop `{label}` not found."),
                ));
            };
            let removed = state_runtime
                .delete_loop_delegation(thread_id, &job.id)
                .await
                .map_err(loop_state_error)?;
            if !removed {
                return Ok(loop_action_failure(
                    "undelegate",
                    thread_id,
                    format!("Loop `{label}` has no persisted delegation."),
                ));
            }
            app.record_vivling_loop_job("undelegate", &label, Some(&job), source);
            loop_action_success(
                "undelegate",
                thread_id,
                format!("Loop `{label}` returned to its thread-level owner."),
                Some(&job),
                None,
            )
        }
        LoopCommandRequest::Delegation { label } => {
            if let Some(label) = label {
                let Some(job) = state_runtime
                    .get_thread_loop_job_by_label(thread_id, &label)
                    .await
                    .map_err(loop_state_error)?
                else {
                    return Ok(loop_action_failure(
                        "delegation",
                        thread_id,
                        format!("Loop `{label}` not found."),
                    ));
                };
                let Some(delegation) = state_runtime
                    .get_loop_delegation(thread_id, &job.id)
                    .await
                    .map_err(loop_state_error)?
                else {
                    return Ok(loop_action_failure(
                        "delegation",
                        thread_id,
                        format!("Loop `{label}` has no persisted delegation."),
                    ));
                };
                loop_action_success(
                    "delegation",
                    thread_id,
                    format!(
                        "Loop `{label}`: {} strategy={}, override_main={}, suspend_reason={:?}, cooldown_until_ms={:?}.",
                        delegation.vivling_id,
                        delegation.strategy.as_str(),
                        delegation.override_main,
                        delegation.suspend_reason,
                        delegation.cooldown_until_ms,
                    ),
                    Some(&job),
                    None,
                )
            } else {
                let delegations = state_runtime
                    .list_loop_delegations(thread_id)
                    .await
                    .map_err(loop_state_error)?;
                let items = delegations
                    .into_iter()
                    .map(|delegation| {
                        serde_json::json!({
                            "job_id": delegation.job_id,
                            "label": delegation.loop_label,
                            "vivling_id": delegation.vivling_id,
                            "strategy": delegation.strategy.as_str(),
                            "strategy_override": delegation.strategy_override.map(|value| value.as_str()),
                            "override_main": delegation.override_main,
                            "ticks_managed": delegation.ticks_managed,
                            "cooldown_until_ms": delegation.cooldown_until_ms,
                            "suspend_reason": delegation.suspend_reason,
                        })
                    })
                    .collect::<Vec<_>>();
                loop_action_success(
                    "delegation",
                    thread_id,
                    format!("{} persisted loop delegation(s).", items.len()),
                    None,
                    Some(items),
                )
            }
        }
        LoopCommandRequest::SetStrategy { label, strategy } => {
            let Some(job) = state_runtime
                .get_thread_loop_job_by_label(thread_id, &label)
                .await
                .map_err(loop_state_error)?
            else {
                return Ok(loop_action_failure(
                    "strategy",
                    thread_id,
                    format!("Loop `{label}` not found."),
                ));
            };
            let Some(existing) = state_runtime
                .get_loop_delegation(thread_id, &job.id)
                .await
                .map_err(loop_state_error)?
            else {
                return Ok(loop_action_failure(
                    "strategy",
                    thread_id,
                    format!("Loop `{label}` has no persisted delegation."),
                ));
            };
            let requested = strategy.trim();
            let clear_override = requested.eq_ignore_ascii_case("auto");
            let strategy = if clear_override {
                let parsed = codex_state::parse_recent_results(&existing.recent_results_json);
                let metrics = codex_state::LoopMetrics::from_entries(&parsed.entries);
                let Ok((is_adult, brain_enabled, has_profile, bond, _phase)) = app
                    .chat_widget
                    .vivling_loop_management_gate_inputs(&app.config, &existing.vivling_id)
                else {
                    return Ok(loop_action_failure(
                        "strategy",
                        thread_id,
                        "Automatic strategy could not verify its gate inputs.".to_string(),
                    ));
                };
                if is_adult && brain_enabled && has_profile {
                    codex_state::loop_management_strategy(existing.ticks_managed, bond, metrics)
                } else {
                    codex_state::LoopDelegationStrategy::Observe
                }
            } else {
                match codex_state::LoopDelegationStrategy::try_from(requested) {
                    Ok(strategy) => strategy,
                    Err(err) => {
                        return Ok(loop_action_failure("strategy", thread_id, err.to_string()));
                    }
                }
            };
            if strategy == codex_state::LoopDelegationStrategy::Manage
                && !super::vivling_delegation::management_gate_is_green(app, &existing)
            {
                return Ok(loop_action_failure(
                    "strategy",
                    thread_id,
                    "Manage strategy rejected: Adult, brain, profile, and active phase are required."
                        .to_string(),
                ));
            };
            let saved = state_runtime
                .upsert_loop_delegation(codex_state::LoopDelegationUpsertParams {
                    thread_id,
                    job_id: existing.job_id.clone(),
                    loop_label: job.label.clone(),
                    vivling_id: existing.vivling_id.clone(),
                    strategy,
                    ticks_managed: existing.ticks_managed,
                    recent_results_json: existing.recent_results_json.clone(),
                    last_plan_approved: existing.last_plan_approved,
                    strategy_override: (!clear_override).then_some(strategy),
                    override_main: existing.override_main,
                    cooldown_until_ms: existing.cooldown_until_ms,
                    suspend_reason: existing.suspend_reason.clone(),
                    created_at_ms: existing.created_at_ms,
                    updated_at_ms: loop_now_ms(),
                })
                .await
                .map_err(loop_state_error)?;
            app.record_vivling_loop_job("strategy", &label, Some(&job), source);
            loop_action_success(
                "strategy",
                thread_id,
                format!(
                    "Loop `{label}` strategy set to {}.",
                    saved.strategy.as_str()
                ),
                Some(&job),
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
        // `/loop apply`/`/loop dismiss`: puro routing ad eventi
        // Vivling. Nessun tocco allo state DB qui; l'applicazione effettiva
        // (map_to_command + run_command_request ricorsivo) avviene nel
        // handler ApplyLoopSuggestion, sempre gated dal comando utente.
        LoopCommandRequest::Apply { suggestion_id } => {
            app.app_event_tx
                .send_vl(crate::vl::VlEvent::ApplyLoopSuggestion { suggestion_id });
            LoopActionOutcome {
                success: true,
                message: "Suggestion apply dispatched.".to_string(),
                payload: serde_json::json!({
                    "ok": true,
                    "action": "apply_suggestion",
                }),
            }
        }
        LoopCommandRequest::Dismiss { suggestion_id } => {
            app.app_event_tx
                .send_vl(crate::vl::VlEvent::DismissLoopSuggestion { suggestion_id });
            LoopActionOutcome {
                success: true,
                message: "Suggestion dismissed.".to_string(),
                payload: serde_json::json!({
                    "ok": true,
                    "action": "dismiss_suggestion",
                }),
            }
        }
    };

    app.refresh_loop_jobs(thread_id).await?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::managed_request_is_allowed;
    use super::managed_scope_matches;
    use super::validate_runner_model_against_catalog;
    use crate::app::loop_controller::types::LoopCommandScope;
    use crate::vl::events::LoopCommandRequest;
    use codex_protocol::ThreadId;
    use codex_state::LoopRunnerKind;

    #[test]
    fn managed_scope_rejects_a_different_job_even_when_label_is_supplied() {
        let thread_id = ThreadId::new();
        let scope = LoopCommandScope {
            instance_id: "tick-1".to_string(),
            thread_id,
            job_id: "job-1".to_string(),
            label: "safe".to_string(),
        };
        assert!(managed_scope_matches(&scope, thread_id, "job-1", "safe"));
        assert!(!managed_scope_matches(&scope, thread_id, "job-2", "safe"));
        assert!(!managed_scope_matches(&scope, thread_id, "job-1", "other"));
    }

    #[test]
    fn managed_allowlist_has_no_create_or_owner_surface() {
        let request = LoopCommandRequest::Add {
            label: "x".to_string(),
            interval_seconds: 60,
            prompt_text: "x".to_string(),
            goal_text: None,
            auto_remove_on_completion: None,
            runner_kind: LoopRunnerKind::Main,
            runner_model: None,
            schedule_kind: "interval".to_string(),
            schedule_at: None,
            one_shot_at_ms: None,
            tz: None,
            rearm_on_boot: None,
        };
        assert!(!managed_request_is_allowed(&request));
    }

    #[test]
    fn invalid_runner_model_is_rejected_against_the_effective_catalog() {
        let catalog = crate::test_support::TEST_MODEL_PRESETS.clone();
        let known = catalog.first().expect("test catalog is not empty");

        assert!(
            validate_runner_model_against_catalog(
                LoopRunnerKind::ChildAgent,
                Some(known.model.as_str()),
                &catalog,
                "test-provider",
            )
            .is_ok()
        );
        let error = validate_runner_model_against_catalog(
            LoopRunnerKind::ChildAgent,
            Some("model-that-is-not-in-the-provider-catalog"),
            &catalog,
            "test-provider",
        )
        .expect_err("unknown runner model must be rejected");
        assert!(error.to_string().contains("invalid_runner_model"));
    }

    #[test]
    fn child_runner_without_model_is_invalid_without_a_main_fallback() {
        let error = validate_runner_model_against_catalog(
            LoopRunnerKind::ChildAgent,
            None,
            &[],
            "test-provider",
        )
        .expect_err("child runner without model must be rejected");
        assert!(error.to_string().contains("requires `runner_model`"));
    }
}
