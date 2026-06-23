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

use crate::app::App;
use crate::vivling::VivlingLoopTickResult;
use crate::vl::VlEvent;
use crate::vl::events::LoopCommandRequest;

use super::formatting::LOOP_STATUS_BLOCKED;
use super::formatting::LOOP_STATUS_BLOCKED_OWNER;
use super::formatting::LOOP_STATUS_DONE;
use super::formatting::LOOP_STATUS_PROGRESS;
use super::jobs;
use super::parsing::parse_manage_loops_interval_seconds;
use super::parsing::parse_vivling_loop_status;
use super::state::loop_now_ms;
use super::state::loop_state_error;
use super::types::LoopCommandSource;

pub(super) async fn handle_loop_tick_finished(
    app: &mut App,
    thread_id: ThreadId,
    job_id: String,
    result: Result<VivlingLoopTickResult, String>,
) -> color_eyre::Result<()> {
    let state_runtime = app.loop_state_runtime().await?;
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
                && let Err(persist_err) = app
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
            app.chat_widget
                .add_error_message(format!("Vivling loop `{}` failed: {err}", job.label));
            app.record_vivling_loop_runtime(
                &job.label,
                Some("pending"),
                Some(LOOP_STATUS_BLOCKED_OWNER),
                job.goal_text.as_deref().or(Some(job.prompt_text.as_str())),
                &job.created_by,
            );
            app.refresh_loop_jobs(thread_id).await?;
            return Ok(());
        }
        Ok(result) => {
            if let Some(vivling_id) = owner_vivling_id.as_deref()
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
                let _ =
                    jobs::run_command_request(app, thread_id, request, LoopCommandSource::Agent)
                        .await?;
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

pub(super) fn run_assist(app: &mut App, request: crate::vivling::VivlingAssistRequest) {
    let app_event_tx = app.app_event_tx.clone();
    let config = crate::app::vivling_background::config_with_session_model(
        &app.config,
        app.chat_widget.effective_collaboration_mode().model(),
    );
    let session_telemetry = app.session_telemetry.clone();
    tokio::spawn(async move {
        let vivling_id = request.vivling_id.clone();
        let kind = request.kind.clone();
        let result = crate::app::vivling_background::run_vivling_assist_request(
            config,
            session_telemetry,
            request,
        )
        .await;
        app_event_tx.send_vl(VlEvent::VivlingAssistFinished {
            vivling_id,
            kind,
            result,
        });
    });
}

pub(super) fn run_loop_tick(
    app: &mut App,
    thread_id: ThreadId,
    job_id: String,
    request: crate::vivling::VivlingLoopTickRequest,
) {
    let app_event_tx = app.app_event_tx.clone();
    let config = crate::app::vivling_background::config_with_session_model(
        &app.config,
        app.chat_widget.effective_collaboration_mode().model(),
    );
    let session_telemetry = app.session_telemetry.clone();
    tokio::spawn(async move {
        let result = crate::app::vivling_background::run_vivling_loop_tick_request(
            config,
            session_telemetry,
            request,
        )
        .await;
        app_event_tx.send_vl(VlEvent::VivlingLoopTickFinished {
            thread_id,
            job_id,
            result,
        });
    });
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
