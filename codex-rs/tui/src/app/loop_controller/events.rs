//! codex-vl loop_controller: event/refresh handlers.
//!
//! Free functions taking `&mut App` so the facade in `mod.rs` keeps the
//! existing `pub(super) async fn` signatures on `App` byte-identical
//! (`refresh_loop_jobs`, `handle_reload_loop_jobs`). The bodies migrate
//! here so future iters (ticks, vivling delegation) can move next to
//! them without further touching the facade.

use codex_protocol::ThreadId;

use crate::app::App;

use super::formatting::LOOP_STATUS_EXPIRED;
use super::state::loop_now_ms;
use super::state::loop_state_error;
use super::state::one_shot_expired;

async fn expire_stale_one_shots(
    app: &mut App,
    state_runtime: &codex_state::StateRuntime,
    jobs: &[codex_state::ThreadLoopJob],
) -> color_eyre::Result<bool> {
    let now = loop_now_ms();
    let mut changed = false;
    for job in jobs {
        let Some(scheduled_at_ms) = job.next_run_ms else {
            continue;
        };
        let Some(descriptor) = state_runtime
            .get_loop_descriptor(&job.id)
            .await
            .map_err(loop_state_error)?
        else {
            continue;
        };
        let Some(one_shot_at_ms) = descriptor.one_shot_at_ms else {
            continue;
        };
        let plan = super::state::SchedulePlan {
            schedule_kind: &descriptor.schedule_kind,
            interval_seconds: job.interval_seconds,
            schedule_at: descriptor.schedule_at.as_deref(),
            tz: descriptor.tz.as_deref(),
            one_shot_at_ms: Some(one_shot_at_ms),
        };
        if !one_shot_expired(&plan, now)
            || state_runtime
                .has_loop_occurrence(&job.id, scheduled_at_ms)
                .await
                .map_err(loop_state_error)?
        {
            continue;
        }
        state_runtime
            .update_thread_loop_job_runtime(
                job.thread_id,
                &job.id,
                codex_state::ThreadLoopJobRuntimeUpdate {
                    next_run_ms: None,
                    last_run_ms: job.last_run_ms,
                    last_status: Some(LOOP_STATUS_EXPIRED.to_string()),
                    last_error: None,
                    pending_tick: false,
                    updated_at_ms: now,
                },
            )
            .await
            .map_err(loop_state_error)?;
        // An expired one-shot is a terminal outcome, not silence: the summary
        // (and its pending notification) is persisted and emitted like every
        // other finished tick. The row is re-read after the update so the
        // summary reports the disarmed state, not the stale instant.
        if let Some(job_after) = state_runtime
            .get_thread_loop_job_by_id(job.thread_id, &job.id)
            .await
            .map_err(loop_state_error)?
        {
            if let Err(err) = super::notify::persist_summary_with_outcome(
                app,
                state_runtime,
                &job_after,
                Some(&descriptor),
                None,
                super::summary::LoopManager::Main,
                "expired".to_string(),
                Some(scheduled_at_ms),
                super::summary::LoopTickOutcome::OneShotExpired,
                now,
            )
            .await
            {
                tracing::warn!(
                    target: "codex_vl::loop_summary",
                    error = %err,
                    "expired one-shot summary persistence failed; emission suppressed"
                );
            }
        }
        changed = true;
    }
    Ok(changed)
}

pub(super) async fn refresh_jobs(app: &mut App, thread_id: ThreadId) -> color_eyre::Result<()> {
    let state_runtime = app.loop_state_runtime().await?;
    let mut jobs = state_runtime
        .list_thread_loop_jobs(thread_id)
        .await
        .map_err(loop_state_error)?;
    if expire_stale_one_shots(app, &state_runtime, &jobs).await? {
        jobs = state_runtime
            .list_thread_loop_jobs(thread_id)
            .await
            .map_err(loop_state_error)?;
    }
    let owner = state_runtime
        .get_thread_loop_owner(thread_id)
        .await
        .map_err(loop_state_error)?;
    app.chat_widget
        .replace_loop_jobs_with_owner(thread_id, jobs, owner);
    Ok(())
}

