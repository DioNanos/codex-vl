-- codex-vl: namespace loop tables under a vl_ prefix so upstream cannot
-- collide with our custom schema. Runs after all upstream migrations
-- (00xx range) and after our additive 0027/0028/0030 files.
ALTER TABLE thread_loop_jobs RENAME TO vl_thread_loop_jobs;
ALTER TABLE thread_loop_owners RENAME TO vl_thread_loop_owners;

DROP INDEX IF EXISTS idx_thread_loop_jobs_thread_id;
CREATE INDEX IF NOT EXISTS idx_vl_thread_loop_jobs_thread_id
ON vl_thread_loop_jobs(thread_id);
