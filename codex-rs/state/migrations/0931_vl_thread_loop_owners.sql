CREATE TABLE IF NOT EXISTS vl_thread_loop_owners (
    thread_id TEXT PRIMARY KEY NOT NULL,
    owner_kind TEXT NOT NULL,
    owner_vivling_id TEXT,
    updated_at_ms INTEGER NOT NULL
);
