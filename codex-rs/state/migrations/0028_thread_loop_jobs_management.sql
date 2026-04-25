ALTER TABLE thread_loop_jobs
ADD COLUMN goal_text TEXT;

ALTER TABLE thread_loop_jobs
ADD COLUMN auto_remove_on_completion INTEGER NOT NULL DEFAULT 1;

ALTER TABLE thread_loop_jobs
ADD COLUMN created_by TEXT NOT NULL DEFAULT 'user';
