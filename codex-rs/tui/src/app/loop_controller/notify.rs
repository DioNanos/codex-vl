//! codex-vl loop_controller: T6 m2 — bounded queue, separate consumer and
//! bootstrap replay for the persisted tick summaries.
//!
//! Seam contract (R11): the tick path only **persists** (m1 gate 1) and then
//! `try_send`s onto a fixed-capacity channel — the queue is transport only,
//! a full queue drops the emission, never the tick, and no channel error
//! ever reaches the tick. The consumer below is a distinct task: it drains
//! the queue and, for the events R3 admits (anomalies + one-shots), tries
//! the external notifier **only if configured**. An absent channel is a
//! normal state: the persisted `pending` rows stay untouched and replay at
//! the next bootstrap (once per start; the m1 `INSERT OR IGNORE` on
//! `event_id` is the dedup verdict).
//!
//! The external notifier is **off by default** (declared Dev decision
//! 21:57): `LoopNotifyChannel::from_config()` returns [`LoopNotifyChannel::Absent`]
//! until the configuration key for the NexusCrew server name is decided —
//! the delivery seam is already there (`core/src/hook_mcp_executor.rs`
//! `CoreHookMcpExecutor` on `codex_mcp::McpRuntime`; `codex-mcp` in
//! `tui/Cargo.toml`), so wiring it is a one-variant extension of
//! [`LoopNotifyChannel`], no new crates.

use std::sync::OnceLock;

use codex_state::LOOP_NOTIFICATION_KIND_PENDING;
use codex_state::LOOP_NOTIFICATION_KIND_SUMMARY;
use codex_state::LoopNotificationRecord;
use tokio::sync::mpsc;

use super::state::loop_now_ms;
use super::summary::LoopManager;
use super::summary::LoopScheduleKind;
use super::summary::LoopTickOutcome;
use super::summary::LoopTickSummary;
use super::summary::NextRun;

/// Fixed queue capacity (R11 gate 2): bounded, never grows with the tick
/// count; overflow drops the emission and keeps the persisted row.
pub(crate) const LOOP_SUMMARY_QUEUE_CAPACITY: usize = 32;

static LOOP_SUMMARY_TX: OnceLock<mpsc::Sender<LoopTickSummary>> = OnceLock::new();

/// Where the external notifier delivers. `Absent` is the shipped default
/// (off by default, normal state — not an error).
pub(crate) enum LoopNotifyChannel {
    Absent,
}

impl LoopNotifyChannel {
    pub(crate) fn from_config() -> Self {
        // Declared decision (Dev 21:57): the NexusCrew server-name
        // configuration key is not chosen yet, so the channel ships off.
        LoopNotifyChannel::Absent
    }
}

/// Outcome of a finished tick, derived from the persisted post-tick row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DerivedOutcome {
    Ok,
    Failed,
    SkippedBusy,
    OneShotExpired,
}

/// Persist-before-emit (R11 gate 1) + bounded enqueue (gate 2). Returns
/// `true` when this is the first persistence of the event and the caller
/// may emit it; `false` = duplicate `event_id` (already durable, never emit
/// again). The pending row (anomalies + one-shots only) is written here,
/// before the queue — a later failed delivery always has its row.
pub(crate) async fn persist_and_queue(
    state_runtime: &codex_state::StateRuntime,
    summary: &LoopTickSummary,
) -> anyhow::Result<bool> {
    let first = state_runtime
        .record_loop_notification(LoopNotificationRecord {
            event_id: summary.event_id(),
            thread_id: summary.thread_id,
            job_id: summary.job_id.clone(),
            label: summary.label.clone(),
            kind: LOOP_NOTIFICATION_KIND_SUMMARY,
            summary_json: summary.to_persisted_json()?,
            created_at_ms: summary.finished_at_ms,
        })
        .await?;
    if !first {
        return Ok(false);
    }
    if summary.pending_needed() {
        state_runtime
            .record_loop_notification(LoopNotificationRecord {
                event_id: format!("pending:{}", summary.event_id()),
                thread_id: summary.thread_id,
                job_id: summary.job_id.clone(),
                label: summary.label.clone(),
                kind: LOOP_NOTIFICATION_KIND_PENDING,
                summary_json: summary.to_persisted_json()?,
                created_at_ms: summary.finished_at_ms,
            })
            .await?;
    }
    enqueue_or_drop(LOOP_SUMMARY_TX.get(), summary);
    Ok(true)
}

