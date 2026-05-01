use crate::AgentJob;
use crate::AgentJobCreateParams;
use crate::AgentJobItem;
use crate::AgentJobItemCreateParams;
use crate::AgentJobItemStatus;
use crate::AgentJobProgress;
use crate::AgentJobStatus;
use crate::LOGS_DB_FILENAME;
use crate::LOGS_DB_VERSION;
use crate::LogEntry;
use crate::LogQuery;
use crate::LogRow;
use crate::STATE_DB_FILENAME;
use crate::STATE_DB_VERSION;
use crate::SortKey;
use crate::ThreadMetadata;
use crate::ThreadMetadataBuilder;
use crate::ThreadsPage;
use crate::apply_rollout_item;
use crate::migrations::runtime_logs_migrator;
use crate::migrations::runtime_state_migrator;
use crate::model::AgentJobRow;
use crate::model::ThreadGoalRow;
use crate::model::ThreadRow;
use crate::model::anchor_from_item;
use crate::model::datetime_to_epoch_millis;
use crate::model::datetime_to_epoch_seconds;
use crate::model::epoch_millis_to_datetime;
use crate::paths::file_modified_time_utc;
use chrono::DateTime;
use chrono::Utc;
use codex_protocol::ThreadId;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::protocol::RolloutItem;
use log::LevelFilter;
use serde_json::Value;
use sqlx::ConnectOptions;
use sqlx::QueryBuilder;
use sqlx::Row;
use sqlx::Sqlite;
use sqlx::SqliteConnection;
use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use sqlx::sqlite::SqliteAutoVacuum;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqliteJournalMode;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::sqlite::SqliteSynchronous;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::time::Duration;
use tracing::warn;

mod agent_jobs;
mod backfill;
mod device_key;
#[cfg(test)]
mod device_key_tests;
mod goals;
mod logs;
mod memories;
mod remote_control;
#[cfg(test)]
mod test_support;
mod thread_loop_jobs;
mod threads;

pub use device_key::DeviceKeyBindingRecord;
pub use goals::ThreadGoalAccountingMode;
pub use goals::ThreadGoalAccountingOutcome;
pub use goals::ThreadGoalUpdate;
pub use remote_control::RemoteControlEnrollmentRecord;
pub use threads::ThreadFilterOptions;

// "Partition" is the retained-log-content bucket we cap at 10 MiB:
// - one bucket per non-null thread_id
// - one bucket per threadless (thread_id IS NULL) non-null process_uuid
// - one bucket for threadless rows with process_uuid IS NULL
// This budget tracks each row's persisted rendered log body plus non-body
// metadata, rather than the exact sum of all persisted SQLite column bytes.
const LOG_PARTITION_SIZE_LIMIT_BYTES: i64 = 10 * 1024 * 1024;
const LOG_PARTITION_ROW_LIMIT: i64 = 1_000;

#[derive(Clone)]
pub struct StateRuntime {
    codex_home: PathBuf,
    default_provider: String,
    pool: Arc<sqlx::SqlitePool>,
    logs_pool: Arc<sqlx::SqlitePool>,
    thread_updated_at_millis: Arc<AtomicI64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SqliteRuntimeMode {
    journal_mode: SqliteJournalMode,
    max_connections: u32,
}

impl SqliteRuntimeMode {
    fn default() -> Self {
        Self {
            journal_mode: SqliteJournalMode::Wal,
            max_connections: 5,
        }
    }

