-- persisted loop tick summaries and notification pending rows.
-- Guarantees: persist-before-emit (rows land before any emit) and dedup on
-- the persisted event_id (INSERT OR IGNORE verdict). `pending` rows replay
-- at bootstrap by the notification consumer (m2); retention is bounded there.
-- job_id carries NO foreign key BY DESIGN: this table is an audit trail of
-- what was summarized/notified, deliberately decoupled from the job
-- lifetime — rows outlive a removed job, and retention is the notification
-- consumer's job, not the schema's.
CREATE TABLE IF NOT EXISTS vl_loop_notifications (
    event_id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL,
    job_id TEXT NOT NULL,
    label TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('summary', 'pending')),
    summary_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);
