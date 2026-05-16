//! codex-vl loop_controller: status constants + narrating helpers +
//! JSON payload builders.

use codex_protocol::ThreadId;

use super::types::LoopActionOutcome;

pub(super) const LOOP_STATUS_SUBMITTED: &str = "submitted";
pub(super) const LOOP_STATUS_PENDING_BUSY: &str = "pending_busy";
pub(super) const LOOP_STATUS_BLOCKED_REVIEW: &str = "blocked_review";
pub(super) const LOOP_STATUS_BLOCKED_SIDE: &str = "blocked_side";
pub(super) const LOOP_STATUS_BLOCKED_OWNER: &str = "blocked_owner";
pub(super) const LOOP_STATUS_DELEGATED_VIVLING: &str = "delegated_vivling";
pub(super) const LOOP_STATUS_PROGRESS: &str = "progress";
pub(super) const LOOP_STATUS_BLOCKED: &str = "blocked";
pub(super) const LOOP_STATUS_DONE: &str = "done";
pub(super) const LOOP_STATUS_DISABLED: &str = "disabled";
pub(super) const LOOP_STATUS_REMOVED: &str = "removed";

pub(super) fn format_loop_interval(interval_seconds: i64) -> String {
    if interval_seconds % 3600 == 0 {
        format!("{}h", interval_seconds / 3600)
    } else if interval_seconds % 60 == 0 {
        format!("{}m", interval_seconds / 60)
    } else {
        format!("{interval_seconds}s")
    }
}

pub(super) fn canonical_last_status(job: &codex_state::ThreadLoopJob) -> Option<String> {
    match job.last_status.as_deref() {
        Some("pending") => Some(LOOP_STATUS_PENDING_BUSY.to_string()),
        Some(status) if !status.trim().is_empty() => Some(status.to_string()),
        _ => None,
    }
}

pub(super) fn loop_runtime_state(job: &codex_state::ThreadLoopJob) -> &'static str {
    if !job.enabled {
        "disabled"
    } else if job.pending_tick {
        "pending"
    } else if job.next_run_ms.is_some() {
        "scheduled"
    } else {
        "unscheduled"
    }
}

pub(super) fn thread_loop_owner_summary(owner: &codex_state::ThreadLoopOwner) -> String {
    match owner.owner_kind.as_str() {
        codex_state::THREAD_LOOP_OWNER_KIND_VIVLING => format!(
            "vivling ({})",
            owner.owner_vivling_id.as_deref().unwrap_or("missing")
        ),
        _ => codex_state::THREAD_LOOP_OWNER_KIND_MAIN.to_string(),
    }
}

pub(super) fn format_loop_job_line(job: &codex_state::ThreadLoopJob) -> String {
    let enabled = if job.enabled { "on" } else { "off" };
    let status = canonical_last_status(job).unwrap_or_else(|| "never".to_string());
    let cleanup = if job.auto_remove_on_completion {
        "auto-remove"
    } else {
        "keep"
    };
    format!(
        "{} [{}] every {} | runtime={} | status={} | {}",
        job.label,
        enabled,
        format_loop_interval(job.interval_seconds),
        loop_runtime_state(job),
        status,
        cleanup
    )
}

pub(super) fn summarize_loop_goal(job: &codex_state::ThreadLoopJob) -> String {
    job.goal_text
        .as_deref()
        .filter(|goal| !goal.trim().is_empty())
        .unwrap_or(&job.prompt_text)
        .to_string()
}

pub(super) fn format_loop_job_details(job: &codex_state::ThreadLoopJob) -> String {
    let next_run = job
        .next_run_ms
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string());
    let last_run = job
        .last_run_ms
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string());
    let last_status = canonical_last_status(job).unwrap_or_else(|| "never".to_string());
    let last_error = job.last_error.as_deref().unwrap_or("none");
    let goal = summarize_loop_goal(job);
    format!(
        "label: {}\nenabled: {}\ninterval: {}\nruntime_state: {}\nlast_status: {}\npending: {}\nauto_remove_on_completion: {}\ncreated_by: {}\ngoal: {}\nprompt: {}\nnext_run_ms: {}\nlast_run_ms: {}\nlast_error: {}",
        job.label,
        job.enabled,
        format_loop_interval(job.interval_seconds),
        loop_runtime_state(job),
        last_status,
        job.pending_tick,
        job.auto_remove_on_completion,
        job.created_by,
        goal,
        job.prompt_text,
        next_run,
        last_run,
        last_error
    )
}

pub(super) fn loop_job_json(job: &codex_state::ThreadLoopJob) -> serde_json::Value {
    serde_json::json!({
        "label": job.label,
        "enabled": job.enabled,
        "interval_seconds": job.interval_seconds,
        "prompt_text": job.prompt_text,
        "goal_text": job.goal_text,
        "auto_remove_on_completion": job.auto_remove_on_completion,
        "created_by": job.created_by,
        "run_policy": job.run_policy,
        "runtime_state": loop_runtime_state(job),
        "last_status": canonical_last_status(job),
        "last_error": job.last_error,
        "pending_tick": job.pending_tick,
        "next_run_ms": job.next_run_ms,
        "last_run_ms": job.last_run_ms,
    })
}

pub(super) fn loop_success_payload(
    action: &str,
    thread_id: ThreadId,
    job: Option<&codex_state::ThreadLoopJob>,
    jobs: Option<Vec<serde_json::Value>>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "ok": true,
        "action": action,
        "thread_id": thread_id.to_string(),
    });
    let object = payload.as_object_mut().expect("payload must be object");
    if let Some(job) = job {
        object.insert("job".to_string(), loop_job_json(job));
    }
    if let Some(jobs) = jobs {
        object.insert("jobs".to_string(), serde_json::Value::Array(jobs));
    }
    payload
}

pub(super) fn loop_error_payload(
    action: &str,
    thread_id: ThreadId,
    error: String,
) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "action": action,
        "thread_id": thread_id.to_string(),
        "error": error,
    })
}

pub(super) fn loop_action_success(
    action: &str,
    thread_id: ThreadId,
    message: String,
    job: Option<&codex_state::ThreadLoopJob>,
    jobs: Option<Vec<serde_json::Value>>,
) -> LoopActionOutcome {
    LoopActionOutcome {
        success: true,
        message,
        payload: loop_success_payload(action, thread_id, job, jobs),
    }
}

pub(super) fn loop_action_failure(
    action: &str,
    thread_id: ThreadId,
    message: String,
) -> LoopActionOutcome {
    LoopActionOutcome {
        success: false,
        payload: loop_error_payload(action, thread_id, message.clone()),
        message,
    }
}