    fn android_compat() -> Self {
        Self {
            journal_mode: SqliteJournalMode::Delete,
            max_connections: 1,
        }
    }
}

impl StateRuntime {
    /// Initialize the state runtime using the provided Codex home and default provider.
    ///
    /// This opens (and migrates) the SQLite databases under `codex_home`,
    /// keeping logs in a dedicated file to reduce lock contention with the
    /// rest of the state store.
    pub async fn init(codex_home: PathBuf, default_provider: String) -> anyhow::Result<Arc<Self>> {
        tokio::fs::create_dir_all(&codex_home).await?;
        let state_migrator = runtime_state_migrator();
        let logs_migrator = runtime_logs_migrator();
        let current_state_name = state_db_filename();
        let current_logs_name = logs_db_filename();
        remove_legacy_db_files(
            &codex_home,
            current_state_name.as_str(),
            STATE_DB_FILENAME,
            "state",
        )
        .await;
        remove_legacy_db_files(
            &codex_home,
            current_logs_name.as_str(),
            LOGS_DB_FILENAME,
            "logs",
        )
        .await;
        let state_path = state_db_path(codex_home.as_path());
        let logs_path = logs_db_path(codex_home.as_path());
        let pool = match open_state_sqlite(
            &state_path,
            &state_migrator,
            SqliteRuntimeMode::default(),
        )
        .await
        {
            Ok(db) => Arc::new(db),
            Err(err) => {
                if cfg!(target_os = "android") {
                    warn!(
                        "failed to open state db at {} with default SQLite mode; retrying in Android compatibility mode: {err}",
                        state_path.display()
                    );
                    match open_state_sqlite(
                        &state_path,
                        &state_migrator,
                        SqliteRuntimeMode::android_compat(),
                    )
                    .await
                    {
                        Ok(db) => Arc::new(db),
                        Err(retry_err) => {
                            warn!(
                                "failed to open state db at {} in Android compatibility mode: {retry_err}",
                                state_path.display()
                            );
                            return Err(retry_err);
                        }
                    }
                } else {
                    warn!("failed to open state db at {}: {err}", state_path.display());
                    return Err(err);
                }
            }
        };
        let logs_pool = match open_logs_sqlite(&logs_path, &logs_migrator).await {
            Ok(db) => Arc::new(db),
            Err(err) => {
                warn!("failed to open logs db at {}: {err}", logs_path.display());
                return Err(err);
            }
        };
        let thread_updated_at_millis: Option<i64> =
            sqlx::query_scalar("SELECT MAX(threads.updated_at_ms) FROM threads")
                .fetch_one(pool.as_ref())
                .await?;
        let thread_updated_at_millis = thread_updated_at_millis.unwrap_or(0);
        let runtime = Arc::new(Self {
            pool,
            logs_pool,
            codex_home,
            default_provider,
            thread_updated_at_millis: Arc::new(AtomicI64::new(thread_updated_at_millis)),
        });
        if let Err(err) = runtime.run_logs_startup_maintenance().await {
            warn!(
                "failed to run startup maintenance for logs db at {}: {err}",
                logs_path.display(),
            );
        }
        Ok(runtime)
    }

    /// Return the configured Codex home directory for this runtime.
    pub fn codex_home(&self) -> &Path {
        self.codex_home.as_path()
    }
}

fn base_sqlite_options(path: &Path, mode: SqliteRuntimeMode) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(mode.journal_mode)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .log_statements(LevelFilter::Off)
}

async fn open_state_sqlite(
    path: &Path,
    migrator: &Migrator,
    mode: SqliteRuntimeMode,
) -> anyhow::Result<SqlitePool> {
    let options = base_sqlite_options(path, mode).auto_vacuum(SqliteAutoVacuum::Incremental);
    let pool = SqlitePoolOptions::new()
        .max_connections(mode.max_connections)
        .connect_with(options)
        .await?;
    reconcile_vl_legacy_state_migrations(&pool, migrator).await?;
    migrator.run(&pool).await?;
    let auto_vacuum = sqlx::query_scalar::<_, i64>("PRAGMA auto_vacuum")
        .fetch_one(&pool)
        .await?;
    if auto_vacuum != SqliteAutoVacuum::Incremental as i64 {
        // Existing state DBs need one non-transactional `VACUUM` before
        // SQLite persists `auto_vacuum = INCREMENTAL` in the database header.
        sqlx::query("PRAGMA auto_vacuum = INCREMENTAL")
            .execute(&pool)
            .await?;
        // We do it on best effort. If the lock can't be acquired, it will be done at next run.
        let _ = sqlx::query("VACUUM").execute(&pool).await;
    }
    // We do it on best effort. If the lock can't be acquired, it will be done at next run.
    let _ = sqlx::query("PRAGMA incremental_vacuum")
        .execute(&pool)
        .await;
    Ok(pool)
}

