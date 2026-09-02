-- T6 m1: persisted loop tick summaries and notification pending rows.
-- R11 gates: persist-before-emit (rows land before any emit) and dedup on
-- the persisted event_id (INSERT OR IGNORE verdict). `pending` rows replay
-- at bootstrap by the notification consumer (m2); retention is bounded there.
CREATE TABLE IF NOT EXISTS vl_loop_notifications (
    event_id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL,
    job_id TEXT NOT NULL,
    label TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('summary', 'pending')),
    summary_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);