pub(super) async fn handle_reload(app: &mut App, thread_id: ThreadId) -> color_eyre::Result<()> {
    if app.primary_thread_id != Some(thread_id) || app.chat_widget.thread_id() != Some(thread_id) {
        app.chat_widget.clear_loop_jobs();
        return Ok(());
    }

    let state_runtime = app.loop_state_runtime().await?;
    let jobs = state_runtime
        .list_thread_loop_jobs(thread_id)
        .await
        .map_err(loop_state_error)?;

    if let Some(pending_job) = jobs
        .iter()
        .find(|job| job.enabled && job.pending_tick)
        .cloned()
    {
        super::ticks::process_submission(app, thread_id, pending_job).await?;
    }

    // re-arm `rearm_on_boot` descriptors on the single loop-restore
    // path (idempotent: armed-with-live-occurrence jobs are left alone).
    super::rearm::rearm_disarmed_jobs(app, thread_id).await?;

    refresh_jobs(app, thread_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::make_test_app_with_channels;
    use codex_state::LoopDescriptorUpsertParams;
    use codex_state::LoopRunnerKind;
    use codex_state::SqliteConfig;
    use codex_state::StateRuntime;
    use codex_state::ThreadLoopJobCreateParams;
    use codex_utils_absolute_path::test_support::PathExt;
    use tempfile::tempdir;

    // A one-shot that expires without an occurrence is a finished tick: it
    // must produce a summary with the expired outcome (and its pending),
    // instead of leaving the runtime row alone.
    #[tokio::test]
    async fn expired_one_shot_produces_a_summary() -> anyhow::Result<()> {
        let (mut app, mut events, _ops) = make_test_app_with_channels().await;
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

        let job = state_runtime
            .create_or_replace_thread_loop_job(ThreadLoopJobCreateParams {
                id: "job-expired".to_string(),
                thread_id,
                label: "expired".to_string(),
                prompt_text: "one shot".to_string(),
                goal_text: None,
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
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        state_runtime
            .upsert_loop_descriptor(codex_state::LoopDescriptorUpsertParams {
                job_id: job.id.clone(),
                runner_kind: LoopRunnerKind::Main,
                runner_model: None,
                runner_reasoning_effort: None,
                tz: None,
                schedule_kind: "one_shot".to_string(),
                schedule_at: None,
                one_shot_at_ms: Some(1_700_000_000_000),
                rearm_on_boot: false,
                updated_at_ms: 1_700_000_000_000,
            })
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;

        let jobs = vec![job.clone()];
        expire_stale_one_shots(&mut app, &state_runtime, &jobs)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;

        // The summary and its pending are persisted with the expired outcome.
        assert_eq!(
            state_runtime
                .count_loop_notifications(&job.id, codex_state::LOOP_NOTIFICATION_KIND_SUMMARY)
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?,
            1,
            "the expired one-shot must leave exactly one summary"
        );
        assert_eq!(
            state_runtime
                .count_loop_notifications(&job.id, codex_state::LOOP_NOTIFICATION_KIND_PENDING)
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?,
            1,
            "an expired one-shot is an anomalous event: one pending"
        );

        // The emitted event carries the expired outcome.
        let mut saw_expired_summary = false;
        while let Ok(app_event) = events.try_recv() {
            if let crate::app_event::AppEvent::Vl(crate::vl::VlEvent::LoopTickSummary { summary }) =
                app_event
            {
                assert!(matches!(
                    summary.outcome,
                    super::super::summary::LoopTickOutcome::OneShotExpired
                ));
                saw_expired_summary = true;
            }
        }
        assert!(
            saw_expired_summary,
            "the expiry must emit one loop tick summary event"
        );
        Ok(())
    }
}