async fn reconcile_vl_legacy_state_migrations(
    pool: &SqlitePool,
    migrator: &Migrator,
) -> anyhow::Result<()> {
    if !sqlite_object_exists(pool, "table", "_sqlx_migrations").await? {
        return Ok(());
    }

    reconcile_legacy_loop_tables(pool).await?;

    delete_applied_migration_if_description(pool, 27, "thread loop jobs").await?;
    delete_applied_migration_if_description(pool, 28, "thread loop jobs management").await?;
    delete_applied_migration_if_description(pool, 29, "threads cwd sort indexes").await?;
    delete_applied_migration_if_description(pool, 30, "thread loop owners").await?;

    if sqlite_object_exists(pool, "index", "idx_threads_archived_cwd_created_at_ms").await?
        && sqlite_object_exists(pool, "index", "idx_threads_archived_cwd_updated_at_ms").await?
    {
        mark_embedded_migration_applied(pool, migrator, 27).await?;
    }
    if sqlite_object_exists(pool, "table", "device_key_bindings").await? {
        mark_embedded_migration_applied(pool, migrator, 28).await?;
    }
    if sqlite_object_exists(pool, "table", "thread_goals").await? {
        mark_embedded_migration_applied(pool, migrator, 29).await?;
    }

    Ok(())
}

async fn reconcile_legacy_loop_tables(pool: &SqlitePool) -> anyhow::Result<()> {
    if sqlite_object_exists(pool, "table", "thread_loop_jobs").await? {
        if !sqlite_object_exists(pool, "table", "vl_thread_loop_jobs").await? {
            sqlx::query("ALTER TABLE thread_loop_jobs RENAME TO vl_thread_loop_jobs")
                .execute(pool)
                .await?;
        }
    }
    if sqlite_object_exists(pool, "table", "vl_thread_loop_jobs").await? {
        add_column_if_missing(pool, "vl_thread_loop_jobs", "goal_text", "TEXT").await?;
        add_column_if_missing(
            pool,
            "vl_thread_loop_jobs",
            "auto_remove_on_completion",
            "INTEGER NOT NULL DEFAULT 1",
        )
        .await?;
        add_column_if_missing(
            pool,
            "vl_thread_loop_jobs",
            "created_by",
            "TEXT NOT NULL DEFAULT 'user'",
        )
        .await?;
        sqlx::query("DROP INDEX IF EXISTS idx_thread_loop_jobs_thread_id")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_vl_thread_loop_jobs_thread_id ON vl_thread_loop_jobs(thread_id)",
        )
        .execute(pool)
        .await?;
    }

    if sqlite_object_exists(pool, "table", "thread_loop_owners").await?
        && !sqlite_object_exists(pool, "table", "vl_thread_loop_owners").await?
    {
        sqlx::query("ALTER TABLE thread_loop_owners RENAME TO vl_thread_loop_owners")
            .execute(pool)
            .await?;
    }

    Ok(())
}

async fn add_column_if_missing(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    declaration: &str,
) -> anyhow::Result<()> {
    if table_column_exists(pool, table, column).await? {
        return Ok(());
    }
    let query = format!("ALTER TABLE {table} ADD COLUMN {column} {declaration}");
    sqlx::query(&query).execute(pool).await?;
    Ok(())
}

async fn table_column_exists(pool: &SqlitePool, table: &str, column: &str) -> anyhow::Result<bool> {
    let query = format!("PRAGMA table_info({table})");
    let rows = sqlx::query(&query).fetch_all(pool).await?;
    Ok(rows.iter().any(|row| {
        row.try_get::<String, _>("name")
            .is_ok_and(|name| name == column)
    }))
}

async fn sqlite_object_exists(
    pool: &SqlitePool,
    object_type: &str,
    name: &str,
) -> anyhow::Result<bool> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = ? AND name = ?")
            .bind(object_type)
            .bind(name)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

async fn delete_applied_migration_if_description(
    pool: &SqlitePool,
    version: i64,
    description: &str,
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = ? AND description = ?")
        .bind(version)
        .bind(description)
        .execute(pool)
        .await?;
    Ok(())
}

