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
    if expire_stale_one_shots(&state_runtime, &jobs).await? {
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

    // T4 — re-arm `rearm_on_boot` descriptors on the single loop-restore
    // path (idempotent: armed-with-live-occurrence jobs are left alone).
    super::rearm::rearm_disarmed_jobs(app, thread_id).await?;

    refresh_jobs(app, thread_id).await
}
