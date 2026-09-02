CREATE TABLE IF NOT EXISTS vl_loop_delegations (
    thread_id TEXT NOT NULL,
    job_id TEXT NOT NULL,
    loop_label TEXT NOT NULL,
    vivling_id TEXT NOT NULL,
    strategy TEXT NOT NULL DEFAULT 'observe',
    ticks_managed INTEGER NOT NULL DEFAULT 0,
    recent_results_json TEXT NOT NULL DEFAULT '[]',
    last_plan_approved INTEGER,
    override_main INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY(thread_id, job_id)
);

CREATE INDEX IF NOT EXISTS idx_vl_loop_delegations_thread_id
ON vl_loop_delegations(thread_id);
