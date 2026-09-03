//! codex-vl loop_controller: re-arm at bootstrap (`rearm_on_boot`).
//!
//! Hook called from `events::handle_reload` (the single loop-restore
//! path, already guarded active-primary). For every enabled, non-pending
//! job whose 0933 descriptor carries `rearm_on_boot`, the hook re-arms
//! the timer when the job is disarmed (`next_run_ms = None`) or when its
//! scheduled occurrence was already claimed in 0934: a claimed occurrence
//! dispatches at-most-once, so a schedule pointer onto it is dead.
//! Idempotency on repeated reloads comes from those skips: a job armed
//! with a live (never-claimed) occurrence is left untouched. The pure
//! scheduler recomputes an expired one-shot to `None` (terminal
//! expired, never resurrected via re-arm) and manually disabled jobs
//! (`enabled = false`) are never touched (no implicit auto-start; the
//! flag survives the disable). The hook writes only the 0930 runtime row
//! (`next_run_ms`, `pending_tick`); the 0933 descriptor stays identical.

use codex_protocol::ThreadId;

use crate::app::App;
use crate::vivling::VivlingLoopEventKind;
use crate::vivling::VivlingLoopEventSource;

use super::state::SchedulePlan;
use super::state::loop_now_ms;
use super::state::loop_state_error;
use super::state::next_run_at_ms;