async fn mark_embedded_migration_applied(
    pool: &SqlitePool,
    migrator: &Migrator,
    version: i64,
) -> anyhow::Result<()> {
    let Some(migration) = migrator
        .migrations
        .iter()
        .find(|migration| migration.version == version)
    else {
        return Ok(());
    };
    sqlx::query(
        "INSERT OR IGNORE INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(migration.version)
    .bind(migration.description.as_ref())
    .bind(true)
    .bind(migration.checksum.as_ref().to_vec())
    .bind(0_i64)
    .execute(pool)
    .await?;
    Ok(())
}

async fn open_logs_sqlite(path: &Path, migrator: &Migrator) -> anyhow::Result<SqlitePool> {
    let options = base_sqlite_options(path, SqliteRuntimeMode::default())
        .auto_vacuum(SqliteAutoVacuum::Incremental);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    migrator.run(&pool).await?;
    Ok(pool)
}

fn db_filename(base_name: &str, version: u32) -> String {
    format!("{base_name}_{version}.sqlite")
}

pub fn state_db_filename() -> String {
    db_filename(STATE_DB_FILENAME, STATE_DB_VERSION)
}

pub fn state_db_path(codex_home: &Path) -> PathBuf {
    codex_home.join(state_db_filename())
}

pub fn logs_db_filename() -> String {
    db_filename(LOGS_DB_FILENAME, LOGS_DB_VERSION)
}

pub fn logs_db_path(codex_home: &Path) -> PathBuf {
    codex_home.join(logs_db_filename())
}

async fn remove_legacy_db_files(
    codex_home: &Path,
    current_name: &str,
    base_name: &str,
    db_label: &str,
) {
    let mut entries = match tokio::fs::read_dir(codex_home).await {
        Ok(entries) => entries,
        Err(err) => {
            warn!(
                "failed to read codex_home for {db_label} db cleanup {}: {err}",
                codex_home.display(),
            );
            return;
        }
    };
    let mut legacy_paths = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if !entry
            .file_type()
            .await
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !should_remove_db_file(file_name.as_ref(), current_name, base_name) {
            continue;
        }

        legacy_paths.push(entry.path());
    }

    // On Windows, SQLite can keep the main database file undeletable until the
    // matching `-wal` / `-shm` sidecars are removed. Remove the longest
    // sidecar-style paths first so the main file is attempted last.
    legacy_paths.sort_by_key(|path| std::cmp::Reverse(path.as_os_str().len()));
    for legacy_path in legacy_paths {
        let mut result = tokio::fs::remove_file(&legacy_path).await;
        for _ in 0..3 {
            if result.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
            result = tokio::fs::remove_file(&legacy_path).await;
        }
        if let Err(err) = result {
            warn!(
                "failed to remove legacy {db_label} db file {}: {err}",
                legacy_path.display(),
            );
        }
    }
}

