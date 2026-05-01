CREATE TABLE IF NOT EXISTS vl_thread_loop_jobs (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL,
    label TEXT NOT NULL,
    goal_text TEXT,
    prompt_text TEXT NOT NULL,
    interval_seconds INTEGER NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    run_policy TEXT NOT NULL DEFAULT 'queue_one',
    auto_remove_on_completion INTEGER NOT NULL DEFAULT 1,
    created_by TEXT NOT NULL DEFAULT 'user',
    next_run_ms INTEGER,
    last_run_ms INTEGER,
    last_status TEXT,
    last_error TEXT,
    pending_tick INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    UNIQUE(thread_id, label)
);

CREATE INDEX IF NOT EXISTS idx_vl_thread_loop_jobs_thread_id
ON vl_thread_loop_jobs(thread_id);