pub(super) async fn rearm_disarmed_jobs(
    app: &mut App,
    thread_id: ThreadId,
) -> color_eyre::Result<()> {
    let state_runtime = app.loop_state_runtime().await?;
    let jobs = state_runtime
        .list_thread_loop_jobs(thread_id)
        .await
        .map_err(loop_state_error)?;
    let now = loop_now_ms();
    for job in jobs {
        // Manual disables and in-flight (pending) ticks are never re-armed:
        // pending work is the runtime's own retry path, a disable is an
        // explicit user decision the flag must survive, not override.
        if !job.enabled || job.pending_tick {
            continue;
        }
        let Some(descriptor) = state_runtime
            .get_loop_descriptor(&job.id)
            .await
            .map_err(loop_state_error)?
        else {
            continue;
        };
        if !descriptor.rearm_on_boot {
            continue;
        }
        if job.next_run_ms.is_none()
            && job.last_status.as_deref() == Some(super::formatting::LOOP_STATUS_EXPIRED)
        {
            continue;
        }
        let plan = SchedulePlan {
            schedule_kind: &descriptor.schedule_kind,
            interval_seconds: job.interval_seconds,
            schedule_at: descriptor.schedule_at.as_deref(),
            tz: descriptor.tz.as_deref(),
            one_shot_at_ms: descriptor.one_shot_at_ms,
        };
        let scheduled_at_ms = job.next_run_ms;
        let claimed = match scheduled_at_ms {
            Some(scheduled_at_ms) => state_runtime
                .has_loop_occurrence(&job.id, scheduled_at_ms)
                .await
                .map_err(loop_state_error)?,
            None => false,
        };
        // Expiry is checked before the armed/live-occurrence fast path. A
        // persisted one-shot can be armed across a restart, and must not be
        // scheduled or claimed after its grace window.
        if !claimed && super::state::one_shot_expired(&plan, now) {
            state_runtime
                .update_thread_loop_job_runtime(
                    thread_id,
                    &job.id,
                    codex_state::ThreadLoopJobRuntimeUpdate {
                        next_run_ms: None,
                        last_run_ms: job.last_run_ms,
                        last_status: Some(super::formatting::LOOP_STATUS_EXPIRED.to_string()),
                        last_error: None,
                        pending_tick: false,
                        updated_at_ms: now,
                    },
                )
                .await
                .map_err(loop_state_error)?;
            if let Some(expired_job) = state_runtime
                .get_thread_loop_job_by_id(thread_id, &job.id)
                .await
                .map_err(loop_state_error)?
            {
                let mut summary = super::notify::build_tick_summary(
                    &expired_job,
                    Some(&descriptor),
                    None,
                    super::summary::LoopManager::Main,
                    "bootstrap_expired".to_string(),
                    scheduled_at_ms,
                    false,
                    now,
                    now,
                );
                summary.outcome = super::summary::LoopTickOutcome::OneShotExpired;
                if let Err(err) =
                    super::notify::persist_and_emit(app, &state_runtime, &summary).await
                {
                    tracing::warn!(
                        target: "codex_vl::loop_summary",
                        error = %err,
                        "expired one-shot summary persistence failed"
                    );
                }
            }
            continue;
        }
        // Armed with a live (never-claimed) occurrence: the timer is real,
        // nothing to re-arm (idempotent on repeated reloads).
        if scheduled_at_ms.is_some() && !claimed {
            continue;
        }
        // Pure scheduler: interval/at resolve the next future instant;
        // an expired one-shot recomputes to `None` and stays disarmed.
        let next_run_ms = next_run_at_ms(&plan, now);
        let Some(next_run_ms) = next_run_ms else {
            // A one-shot past its grace is terminal
            // but NOT silent at bootstrap: persist the terminal `expired`
            // status and the tick summary + pending row (an anomalous completion is recorded like any other).
            if descriptor.schedule_kind == "one_shot" {
                state_runtime
                    .update_thread_loop_job_runtime(
                        thread_id,
                        &job.id,
                        codex_state::ThreadLoopJobRuntimeUpdate {
                            next_run_ms: None,
                            last_run_ms: job.last_run_ms,
                            last_status: Some(super::formatting::LOOP_STATUS_EXPIRED.to_string()),
                            last_error: None,
                            pending_tick: false,
                            updated_at_ms: now,
                        },
                    )
                    .await
                    .map_err(loop_state_error)?;
                let Some(expired_job) = state_runtime
                    .get_thread_loop_job_by_id(thread_id, &job.id)
                    .await
                    .map_err(loop_state_error)?
                else {
                    continue;
                };
                let mut summary = super::notify::build_tick_summary(
                    &expired_job,
                    Some(&descriptor),
                    None,
                    super::summary::LoopManager::Main,
                    "bootstrap_expired".to_string(),
                    /*occurrence_ms*/ scheduled_at_ms,
                    /*occurrence_claimed*/ false,
                    now,
                    now,
                );
                // The scheduler has already ruled: this is the terminal
                // expired esito, whatever the row-derived derivation says.
                summary.outcome = super::summary::LoopTickOutcome::OneShotExpired;
                if let Err(err) =
                    super::notify::persist_and_emit(app, &state_runtime, &summary).await
                {
                    tracing::warn!(
                        target: "codex_vl::loop_summary",
                        error = %err,
                        "expired one-shot summary persistence failed"
                    );
                }
            }
            continue;
        };
        state_runtime
            .update_thread_loop_job_runtime(
                thread_id,
                &job.id,
                codex_state::ThreadLoopJobRuntimeUpdate {
                    next_run_ms: Some(next_run_ms),
                    last_run_ms: job.last_run_ms,
                    last_status: job.last_status.clone(),
                    last_error: job.last_error.clone(),
                    pending_tick: false,
                    updated_at_ms: now,
                },
            )
            .await
            .map_err(loop_state_error)?;
        tracing::info!(
            target: "codex_vl::loop_schedule",
            label = %job.label,
            next_run_ms,
            "re-armed loop at bootstrap (rearm_on_boot)"
        );
        app.chat_widget.record_vivling_loop_event(
            VivlingLoopEventKind::Runtime,
            VivlingLoopEventSource::Agent,
            "rearm",
            &job.label,
            Some("scheduled"),
            None,
            job.goal_text.as_deref(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::rearm_disarmed_jobs;
    use crate::app::tests::make_test_app_with_channels;
    use crate::app::tests::test_thread_session;
    use codex_protocol::ThreadId;
    use codex_state::LoopDescriptorUpsertParams;
    use codex_state::LoopRunnerKind;
    use codex_state::SqliteConfig;
    use codex_state::StateRuntime;
    use codex_state::ThreadLoopJobCreateParams;
    use codex_utils_absolute_path::test_support::PathExt;
    use tempfile::tempdir;

    async fn runtime() -> anyhow::Result<(std::sync::Arc<StateRuntime>, tempfile::TempDir)> {
        let codex_home = tempdir()?;
        let state_runtime = StateRuntime::init(
            SqliteConfig::new_for_testing(codex_home.path().abs()),
            "test-provider".to_string(),
        )
        .await?;
        Ok((state_runtime, codex_home))
    }

    async fn create_job(
        state_runtime: &StateRuntime,
        thread_id: ThreadId,
        label: &str,
        next_run_ms: Option<i64>,
    ) -> anyhow::Result<codex_state::ThreadLoopJob> {
        state_runtime
            .create_or_replace_thread_loop_job(ThreadLoopJobCreateParams {
                id: format!("job-{label}"),
                thread_id,
                label: label.to_string(),
                prompt_text: "tick".to_string(),
                goal_text: Some("test rearm".to_string()),
                interval_seconds: 60,
                enabled: true,
                run_policy: "queue_one".to_string(),
                auto_remove_on_completion: true,
                created_by: "agent".to_string(),
                next_run_ms,
                created_at_ms: 1_700_000_000_000,
                updated_at_ms: 1_700_000_000_000,
            })
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        state_runtime
            .get_thread_loop_job_by_id(thread_id, &format!("job-{label}"))
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow::anyhow!("job just created"))
    }

    async fn persist_pending(
        state_runtime: &StateRuntime,
        thread_id: ThreadId,
        job_id: &str,
    ) -> anyhow::Result<()> {
        state_runtime
            .update_thread_loop_job_runtime(
                thread_id,
                job_id,
                codex_state::ThreadLoopJobRuntimeUpdate {
                    next_run_ms: None,
                    last_run_ms: None,
                    last_status: None,
                    last_error: None,
                    pending_tick: true,
                    updated_at_ms: 1_700_000_000_002,
                },
            )
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        Ok(())
    }

    async fn set_rearm_flag(
        state_runtime: &StateRuntime,
        job_id: &str,
        rearm_on_boot: bool,
        schedule_kind: &str,
        one_shot_at_ms: Option<i64>,
        now: i64,
    ) -> anyhow::Result<()> {
        state_runtime
            .upsert_loop_descriptor(LoopDescriptorUpsertParams {
                job_id: job_id.to_string(),
                runner_kind: LoopRunnerKind::Main,
                runner_model: None,
                runner_reasoning_effort: None,
                tz: None,
                schedule_kind: schedule_kind.to_string(),
                schedule_at: None,
                one_shot_at_ms,
                rearm_on_boot,
                updated_at_ms: now,
            })
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        Ok(())
    }

    async fn load_job(
        state_runtime: &StateRuntime,
        thread_id: ThreadId,
        job_id: &str,
    ) -> anyhow::Result<codex_state::ThreadLoopJob> {
        state_runtime
            .get_thread_loop_job_by_id(thread_id, job_id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow::anyhow!("job {job_id} missing"))
    }

    // A disarmed interval loop with rearm_on_boot=1 is re-armed by the
    // hook, and the same test pins that the 0933 descriptor is untouched.
    #[tokio::test]
    async fn rearm_schedules_disarmed_interval_job_and_keeps_descriptor() -> anyhow::Result<()> {
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
        let (state_runtime, _codex_home) = runtime().await?;
        app.state_db = Some(state_runtime.clone());

        let thread_id = ThreadId::new();
        app.primary_thread_id = Some(thread_id);
        app.active_thread_id = Some(thread_id);
        let cwd = app.config.cwd.to_path_buf();
        app.chat_widget
            .handle_thread_session(test_thread_session(thread_id, cwd));
        let job = create_job(&state_runtime, thread_id, "rearmed", None).await?;
        let before = state_runtime
            .get_loop_descriptor(&job.id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        // The descriptor is created with the job (default `rearm_on_boot`
        // off) and must survive the re-arm unchanged: the hook is allowed to
        // move the runtime row only, never the descriptor.
        assert!(
            before
                .as_ref()
                .is_some_and(|descriptor| !descriptor.rearm_on_boot),
            "the descriptor is born with the job: default rearm_on_boot off"
        );
        set_rearm_flag(&state_runtime, &job.id, true, "interval", None, 1).await?;
        let flagged = state_runtime
            .get_loop_descriptor(&job.id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?
            .expect("descriptor present");

        rearm_disarmed_jobs(&mut app, thread_id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;

        let after = load_job(&state_runtime, thread_id, &job.id).await?;
        assert!(
            after.next_run_ms.is_some(),
            "disarmed rearm_on_boot job must be re-armed"
        );
        assert!(after.next_run_ms.unwrap() > 1_700_000_000_000);
        assert!(!after.pending_tick, "re-arm clears a stale pending flag");
        // The hook writes only the 0930 runtime row: the 0933 descriptor
        // (minus updated_at_ms, untouched by the hook) stays byte-identical.
        let after_descriptor = state_runtime
            .get_loop_descriptor(&job.id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?
            .expect("descriptor still present");
        assert_eq!(after_descriptor, flagged);
        Ok(())
    }

    // Re-arm idempotency: a second reload must not re-arm (or move) a job that
    // is already armed with a live occurrence.
    #[tokio::test]
    async fn second_reload_does_not_rearm_an_armed_job() -> anyhow::Result<()> {
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
        let (state_runtime, _codex_home) = runtime().await?;
        app.state_db = Some(state_runtime.clone());

        let thread_id = ThreadId::new();
        app.primary_thread_id = Some(thread_id);
        app.active_thread_id = Some(thread_id);
        let cwd = app.config.cwd.to_path_buf();
        app.chat_widget
            .handle_thread_session(test_thread_session(thread_id, cwd));
        let job = create_job(&state_runtime, thread_id, "armed", None).await?;
        set_rearm_flag(&state_runtime, &job.id, true, "interval", None, 1).await?;

        rearm_disarmed_jobs(&mut app, thread_id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        let first = load_job(&state_runtime, thread_id, &job.id).await?;
        let armed_at = first
            .next_run_ms
            .expect("first reload must arm the disarmed job");

        rearm_disarmed_jobs(&mut app, thread_id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        let second = load_job(&state_runtime, thread_id, &job.id).await?;
        assert_eq!(
            second.next_run_ms,
            Some(armed_at),
            "second reload must not move the armed schedule"
        );
        assert!(
            !state_runtime
                .has_loop_occurrence(&job.id, armed_at)
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?,
            "an armed live occurrence must stay unclaimed (the tick owns the CAS)"
        );
        Ok(())
    }

    // With rearm_on_boot=0 today's behaviour holds — a disarmed job stays
    // disarmed through the reload hook.
    #[tokio::test]
    async fn flag_false_leaves_the_disarmed_job_untouched() -> anyhow::Result<()> {
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
        let (state_runtime, _codex_home) = runtime().await?;
        app.state_db = Some(state_runtime.clone());

        let thread_id = ThreadId::new();
        app.primary_thread_id = Some(thread_id);
        app.active_thread_id = Some(thread_id);
        let cwd = app.config.cwd.to_path_buf();
        app.chat_widget
            .handle_thread_session(test_thread_session(thread_id, cwd));
        let job = create_job(&state_runtime, thread_id, "no-rearm", None).await?;
        set_rearm_flag(&state_runtime, &job.id, false, "interval", None, 1).await?;

        rearm_disarmed_jobs(&mut app, thread_id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;

        let after = load_job(&state_runtime, thread_id, &job.id).await?;
        assert_eq!(
            after.next_run_ms, None,
            "rearm_on_boot=false must keep the disarmed job disarmed"
        );
        assert!(!after.pending_tick);
        Ok(())
    }

    // an expired one-shot is terminal: re-arm recomputes the
    // schedule to None and must never resurrect it. A one-shot inside its
    // grace window whose occurrence was already claimed is equally dead:
    // recomputing would yield the same instant, which loses the CAS, so the
    // job stays disarmed too.
    #[tokio::test]
    async fn expired_one_shot_is_never_resurrected_by_rearm() -> anyhow::Result<()> {
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
        let (state_runtime, _codex_home) = runtime().await?;
        app.state_db = Some(state_runtime.clone());

        let thread_id = ThreadId::new();
        app.primary_thread_id = Some(thread_id);
        app.active_thread_id = Some(thread_id);
        let cwd = app.config.cwd.to_path_buf();
        app.chat_widget
            .handle_thread_session(test_thread_session(thread_id, cwd));
        let now = 1_700_000_000_000i64;
        let expired_at = now - 10 * 60 * 1000; // past the grace window
        let job = create_job(&state_runtime, thread_id, "expired-shot", Some(expired_at)).await?;
        set_rearm_flag(
            &state_runtime,
            &job.id,
            true,
            "one_shot",
            Some(expired_at),
            1,
        )
        .await?;

        rearm_disarmed_jobs(&mut app, thread_id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;

        let after = load_job(&state_runtime, thread_id, &job.id).await?;
        assert_eq!(
            after.next_run_ms, None,
            "an expired one-shot must never be re-armed"
        );
        assert!(
            !state_runtime
                .has_loop_occurrence(&job.id, expired_at)
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?,
            "no occurrence row may be fabricated by the re-arm"
        );
        rearm_disarmed_jobs(&mut app, thread_id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        assert_eq!(
            state_runtime.list_pending_loop_notifications().await?.len(),
            1,
            "reloading an expired armed one-shot must not duplicate its summary"
        );
        Ok(())
    }

    // A pending tick belongs to the runtime's own retry path: the hook must
    // not compete with it, whatever the flag says.
    #[tokio::test]
    async fn pending_tick_job_is_skipped_even_with_the_flag() -> anyhow::Result<()> {
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
        let (state_runtime, _codex_home) = runtime().await?;
        app.state_db = Some(state_runtime.clone());

        let thread_id = ThreadId::new();
        app.primary_thread_id = Some(thread_id);
        app.active_thread_id = Some(thread_id);
        let cwd = app.config.cwd.to_path_buf();
        app.chat_widget
            .handle_thread_session(test_thread_session(thread_id, cwd));
        let job = create_job(&state_runtime, thread_id, "pending", None).await?;
        set_rearm_flag(&state_runtime, &job.id, true, "interval", None, 1).await?;
        persist_pending(&state_runtime, thread_id, &job.id).await?;

        rearm_disarmed_jobs(&mut app, thread_id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;

        let after = load_job(&state_runtime, thread_id, &job.id).await?;
        assert_eq!(
            after.next_run_ms, None,
            "a pending job must stay on the runtime retry path"
        );
        assert!(after.pending_tick);
        Ok(())
    }

    // A manually disabled job stays disabled across reloads: the flag
    // survives the disable (it stays persisted on the descriptor) but a
    // disable is an explicit user decision, never an auto-start input.
    #[tokio::test]
    async fn manually_disabled_job_is_not_rearmed() -> anyhow::Result<()> {
        let (mut app, _events, _ops) = make_test_app_with_channels().await;
        let (state_runtime, _codex_home) = runtime().await?;
        app.state_db = Some(state_runtime.clone());

        let thread_id = ThreadId::new();
        app.primary_thread_id = Some(thread_id);
        app.active_thread_id = Some(thread_id);
        let cwd = app.config.cwd.to_path_buf();
        app.chat_widget
            .handle_thread_session(test_thread_session(thread_id, cwd));
        let job = create_job(&state_runtime, thread_id, "disabled", None).await?;
        set_rearm_flag(&state_runtime, &job.id, true, "interval", None, 1).await?;
        state_runtime
            .set_thread_loop_job_enabled(thread_id, &job.label, false, None, 1_700_000_000_001)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;

        rearm_disarmed_jobs(&mut app, thread_id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;

        let after = load_job(&state_runtime, thread_id, &job.id).await?;
        assert!(!after.enabled, "manual disable must survive the reload");
        assert_eq!(after.next_run_ms, None);
        // The flag survives the manual disable on the descriptor.
        let descriptor = state_runtime
            .get_loop_descriptor(&job.id)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?
            .expect("descriptor present");
        assert!(descriptor.rearm_on_boot);
        Ok(())
    }
}
