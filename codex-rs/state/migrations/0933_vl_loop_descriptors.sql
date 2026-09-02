-- Loop-owned runner and scheduling descriptor. 0934 extends this descriptor
-- with occurrence accounting; keep job_id stable so child rows remain valid.
CREATE TABLE IF NOT EXISTS vl_loop_descriptors (
    job_id TEXT PRIMARY KEY NOT NULL
        REFERENCES vl_thread_loop_jobs(id) ON DELETE CASCADE,
    runner_kind TEXT NOT NULL DEFAULT 'main'
        CHECK (runner_kind IN ('main', 'child_agent')),
    runner_model TEXT,
    runner_reasoning_effort TEXT,
    tz TEXT,
    schedule_kind TEXT NOT NULL DEFAULT 'interval'
        CHECK (schedule_kind IN ('interval', 'at', 'one_shot')),
    schedule_at TEXT,
    one_shot_at_ms INTEGER,
    rearm_on_boot INTEGER NOT NULL DEFAULT 0,
    in_flight INTEGER NOT NULL DEFAULT 0,
    updated_at_ms INTEGER NOT NULL
);

INSERT INTO vl_loop_descriptors (job_id, updated_at_ms)
SELECT id, updated_at_ms
FROM vl_thread_loop_jobs
WHERE NOT EXISTS (
    SELECT 1 FROM vl_loop_descriptors AS existing
    WHERE existing.job_id = vl_thread_loop_jobs.id
);