/// Queue is transport only: full or absent, the summary is dropped here and
/// the tick never learns about it (the pending row stays persisted).
fn enqueue_or_drop(tx: Option<&mpsc::Sender<LoopTickSummary>>, summary: &LoopTickSummary) {
    if !enqueue_or_drop_result(tx, summary) {
        tracing::warn!(
            target: "codex_vl::loop_summary",
            label = %summary.label,
            "loop summary queue full; emission dropped, pending row stays persisted"
        );
    }
}

/// Testable core of the enqueue: `true` = queued, `false` = dropped
/// (absent sender or full bounded queue).
fn enqueue_or_drop_result(
    tx: Option<&mpsc::Sender<LoopTickSummary>>,
    summary: &LoopTickSummary,
) -> bool {
    match tx {
        Some(tx) => tx.try_send(summary.clone()).is_ok(),
        None => false,
    }
}

/// Derived from the persisted post-tick row: the persisted state is the
/// single source of truth for the summary (no caller-side interpretation).
pub(crate) fn derive_outcome(
    job: &codex_state::ThreadLoopJob,
    schedule_kind: &str,
    occurrence_claimed: bool,
) -> DerivedOutcome {
    if job.last_error.is_some() {
        return DerivedOutcome::Failed;
    }
    if job.pending_tick {
        return DerivedOutcome::SkippedBusy;
    }
    if schedule_kind == "one_shot" && job.next_run_ms.is_none() && !occurrence_claimed {
        return DerivedOutcome::OneShotExpired;
    }
    DerivedOutcome::Ok
}

/// T6 — a dispatched runner/vivling tick is not finished in-process: its
/// summary lands at `handle_loop_tick_finished` instead. `true` = skip the
/// post-submission summary to avoid a double record.
pub(crate) fn is_post_dispatch(last_status: Option<&str>) -> bool {
    matches!(
        last_status,
        Some("runner_dispatched") | Some("delegated_vivling")
    )
}

/// Manager resolution for the summary line, from the persisted delegation
/// (same source T1 reads; the summary never re-derives ownership).
fn manager_of(delegation: Option<&codex_state::LoopDelegation>) -> (LoopManager, &'static str) {
    match delegation {
        Some(delegation) if delegation.override_main => (LoopManager::Main, "hard_main_override"),
        Some(_) => (LoopManager::Vivling, "delegated"),
        None => (LoopManager::Main, "not_delegated"),
    }
}

/// Build the typed summary from the persisted post-tick row.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_tick_summary(
    job_after: &codex_state::ThreadLoopJob,
    descriptor: Option<&codex_state::LoopDescriptor>,
    delegation: Option<&codex_state::LoopDelegation>,
    occurrence_ms: Option<i64>,
    occurrence_claimed: bool,
    started_ms: i64,
    finished_at_ms: i64,
) -> LoopTickSummary {
    let schedule_kind = descriptor
        .as_ref()
        .map(|descriptor| LoopScheduleKind::from_descriptor(&descriptor.schedule_kind))
        .unwrap_or(LoopScheduleKind::Interval);
    let outcome = match derive_outcome(job_after, schedule_kind.render(), occurrence_claimed) {
        DerivedOutcome::Ok => LoopTickOutcome::Ok,
        DerivedOutcome::Failed => LoopTickOutcome::Failed,
        DerivedOutcome::SkippedBusy => LoopTickOutcome::SkippedBusy,
        DerivedOutcome::OneShotExpired => LoopTickOutcome::OneShotExpired,
    };
    let next_run = match job_after.next_run_ms {
        Some(ms) => NextRun::At(ms),
        None => NextRun::Terminal("tick terminal or disarmed".to_string()),
    };
    let (manager, manager_reason) = manager_of(delegation);
    LoopTickSummary {
        thread_id: job_after.thread_id,
        job_id: job_after.id.clone(),
        label: job_after.label.clone(),
        schedule_kind,
        outcome,
        duration_ms: finished_at_ms.saturating_sub(started_ms),
        next_run,
        runner: descriptor
            .as_ref()
            .map(|descriptor| descriptor.runner_kind)
            .unwrap_or(codex_state::LoopRunnerKind::Main),
        runner_reason: format!(
            "runner={}",
            descriptor
                .as_ref()
                .map(|descriptor| descriptor.runner_kind.as_str())
                .unwrap_or("main")
        ),
        manager,
        manager_reason: manager_reason.to_string(),
        suspend_reason: delegation.and_then(|delegation| delegation.suspend_reason.clone()),
        occurrence_ms,
        finished_at_ms,
    }
}

