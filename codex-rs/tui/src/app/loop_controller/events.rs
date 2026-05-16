//! codex-vl loop_controller: event/refresh handlers.
//!
//! Free functions taking `&mut App` so the facade in `mod.rs` keeps the
//! existing `pub(super) async fn` signatures on `App` byte-identical
//! (`refresh_loop_jobs`, `handle_reload_loop_jobs`). The bodies migrate
//! here so future iters (ticks, vivling delegation) can move next to
//! them without further touching the facade.

use codex_protocol::ThreadId;

use crate::app::App;

use super::state::loop_state_error;

pub(super) async fn refresh_jobs(app: &mut App, thread_id: ThreadId) -> color_eyre::Result<()> {
    let state_runtime = app.loop_state_runtime().await?;
    let jobs = state_runtime
        .list_thread_loop_jobs(thread_id)
        .await
        .map_err(loop_state_error)?;
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

    refresh_jobs(app, thread_id).await
}
