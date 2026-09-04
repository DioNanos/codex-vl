-- Occurrence accounting for loop ticks. One row per (job_id,
-- scheduled_at_ms): the claim is the INSERT itself, so a second timer for
-- the same occurrence finds the row already present (rows_affected = 0) and
-- skips — an occurrence is dispatched at most once. Keep job_id stable so
-- child rows remain valid.
CREATE TABLE IF NOT EXISTS vl_loop_occurrences (
    job_id TEXT NOT NULL
        REFERENCES vl_thread_loop_jobs(id) ON DELETE CASCADE,
    scheduled_at_ms INTEGER NOT NULL,
    fired_count INTEGER NOT NULL DEFAULT 0,
    last_fired_at_ms INTEGER,
    claimed_at_ms INTEGER NOT NULL,
    PRIMARY KEY (job_id, scheduled_at_ms)
);

CREATE INDEX IF NOT EXISTS idx_vl_loop_occurrences_job
ON vl_loop_occurrences(job_id);