fn should_remove_db_file(file_name: &str, current_name: &str, base_name: &str) -> bool {
    let mut normalized_name = file_name;
    for suffix in ["-wal", "-shm", "-journal"] {
        if let Some(stripped) = file_name.strip_suffix(suffix) {
            normalized_name = stripped;
            break;
        }
    }
    if normalized_name == current_name {
        return false;
    }
    let unversioned_name = format!("{base_name}.sqlite");
    if normalized_name == unversioned_name {
        return true;
    }

    let Some(version_with_extension) = normalized_name.strip_prefix(&format!("{base_name}_"))
    else {
        return false;
    };
    let Some(version_suffix) = version_with_extension.strip_suffix(".sqlite") else {
        return false;
    };
    !version_suffix.is_empty() && version_suffix.chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::SqliteJournalMode;
    use super::SqliteRuntimeMode;
    use super::open_state_sqlite;
    use super::runtime_state_migrator;
    use super::state_db_path;
    use super::test_support::unique_temp_dir;
    use crate::migrations::STATE_MIGRATOR;
    use sqlx::SqlitePool;
    use sqlx::migrate::MigrateError;
    use sqlx::migrate::Migrator;
    use sqlx::sqlite::SqliteConnectOptions;
    use std::borrow::Cow;
    use std::path::Path;

    async fn open_db_pool(path: &Path) -> SqlitePool {
        SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(false),
        )
        .await
        .expect("open sqlite pool")
    }

    #[tokio::test]
    async fn open_state_sqlite_tolerates_newer_applied_migrations() {
        let codex_home = unique_temp_dir();
        tokio::fs::create_dir_all(&codex_home)
            .await
            .expect("create codex home");
        let state_path = state_db_path(codex_home.as_path());
        let pool = SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(&state_path)
                .create_if_missing(true),
        )
        .await
        .expect("open state db");
        STATE_MIGRATOR
            .run(&pool)
            .await
            .expect("apply current state schema");
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(9_999_i64)
        .bind("future migration")
        .bind(true)
        .bind(vec![1_u8, 2, 3, 4])
        .bind(1_i64)
        .execute(&pool)
        .await
        .expect("insert future migration record");
        pool.close().await;

        let strict_pool = open_db_pool(state_path.as_path()).await;
        let strict_err = STATE_MIGRATOR
            .run(&strict_pool)
            .await
            .expect_err("strict migrator should reject newer applied migrations");
        assert!(matches!(strict_err, MigrateError::VersionMissing(9_999)));
        strict_pool.close().await;

        let tolerant_migrator = runtime_state_migrator();
        let tolerant_pool = open_state_sqlite(
            state_path.as_path(),
            &tolerant_migrator,
            SqliteRuntimeMode::default(),
        )
        .await
        .expect("runtime migrator should tolerate newer applied migrations");
        tolerant_pool.close().await;

        let _ = tokio::fs::remove_dir_all(codex_home).await;
    }

    #[tokio::test]
    async fn open_state_sqlite_migrates_existing_vl_db_after_device_key_renumber() {
        let codex_home = unique_temp_dir();
        tokio::fs::create_dir_all(&codex_home)
            .await
            .expect("create codex home");
        let state_path = state_db_path(codex_home.as_path());
        let pool = SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(&state_path)
                .create_if_missing(true),
        )
        .await
        .expect("open state db");

        let legacy_migrator = Migrator {
            migrations: Cow::Owned(
                STATE_MIGRATOR
                    .migrations
                    .iter()
                    .filter(|migration| migration.description != "device_key_bindings")
                    .cloned()
                    .collect(),
            ),
            ignore_missing: false,
            locking: STATE_MIGRATOR.locking,
            no_tx: STATE_MIGRATOR.no_tx,
        };
        legacy_migrator
            .run(&pool)
            .await
            .expect("apply pre-renumber codex-vl schema");
        pool.close().await;

        let tolerant_migrator = runtime_state_migrator();
        let tolerant_pool = open_state_sqlite(
            state_path.as_path(),
            &tolerant_migrator,
            SqliteRuntimeMode::default(),
        )
        .await
        .expect("runtime migrator should add the renumbered device key schema");

        let device_key_table: (String,) = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'device_key_bindings'",
        )
        .fetch_one(&tolerant_pool)
        .await
        .expect("device_key_bindings table exists");
        assert_eq!(device_key_table.0, "device_key_bindings");
        tolerant_pool.close().await;

        let _ = tokio::fs::remove_dir_all(codex_home).await;
    }

    #[tokio::test]
    async fn open_state_sqlite_reconciles_legacy_vl_loop_migration_numbers() {
        let codex_home = unique_temp_dir();
        tokio::fs::create_dir_all(&codex_home)
            .await
            .expect("create codex home");
        let state_path = state_db_path(codex_home.as_path());
        let pool = SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(&state_path)
                .create_if_missing(true),
        )
        .await
        .expect("open state db");

        let pre_conflict_migrator = Migrator {
            migrations: Cow::Owned(
                STATE_MIGRATOR
                    .migrations
                    .iter()
                    .filter(|migration| migration.version < 27)
                    .cloned()
                    .collect(),
            ),
            ignore_missing: false,
            locking: STATE_MIGRATOR.locking,
            no_tx: STATE_MIGRATOR.no_tx,
        };
        pre_conflict_migrator
            .run(&pool)
            .await
            .expect("apply schema before conflicting vl migrations");

        sqlx::query(
            "CREATE TABLE thread_loop_jobs (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                label TEXT NOT NULL,
                prompt_text TEXT NOT NULL,
                interval_seconds INTEGER NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                run_policy TEXT NOT NULL DEFAULT 'queue_one',
                next_run_ms INTEGER,
                last_run_ms INTEGER,
                last_status TEXT,
                last_error TEXT,
                pending_tick INTEGER NOT NULL DEFAULT 0,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                UNIQUE(thread_id, label)
            )",
        )
        .execute(&pool)
        .await
        .expect("create legacy loop jobs table");
        sqlx::query("CREATE INDEX idx_thread_loop_jobs_thread_id ON thread_loop_jobs(thread_id)")
            .execute(&pool)
            .await
            .expect("create legacy loop index");
        sqlx::query(
            "CREATE TABLE thread_loop_owners (
                thread_id TEXT PRIMARY KEY NOT NULL,
                owner_kind TEXT NOT NULL,
                owner_vivling_id TEXT,
                updated_at_ms INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .expect("create legacy loop owners table");
        sqlx::query(
            "CREATE INDEX idx_threads_archived_cwd_created_at_ms ON threads(archived, cwd, created_at_ms DESC, id DESC)",
        )
        .execute(&pool)
        .await
        .expect("create upstream cwd created index at legacy version");
        sqlx::query(
            "CREATE INDEX idx_threads_archived_cwd_updated_at_ms ON threads(archived, cwd, updated_at_ms DESC, id DESC)",
        )
        .execute(&pool)
        .await
        .expect("create upstream cwd updated index at legacy version");
        sqlx::query(
            "CREATE TABLE device_key_bindings (
                key_id TEXT PRIMARY KEY NOT NULL,
                account_user_id TEXT NOT NULL,
                client_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .expect("create renumbered device key table");
        sqlx::query(
            "CREATE TABLE thread_goals (
                thread_id TEXT PRIMARY KEY NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
                goal_id TEXT NOT NULL,
                objective TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('active', 'paused', 'budget_limited', 'complete')),
                token_budget INTEGER,
                tokens_used INTEGER NOT NULL DEFAULT 0,
                time_used_seconds INTEGER NOT NULL DEFAULT 0,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .expect("create renumbered goals table");
        for (version, description) in [
            (27_i64, "thread loop jobs"),
            (28_i64, "thread loop jobs management"),
            (29_i64, "threads cwd sort indexes"),
            (30_i64, "thread loop owners"),
            (910_i64, "device key bindings"),
            (920_i64, "thread goals"),
        ] {
            sqlx::query(
                "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(version)
            .bind(description)
            .bind(true)
            .bind(vec![version as u8])
            .bind(1_i64)
            .execute(&pool)
            .await
            .expect("insert legacy migration row");
        }
        pool.close().await;

        let tolerant_migrator = runtime_state_migrator();
        let tolerant_pool = open_state_sqlite(
            state_path.as_path(),
            &tolerant_migrator,
            SqliteRuntimeMode::default(),
        )
        .await
        .expect("runtime migrator should reconcile old vl migration numbers");

        let vl_loop_table: (String,) = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'vl_thread_loop_jobs'",
        )
        .fetch_one(&tolerant_pool)
        .await
        .expect("vl loop jobs table exists");
        assert_eq!(vl_loop_table.0, "vl_thread_loop_jobs");
        let old_loop_table: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'thread_loop_jobs'",
        )
        .fetch_optional(&tolerant_pool)
        .await
        .expect("query old loop table");
        assert!(old_loop_table.is_none());
        let goal_column: (String,) = sqlx::query_as(
            "SELECT name FROM pragma_table_info('vl_thread_loop_jobs') WHERE name = 'goal_text'",
        )
        .fetch_one(&tolerant_pool)
        .await
        .expect("goal_text compatibility column exists");
        assert_eq!(goal_column.0, "goal_text");

        let version_27_description: String =
            sqlx::query_scalar("SELECT description FROM _sqlx_migrations WHERE version = 27")
                .fetch_one(&tolerant_pool)
                .await
                .expect("version 27 is marked as current upstream migration");
        assert_eq!(version_27_description, "threads cwd sort indexes");
        let version_29_description: String =
            sqlx::query_scalar("SELECT description FROM _sqlx_migrations WHERE version = 29")
                .fetch_one(&tolerant_pool)
                .await
                .expect("version 29 is marked as current upstream migration");
        assert_eq!(version_29_description, "thread goals");
        tolerant_pool.close().await;

        let _ = tokio::fs::remove_dir_all(codex_home).await;
    }

    #[test]
    fn sqlite_runtime_mode_android_compat_uses_delete_and_single_connection() {
        let mode = SqliteRuntimeMode::android_compat();
        assert_eq!(mode.journal_mode, SqliteJournalMode::Delete);
        assert_eq!(mode.max_connections, 1);
    }
}