/// One-shot helper for the tick seams: persist the summary (and its pending
/// when R3 admits it), then enqueue. Returns whether the summary is the
/// first persisted instance (and only then the caller emits the event).
pub(crate) async fn persist_and_queue_tick(
    state_runtime: &codex_state::StateRuntime,
    thread_id: codex_protocol::ThreadId,
    job_id: &str,
    occurrence_ms: Option<i64>,
    occurrence_claimed: bool,
    started_ms: i64,
) -> anyhow::Result<Option<LoopTickSummary>> {
    let Some(job_after) = state_runtime
        .get_thread_loop_job_by_id(thread_id, job_id)
        .await?
    else {
        return Ok(None);
    };
    let descriptor = state_runtime.get_loop_descriptor(job_id).await?;
    let delegation = state_runtime.get_loop_delegation(thread_id, job_id).await?;
    let summary = build_tick_summary(
        &job_after,
        descriptor.as_ref(),
        delegation.as_ref(),
        occurrence_ms,
        occurrence_claimed,
        started_ms,
        loop_now_ms(),
    );
    let first = persist_and_queue(state_runtime, &summary).await?;
    Ok(first.then_some(summary))
}

/// R11 gate 3 — the separate consumer: replays undelivered pending rows once
/// per start, then drains the queue. Delivery happens only through the
/// (optional) external channel; absent channel = rows stay pending.
pub(crate) async fn run_loop_summary_worker(
    state_runtime: std::sync::Arc<codex_state::StateRuntime>,
    mut rx: mpsc::Receiver<LoopTickSummary>,
    channel: LoopNotifyChannel,
) {
    replay_pending(&state_runtime, &channel).await;
    while let Some(summary) = rx.recv().await {
        if summary.pending_needed() {
            deliver_once(
                &state_runtime,
                &format!("pending:{}", summary.event_id()),
                &summary.label,
                &channel,
            )
            .await;
        }
    }
}

async fn replay_pending(state_runtime: &codex_state::StateRuntime, channel: &LoopNotifyChannel) {
    match state_runtime.list_pending_loop_notifications().await {
        Ok(rows) => {
            for row in rows {
                deliver_once(state_runtime, &row.event_id, &row.label, channel).await;
            }
        }
        Err(err) => tracing::warn!(
            target: "codex_vl::loop_summary",
            error = %err,
            "pending replay skipped; rows stay persisted for the next bootstrap"
        ),
    }
}

async fn deliver_once(
    state_runtime: &codex_state::StateRuntime,
    event_id: &str,
    label: &str,
    channel: &LoopNotifyChannel,
) {
    match channel {
        LoopNotifyChannel::Absent => tracing::debug!(
            target: "codex_vl::loop_summary",
            label = %label,
            "notify channel absent; pending row stays persisted (normal state)"
        ),
    }
    let _ = event_id; // consumed by the real channel variants (m2-final wiring)
}

