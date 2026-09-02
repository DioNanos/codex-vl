use codex_protocol::ThreadId;

/// Kind values for [`LoopNotificationRecord`] (closed pair, CHECK-enforced
/// in migration 0936): `"summary"` lands on every finished tick, `"pending"`
/// only on the events R3 admits (anomalous outcomes and one-shot ticks).
pub const LOOP_NOTIFICATION_KIND_SUMMARY: &str = "summary";
pub const LOOP_NOTIFICATION_KIND_PENDING: &str = "pending";

/// A row of `vl_loop_notifications` (0936): the persisted-before-emit record
/// of one loop tick summary (R11 gates 1 and 6). `event_id` is the dedup key;
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
