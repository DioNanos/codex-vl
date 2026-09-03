use codex_protocol::ThreadId;

/// Kind values for [`LoopNotificationRecord`] (closed pair, CHECK-enforced
/// in migration 0936): `"summary"` lands on every finished tick, `"pending"`
/// only on anomalous outcomes and one-shot ticks.
pub const LOOP_NOTIFICATION_KIND_SUMMARY: &str = "summary";
pub const LOOP_NOTIFICATION_KIND_PENDING: &str = "pending";

/// retention for summary rows: the last N per job (the same
/// window as `recent_results`), enforced in the insert transaction.
pub const LOOP_NOTIFICATION_SUMMARY_RETENTION: i64 = 20;
/// retention for pending rows: older than this age (ms) are
/// dropped in the insert transaction (canal-less runs do not accumulate).
pub const LOOP_NOTIFICATION_PENDING_MAX_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// A row of `vl_loop_notifications` (0936): the persisted-before-emit record
/// of one loop tick summary. `event_id` is the dedup key;
/// `summary_json` is the fixed-format summary serialized by the TUI builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopNotificationRecord {
    pub event_id: String,
    pub thread_id: ThreadId,
    pub job_id: String,
    pub label: String,
    pub kind: &'static str,
    pub summary_json: String,
    pub created_at_ms: i64,
}