/// Starts (once per process) the bounded queue and the separate consumer
/// task; returns the tick-side sender. Called lazily from the tick seam —
/// tests that never call it keep a no-op tick path.
pub(crate) fn ensure_worker_started(
    state_runtime: &std::sync::Arc<codex_state::StateRuntime>,
) -> Option<mpsc::Sender<LoopTickSummary>> {
    let tx = LOOP_SUMMARY_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<LoopTickSummary>(LOOP_SUMMARY_QUEUE_CAPACITY);
        tokio::spawn(run_loop_summary_worker(
            std::sync::Arc::clone(state_runtime),
            rx,
            LoopNotifyChannel::from_config(),
        ));
        tx
    });
    tx.clone().into()
}

#[cfg(test)]
mod tests {
    use super::DerivedOutcome;
    use super::LOOP_SUMMARY_QUEUE_CAPACITY;
    use super::LoopManager;
    use super::LoopScheduleKind;
    use super::LoopTickOutcome;
    use super::LoopTickSummary;
    use super::NextRun;
    use super::derive_outcome;
    use super::enqueue_or_drop;
    use super::enqueue_or_drop_result;
    use codex_protocol::ThreadId;
    use tokio::sync::mpsc;

    fn job(
        pending_tick: bool,
        next_run_ms: Option<i64>,
        last_error: Option<&str>,
    ) -> codex_state::ThreadLoopJob {
        codex_state::ThreadLoopJob {
            id: "job-x".to_string(),
            thread_id: ThreadId::new(),
            label: "x".to_string(),
            prompt_text: "tick".to_string(),
            goal_text: None,
            interval_seconds: 60,
            enabled: true,
            run_policy: "queue_one".to_string(),
            auto_remove_on_completion: true,
            created_by: "agent".to_string(),
            next_run_ms,
            last_run_ms: None,
            last_status: None,
            last_error: last_error.map(str::to_string),
            pending_tick,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    // Persisted state is the source of truth for the derived outcome.
    #[test]
    fn outcome_is_derived_from_persisted_row() {
        assert_eq!(
            derive_outcome(&job(false, Some(60), None), "interval", true),
            DerivedOutcome::Ok
        );
        assert_eq!(
            derive_outcome(&job(false, None, Some("boom")), "interval", true),
            DerivedOutcome::Failed
        );
        assert_eq!(
            derive_outcome(&job(true, None, None), "interval", true),
            DerivedOutcome::SkippedBusy
        );
        assert_eq!(
            derive_outcome(&job(false, None, None), "one_shot", false),
            DerivedOutcome::OneShotExpired
        );
    }

    // R11 gate 2 — a full bounded queue drops the emission (Err) without
    // panicking; the tick path ignores the result either way.
    #[tokio::test]
    async fn full_queue_drops_the_emission_without_panicking() {
        let (tx, mut rx) = mpsc::channel::<LoopTickSummary>(LOOP_SUMMARY_QUEUE_CAPACITY.min(1));
        let summary_slot = summary_for_queue();
        assert!(enqueue_or_drop_result(Some(&tx), &summary_slot));
        assert!(
            !enqueue_or_drop_result(Some(&tx), &summary_slot),
            "a full bounded queue must drop the emission"
        );
        assert!(rx.try_recv().is_ok(), "first emission stays queued");
        assert!(rx.try_recv().is_err(), "the dropped one must not be queued");
    }

    #[test]
    fn absent_sender_is_a_no_op() {
        assert!(!enqueue_or_drop_result(None, &summary_for_queue()));
    }

    fn summary_for_queue() -> LoopTickSummary {
        LoopTickSummary {
            thread_id: ThreadId::new(),
            job_id: "job-q".to_string(),
            label: "q".to_string(),
            schedule_kind: LoopScheduleKind::Interval,
            outcome: LoopTickOutcome::Ok,
            duration_ms: 5,
            next_run: NextRun::At(1),
            runner: codex_state::LoopRunnerKind::Main,
            runner_reason: "runner=main".to_string(),
            manager: LoopManager::Main,
            manager_reason: "not_delegated".to_string(),
            suspend_reason: None,
            occurrence_ms: Some(1),
            finished_at_ms: 2,
        }
    }
}
