use crate::LogEntry;
use crate::LogQuery;
use crate::LogRow;
use crate::SortKey;
use crate::SqliteConfig;
use crate::ThreadMetadata;
use crate::ThreadMetadataBuilder;
use crate::ThreadsPage;
use crate::apply_rollout_item;
use crate::migrations::repair_legacy_recency_migration_version;
use crate::migrations::runtime_goals_migrator;
use crate::migrations::runtime_logs_migrator;
use crate::migrations::runtime_memories_migrator;
use crate::migrations::runtime_queue_migrator;
use crate::migrations::runtime_state_migrator;
use crate::migrations::runtime_thread_history_migrator;
use crate::model::ThreadRow;
use crate::model::anchor_from_item;
use crate::model::datetime_to_epoch_millis;
use crate::model::datetime_to_epoch_seconds;
use crate::model::epoch_millis_to_datetime;
use crate::paths::file_modified_time_utc;
use crate::sqlite::GOALS_DB_FILENAME;
use crate::sqlite::LOGS_DB_FILENAME;
use crate::sqlite::MEMORIES_DB_FILENAME;
use crate::sqlite::STATE_DB_FILENAME;
use crate::sqlite::THREAD_HISTORY_DB_FILENAME;
use crate::telemetry::DbKind;
use crate::telemetry::DbTelemetry;
use chrono::DateTime;
use chrono::Utc;
use codex_history::RolloutItem;
use codex_protocol::ThreadId;
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
use std::time::Instant;
use tracing::warn;

mod backfill;
mod external_agent_config_imports;
mod goals;
mod logs;
mod loop_delegations;
mod loop_descriptors;
mod loop_notifications;
mod loop_occurrences;
mod memories;
mod projects;
mod queued_items;
mod recovery;
mod remote_control;
mod rollout_migration;
#[cfg(test)]
pub(crate) mod test_support;
mod thread_loop_jobs;
mod thread_section_order;
mod thread_sections;
mod threads;

pub use external_agent_config_imports::ExternalAgentConfigImportDetailsRecord;
pub use external_agent_config_imports::ExternalAgentConfigImportFailureRecord;
pub use external_agent_config_imports::ExternalAgentConfigImportHistoryRecord;
pub use external_agent_config_imports::ExternalAgentConfigImportSuccessRecord;
pub use goals::GoalAccountingMode;
pub use goals::GoalAccountingOutcome;
pub use goals::GoalStore;
pub use goals::GoalUpdate;
pub use loop_notifications::LoopNotificationPendingRow;
pub use memories::MemoryStore;
pub use queued_items::SqliteQueueStore;
pub use recovery::RuntimeDbBackup;
pub(super) use recovery::RuntimeDbInitError;
pub use recovery::backup_runtime_db_for_fresh_start;
pub use recovery::is_sqlite_corruption_error;
pub use recovery::runtime_db_path_for_corruption_error;
pub use recovery::sqlite_error_detail_is_corruption;
pub use recovery::sqlite_error_detail_is_lock;
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
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(60);
const SQLITE_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy)]
struct RuntimeDbSpec {
    label: &'static str,
    filename: &'static str,
    kind: DbKind,
    open_phase: &'static str,
    migrate_phase: &'static str,
}

impl RuntimeDbSpec {
    fn path(self, codex_home: &Path) -> PathBuf {
        codex_home.join(self.filename)
    }
}

const STATE_DB: RuntimeDbSpec = RuntimeDbSpec {
    label: "state DB",
    filename: STATE_DB_FILENAME,
    kind: DbKind::State,
    open_phase: "open_state",
    migrate_phase: "migrate_state",
};

#[derive(Clone)]
pub struct StateRuntime {
    sqlite: SqliteConfig,
    default_provider: String,
    pool: Arc<sqlx::SqlitePool>,
    logs_pool: Arc<sqlx::SqlitePool>,
    thread_goals: GoalStore,
    memories: MemoryStore,
    thread_queue: SqliteQueueStore,
    thread_updated_at_millis: Arc<AtomicI64>,
    thread_recency_at_millis: Arc<AtomicI64>,
}

impl StateRuntime {
    /// Initialize the state runtime using the provided SQLite configuration and default provider.
    ///
    /// This opens (and migrates) the SQLite databases under the configured
    /// `sqlite_home`.
    /// Logs and paginated thread history live in dedicated files to reduce
    /// lock contention with the rest of the state store.
    pub async fn init(sqlite: SqliteConfig, default_provider: String) -> anyhow::Result<Arc<Self>> {
        Self::init_inner(sqlite, default_provider, /*telemetry_override*/ None).await
    }

    #[cfg(test)]
    pub(crate) async fn init_with_telemetry_for_tests(
        sqlite: SqliteConfig,
        default_provider: String,
        telemetry_override: &dyn DbTelemetry,
    ) -> anyhow::Result<Arc<Self>> {
        Self::init_inner(sqlite, default_provider, Some(telemetry_override)).await
    }

    async fn init_inner(
        sqlite: SqliteConfig,
        default_provider: String,
        telemetry_override: Option<&dyn DbTelemetry>,
    ) -> anyhow::Result<Arc<Self>> {
        tokio::fs::create_dir_all(sqlite.home()).await?;
        let state_migrator = runtime_state_migrator();
        let logs_migrator = runtime_logs_migrator();
        let goals_migrator = runtime_goals_migrator();
        let memories_migrator = runtime_memories_migrator();
        let queue_migrator = runtime_queue_migrator();
        let state_path = sqlite.state_db_path();
        let logs_path = sqlite.logs_db_path();
        let goals_path = sqlite.goals_db_path();
        let memories_path = sqlite.memories_db_path();
        let queue_path = sqlite.queue_db_path();
        // Fork: open_state_sqlite wraps sqlite.open_state_db with the VL migration
        // reconciliation (device-key renumber, legacy loop migration numbers), so the
        // upstream call has to stay behind it — see its tests below.
        let pool = match open_state_sqlite(
            &sqlite,
            &state_path,
            &state_migrator,
            telemetry_override,
        )
        .await
        {
            Ok(db) => Arc::new(db),
            Err(err) => {
                if cfg!(target_os = "android") {
                    warn!(
                        "failed to open state db at {} with WAL mode; retrying Android compatibility mode: {err}",
                        state_path.display()
                    );
                    match open_state_sqlite_with_mode(
                        &sqlite,
                        &state_path,
                        &state_migrator,
                        SqliteRuntimeMode::android_compat(),
                        telemetry_override,
                    )
                    .await
                    {
                        Ok(db) => Arc::new(db),
                        Err(retry_err) => {
                            warn!(
                                "failed to open state db at {} with Android compatibility mode: {retry_err}",
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
        let logs_pool = match sqlite
            .open_logs_db(&logs_migrator, telemetry_override)
            .await
        {
            Ok(db) => Arc::new(db),
            Err(err) => {
                warn!("failed to open logs db at {}: {err}", logs_path.display());
                close_sqlite_pools(&[pool.as_ref()]).await;
                return Err(err);
            }
        };
        let goals_pool = match sqlite
            .open_goals_db(&goals_migrator, telemetry_override)
            .await
        {
            Ok(db) => Arc::new(db),
            Err(err) => {
                warn!("failed to open goals db at {}: {err}", goals_path.display());
                close_sqlite_pools(&[pool.as_ref(), logs_pool.as_ref()]).await;
                return Err(err);
            }
        };
        let memories_pool = match sqlite
            .open_memories_db(&memories_migrator, telemetry_override)
            .await
        {
            Ok(db) => Arc::new(db),
            Err(err) => {
                warn!(
                    "failed to open memories db at {}: {err}",
                    memories_path.display()
                );
                close_sqlite_pools(&[pool.as_ref(), logs_pool.as_ref(), goals_pool.as_ref()]).await;
                return Err(err);
            }
        };
        let queue_pool = match sqlite
            .open_queue_db(&queue_migrator, telemetry_override)
            .await
        {
            Ok(db) => Arc::new(db),
            Err(err) => {
                warn!("failed to open queue db at {}: {err}", queue_path.display());
                close_sqlite_pools(&[
                    pool.as_ref(),
                    logs_pool.as_ref(),
                    goals_pool.as_ref(),
                    memories_pool.as_ref(),
                ])
                .await;
                return Err(err);
            }
        };
        let started = Instant::now();
        let backfill_state_result = ensure_backfill_state_row_in_pool(pool.as_ref()).await;
        crate::telemetry::record_init_result(
            telemetry_override,
            DbKind::State,
            "ensure_backfill_state",
            started.elapsed(),
            &backfill_state_result,
        );
        if let Err(err) = backfill_state_result {
            close_sqlite_pools(&[
                pool.as_ref(),
                logs_pool.as_ref(),
                goals_pool.as_ref(),
                memories_pool.as_ref(),
                queue_pool.as_ref(),
            ])
            .await;
            return Err(err);
        }
        let started = Instant::now();
        let thread_timestamp_millis_result: anyhow::Result<(Option<i64>, Option<i64>)> =
            sqlx::query_as(
                "SELECT (SELECT MAX(updated_at_ms) FROM threads), (SELECT MAX(recency_at_ms) FROM threads)",
            )
            .fetch_one(pool.as_ref())
            .await
            .map_err(anyhow::Error::from);
        crate::telemetry::record_init_result(
            telemetry_override,
            DbKind::State,
            "post_init_query",
            started.elapsed(),
            &thread_timestamp_millis_result,
        );
        let (thread_updated_at_millis, thread_recency_at_millis) =
            match thread_timestamp_millis_result {
                Ok(value) => value,
                Err(err) => {
                    close_sqlite_pools(&[
                        pool.as_ref(),
                        logs_pool.as_ref(),
                        goals_pool.as_ref(),
                        memories_pool.as_ref(),
                        queue_pool.as_ref(),
                    ])
                    .await;
                    return Err(err);
                }
            };
        let thread_updated_at_millis = thread_updated_at_millis.unwrap_or(0);
        let thread_recency_at_millis = thread_recency_at_millis.unwrap_or(0);
        let runtime = Arc::new(Self {
            thread_goals: GoalStore::new(Arc::clone(&goals_pool)),
            memories: MemoryStore::new(Arc::clone(&memories_pool), Arc::clone(&pool)),
            thread_queue: SqliteQueueStore::new(queue_pool),
            pool,
            logs_pool,
            sqlite,
            default_provider,
            thread_updated_at_millis: Arc::new(AtomicI64::new(thread_updated_at_millis)),
            thread_recency_at_millis: Arc::new(AtomicI64::new(thread_recency_at_millis)),
        });
        if let Err(err) = runtime.run_logs_startup_maintenance().await {
            warn!(
                "failed to run startup maintenance for logs db at {}: {err}",
                logs_path.display(),
            );
        }
        Ok(runtime)
    }

    /// Return the SQLite configuration for this runtime.
    pub fn sqlite(&self) -> &SqliteConfig {
        &self.sqlite
    }

    pub fn thread_goals(&self) -> &GoalStore {
        &self.thread_goals
    }

    pub fn memories(&self) -> &MemoryStore {
        &self.memories
    }

    /// Return the durable, SQLite-backed user-message queue.
    pub fn thread_queue(&self) -> &SqliteQueueStore {
        &self.thread_queue
    }

    /// Close all SQLite pools and wait for outstanding pool workers to exit.
    pub async fn close(&self) {
        self.thread_queue.close().await;
        self.memories.close().await;
        self.thread_goals.close().await;
        self.logs_pool.close().await;
        self.pool.close().await;
    }

    pub async fn clear_memory_data_in_sqlite_home(sqlite: &SqliteConfig) -> anyhow::Result<bool> {
        let memories_path = sqlite.memories_db_path();
        if !tokio::fs::try_exists(&memories_path).await? {
            return Ok(false);
        }

        let memories_migrator = runtime_memories_migrator();
        let pool = sqlite
            .open_memories_db(&memories_migrator, /*telemetry_override*/ None)
            .await?;
        memories::clear_memory_data_in_pool(&pool).await?;
        pool.close().await;
        Ok(true)
    }
}

async fn close_sqlite_pools(pools: &[&SqlitePool]) {
    for pool in pools {
        pool.close().await;
    }
}

fn base_sqlite_options(path: &Path, mode: SqliteRuntimeMode) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(mode.journal_mode)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(SQLITE_BUSY_TIMEOUT)
        .log_statements(LevelFilter::Off)
}

async fn open_state_sqlite(
    sqlite: &SqliteConfig,
    path: &Path,
    migrator: &Migrator,
    telemetry_override: Option<&dyn DbTelemetry>,
) -> anyhow::Result<SqlitePool> {
    // New state DBs should use incremental auto-vacuum, but retrofitting an
    // existing DB requires a full VACUUM. Do not attempt that during process
    // startup: it is maintenance work that can contend with foreground writers.
    open_state_sqlite_with_mode(
        sqlite,
        path,
        migrator,
        SqliteRuntimeMode::default(),
        telemetry_override,
    )
    .await
}

async fn open_state_sqlite_with_mode(
    sqlite: &SqliteConfig,
    path: &Path,
    migrator: &Migrator,
    mode: SqliteRuntimeMode,
    telemetry_override: Option<&dyn DbTelemetry>,
) -> anyhow::Result<SqlitePool> {
    // codex-vl: preserve `SqliteRuntimeMode` (Android WAL-compat) +
    // `reconcile_vl_legacy` (loop-migration reconciliation) fork params while
    // adopting upstream `RuntimeDbSpec` + `SqliteConfig` caller signature.
    open_sqlite(
        sqlite,
        path,
        migrator,
        STATE_DB,
        mode,
        /*reconcile_vl_legacy*/ true,
        telemetry_override,
    )
    .await
}

/// Open and migrate the rebuildable paginated thread-history database.
pub async fn open_thread_history_db(sqlite: &SqliteConfig) -> anyhow::Result<SqlitePool> {
    let migrator = runtime_thread_history_migrator();
    sqlite
        .open_thread_history_db(&migrator, /*telemetry_override*/ None)
        .await
}

async fn open_sqlite(
    sqlite: &SqliteConfig,
    path: &Path,
    migrator: &Migrator,
    spec: RuntimeDbSpec,
    mode: SqliteRuntimeMode,
    reconcile_vl_legacy: bool,
    telemetry_override: Option<&dyn DbTelemetry>,
) -> anyhow::Result<SqlitePool> {
    // codex-vl: the fork applies `SqliteRuntimeMode` (journal_mode for
    // Android/Termux WAL-compat + max_connections) via `base_sqlite_options`,
    // which upstream's `SqliteConfig::open_read_write_pool` does not expose.
    // `sqlite` is accepted for signature alignment with upstream callers and
    // kept unused on purpose so the runtime mode — required for Android and
    // loop-migration reconciliation — remains authoritative here.
    let _ = sqlite;
    let options = base_sqlite_options(path, mode).auto_vacuum(SqliteAutoVacuum::Incremental);
    let started = Instant::now();
    let pool_result = SqlitePoolOptions::new()
        .max_connections(mode.max_connections)
        .acquire_timeout(SQLITE_ACQUIRE_TIMEOUT)
        .connect_with(options)
        .await
        .map_err(anyhow::Error::from);
    crate::telemetry::record_init_result(
        telemetry_override,
        spec.kind,
        spec.open_phase,
        started.elapsed(),
        &pool_result,
    );
    let pool = pool_result
        .map_err(|source| recovery::RuntimeDbInitError::new(spec.label, "open", path, source))?;
    let started = Instant::now();
    let migrate_result = async {
        if reconcile_vl_legacy {
            reconcile_vl_legacy_state_migrations(&pool, migrator).await?;
        }
        if matches!(spec.kind, DbKind::State) {
            repair_legacy_recency_migration_version(&pool, migrator).await?;
        }
        migrator.run(&pool).await.map_err(anyhow::Error::from)
    }
    .await;
    crate::telemetry::record_init_result(
        telemetry_override,
        spec.kind,
        spec.migrate_phase,
        started.elapsed(),
        &migrate_result,
    );
    if let Err(source) = migrate_result {
        pool.close().await;
        return Err(recovery::RuntimeDbInitError::new(spec.label, "migrate", path, source).into());
    }
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

    // Older codex-vl builds carried local migrations in upstream-numbered
    // slots before those slots were filled upstream. Keep their schema changes,
    // but remove the incompatible applied records so upstream can own 27..31.
    delete_applied_migration_if_description(pool, 27, "thread loop jobs").await?;
    delete_applied_migration_if_description(pool, 28, "thread loop jobs management").await?;
    delete_applied_migration_if_description(pool, 29, "threads cwd sort indexes").await?;
    delete_applied_migration_if_description(pool, 30, "thread loop owners").await?;
    delete_applied_migration_if_description(pool, 31, "drop device key bindings").await?;

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
            "run_policy",
            "TEXT NOT NULL DEFAULT 'queue_one'",
        )
        .await?;
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
    table_name: &str,
    column_name: &str,
    column_definition: &str,
) -> anyhow::Result<()> {
    if table_column_exists(pool, table_name, column_name).await? {
        return Ok(());
    }

    let sql = format!("ALTER TABLE {table_name} ADD COLUMN {column_name} {column_definition}");
    sqlx::query(sqlx::AssertSqlSafe(sql)).execute(pool).await?;
    Ok(())
}

async fn table_column_exists(
    pool: &SqlitePool,
    table_name: &str,
    column_name: &str,
) -> anyhow::Result<bool> {
    let quoted_table = table_name.replace('\'', "''");
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "PRAGMA table_info('{quoted_table}')"
    )))
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().any(|row| {
        let name: String = row.get("name");
        name == column_name
    }))
}

async fn sqlite_object_exists(
    pool: &SqlitePool,
    object_type: &str,
    object_name: &str,
) -> anyhow::Result<bool> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = ? AND name = ?")
            .bind(object_type)
            .bind(object_name)
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
        .iter()
        .find(|migration| migration.version == version)
    else {
        return Ok(());
    };
    let checksum: Vec<u8> = migration.checksum.as_ref().to_vec();
    sqlx::query(
        r#"
INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
VALUES (?, ?, ?, ?, ?)
ON CONFLICT(version) DO NOTHING
"#,
    )
    .bind(version)
    .bind(migration.description.as_ref())
    .bind(true)
    .bind(checksum)
    .bind(0_i64)
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) async fn ensure_backfill_state_row_in_pool(
    pool: &sqlx::SqlitePool,
) -> anyhow::Result<()> {
    // Eagerly check if the operation would have no effect to avoid blocking waiting for a SQLite
    // writer for no reason in the hot startup path.
    if sqlx::query_scalar::<_, i64>("SELECT 1 FROM backfill_state WHERE id = 1")
        .fetch_optional(pool)
        .await?
        .is_some()
    {
        return Ok(());
    }

    sqlx::query(
        r#"
INSERT INTO backfill_state (id, status, last_watermark, last_success_at, updated_at)
VALUES (?, ?, NULL, NULL, ?)
ON CONFLICT(id) DO NOTHING
            "#,
    )
    .bind(1_i64)
    .bind(crate::BackfillStatus::Pending.as_str())
    .bind(Utc::now().timestamp())
    .execute(pool)
    .await?;
    Ok(())
}

// codex-vl compatibility surface: the TUI and Android recovery path still
// address these paths from a Codex home rather than a resolved SqliteConfig.
pub fn state_db_filename() -> String {
    STATE_DB.filename.to_string()
}

pub fn state_db_path(codex_home: &Path) -> PathBuf {
    STATE_DB.path(codex_home)
}

pub fn logs_db_filename() -> String {
    LOGS_DB_FILENAME.to_string()
}

pub fn logs_db_path(codex_home: &Path) -> PathBuf {
    codex_home.join(LOGS_DB_FILENAME)
}

pub fn goals_db_filename() -> String {
    GOALS_DB_FILENAME.to_string()
}

pub fn goals_db_path(codex_home: &Path) -> PathBuf {
    codex_home.join(GOALS_DB_FILENAME)
}

pub fn memories_db_filename() -> String {
    MEMORIES_DB_FILENAME.to_string()
}

pub fn memories_db_path(codex_home: &Path) -> PathBuf {
    codex_home.join(MEMORIES_DB_FILENAME)
}

pub fn thread_history_db_filename() -> String {
    THREAD_HISTORY_DB_FILENAME.to_string()
}

pub fn thread_history_db_path(codex_home: &Path) -> PathBuf {
    codex_home.join(THREAD_HISTORY_DB_FILENAME)
}

/// Integrity-check rows, including those emitted before interruption.
#[derive(Debug, Eq, PartialEq)]
pub enum SqliteIntegrityCheck {
    Complete(Vec<String>),
    TimedOut(Vec<String>),
}

/// Run SQLite's built-in integrity check against an existing database file.
pub async fn sqlite_integrity_check(
    sqlite: &SqliteConfig,
    path: &Path,
    deadline: Option<Instant>,
) -> anyhow::Result<SqliteIntegrityCheck> {
    let pool = sqlite
        .open_read_only_pool(
            path,
            deadline.map(|deadline| deadline.saturating_duration_since(Instant::now())),
        )
        .await?;
    let mut connection = pool.acquire().await?;
    if let Some(deadline) = deadline {
        // Lock waits do not invoke the progress handler; share the remaining budget.
        QueryBuilder::<Sqlite>::new("PRAGMA busy_timeout = ")
            .push(
                deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis(),
            )
            .build()
            .execute(&mut *connection)
            .await?;
        // Interrupt SQLite itself: dropping a timed-out future leaves its worker scanning.
        connection
            .lock_handle()
            .await?
            .set_progress_handler(/*num_ops*/ 1_000, move || Instant::now() < deadline);
    }
    let mut rows = Vec::<String>::new();
    // Keep corruption rows even when a later step interrupts the scan.
    let result = sqlx::query::<Sqlite>("PRAGMA integrity_check")
        .try_map(|row| {
            rows.push(row.try_get(/*index*/ 0)?);
            Ok(())
        })
        .fetch_all(&mut *connection)
        .await;
    drop(connection);
    pool.close().await;
    match result {
        Ok(_) => Ok(SqliteIntegrityCheck::Complete(rows)),
        Err(sqlx::Error::Database(error))
            if deadline.is_some()
                && error.code().is_some_and(|code| {
                    matches!(
                        code.parse::<i32>().ok().map(|code| code & 0xff),
                        Some(libsqlite3_sys::SQLITE_INTERRUPT | libsqlite3_sys::SQLITE_BUSY)
                    )
                }) =>
        {
            Ok(SqliteIntegrityCheck::TimedOut(rows))
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteConnectOptions;
    use super::SqliteIntegrityCheck;
    use super::SqliteJournalMode;
    use super::SqliteRuntimeMode;
    use super::StateRuntime;
    use super::open_state_sqlite;
    use super::runtime_state_migrator;
    use super::sqlite_integrity_check;
    use super::state_db_path;
    use super::test_support::test_thread_metadata;
    use super::test_support::unique_temp_dir;
    use crate::DB_INIT_METRIC;
    use crate::DbTelemetry;
    use crate::migrations::STATE_MIGRATOR;
    use codex_protocol::ThreadId;
    use codex_utils_absolute_path::test_support::PathExt;
    use pretty_assertions::assert_eq;
    use sqlx::SqlitePool;
    use sqlx::migrate::MigrateError;
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::Mutex;
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    use std::time::Instant;

    #[derive(Default)]
    struct TestTelemetry {
        counters: Mutex<Vec<MetricEvent>>,
    }

    #[derive(Debug, Eq, PartialEq)]
    struct MetricEvent {
        name: String,
        tags: BTreeMap<String, String>,
    }

    impl TestTelemetry {
        fn counters(&self) -> Vec<MetricEvent> {
            self.counters
                .lock()
                .expect("telemetry lock")
                .iter()
                .map(|event| MetricEvent {
                    name: event.name.clone(),
                    tags: event.tags.clone(),
                })
                .collect()
        }
    }

    impl DbTelemetry for TestTelemetry {
        fn counter(&self, name: &str, _inc: i64, tags: &[(&str, &str)]) {
            self.counters
                .lock()
                .expect("telemetry lock")
                .push(MetricEvent {
                    name: name.to_string(),
                    tags: tags_to_map(tags),
                });
        }

        fn record_duration(
            &self,
            _name: &str,
            _duration: std::time::Duration,
            _tags: &[(&str, &str)],
        ) {
        }

        fn histogram(&self, _name: &str, _value: i64, _tags: &[(&str, &str)]) {}
    }

    fn tags_to_map(tags: &[(&str, &str)]) -> BTreeMap<String, String> {
        tags.iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    async fn open_db_pool(path: &Path) -> SqlitePool {
        crate::SqliteConfig::new_for_testing(path.parent().unwrap_or(path).abs())
            .open_read_write_pool(path)
            .await
            .expect("open sqlite pool")
    }

    #[tokio::test]
    async fn sqlite_integrity_check_can_be_interrupted_and_retried() {
        let codex_home = unique_temp_dir();
        tokio::fs::create_dir_all(&codex_home)
            .await
            .expect("create codex home");
        let sqlite = crate::SqliteConfig::new_for_testing(codex_home.as_path().abs());
        let path = sqlite.state_db_path();
        let pool = sqlite
            .open_read_write_pool(&path)
            .await
            .expect("open sqlite db");
        sqlx::query("CREATE TABLE sample (id INTEGER PRIMARY KEY, value INTEGER, other INTEGER)")
            .execute(&pool)
            .await
            .expect("create sample table");
        sqlx::query("CREATE UNIQUE INDEX sample_value ON sample(value)")
            .execute(&pool)
            .await
            .expect("create sample index");
        sqlx::query(
            "WITH RECURSIVE rows(id) AS (SELECT 1 UNION ALL SELECT id + 1 FROM rows WHERE id < 2048) INSERT INTO sample SELECT id, id, CASE WHEN id = 1 THEN 0 ELSE id END FROM rows",
        )
        .execute(&pool)
        .await
        .expect("populate enough rows to invoke the progress handler");
        pool.close().await;

        assert_eq!(
            sqlite_integrity_check(&sqlite, &path, Some(Instant::now()))
                .await
                .expect("interrupt integrity check"),
            SqliteIntegrityCheck::TimedOut(Vec::new()),
        );

        let result = sqlite_integrity_check(&sqlite, &path, /*deadline*/ None)
            .await
            .expect("integrity check should run");

        assert_eq!(
            result,
            SqliteIntegrityCheck::Complete(vec!["ok".to_string()])
        );

        let pool = sqlite
            .open_read_write_pool(&path)
            .await
            .expect("reopen sqlite db");
        let mut connection = pool.acquire().await.expect("acquire writer");
        // WAL readers do not wait on this writer, so use a rollback journal.
        sqlx::query("PRAGMA journal_mode=DELETE; BEGIN EXCLUSIVE")
            .execute(&mut *connection)
            .await
            .expect("hold an exclusive lock");
        let started = Instant::now();
        let result =
            sqlite_integrity_check(&sqlite, &path, Some(started + Duration::from_millis(50)))
                .await
                .expect("interrupt lock wait");
        let elapsed = started.elapsed();
        sqlx::query("ROLLBACK")
            .execute(&mut *connection)
            .await
            .expect("release lock");
        assert_eq!(result, SqliteIntegrityCheck::TimedOut(Vec::new()));
        assert!(elapsed < Duration::from_secs(1), "lock wait: {elapsed:?}");

        // Misdescribe one index entry so SQLite emits an error before its first progress callback.
        sqlx::query(
            "PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql='CREATE UNIQUE INDEX sample_value ON sample(other)' WHERE name='sample_value'; PRAGMA writable_schema=OFF",
        )
        .execute(&mut *connection)
        .await
        .expect("introduce an index mismatch");
        drop(connection);
        pool.close().await;
        assert_eq!(
            sqlite_integrity_check(&sqlite, &path, Some(Instant::now()))
                .await
                .expect("retain corruption before interruption"),
            SqliteIntegrityCheck::TimedOut(vec![
                "row 1 missing from index sample_value".to_string()
            ]),
        );
        let _ = tokio::fs::remove_dir_all(codex_home).await;
    }

    #[tokio::test]
    async fn open_state_sqlite_tolerates_newer_applied_migrations() {
        let codex_home = unique_temp_dir();
        tokio::fs::create_dir_all(&codex_home)
            .await
            .expect("create codex home");
        let sqlite = crate::SqliteConfig::new_for_testing(codex_home.as_path().abs());
        let state_path = sqlite.state_db_path();
        let pool = sqlite
            .open_read_write_pool(&state_path)
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
        let tolerant_pool = sqlite
            .open_state_db(&tolerant_migrator, /*telemetry_override*/ None)
            .await
            .expect("runtime migrator should tolerate newer applied migrations");
        tolerant_pool.close().await;

        let _ = tokio::fs::remove_dir_all(codex_home).await;
    }

    #[tokio::test]
    async fn init_records_successful_sqlite_init_phases_to_explicit_telemetry() {
        let codex_home = unique_temp_dir();
        let telemetry = TestTelemetry::default();

        let runtime = StateRuntime::init_with_telemetry_for_tests(
            crate::SqliteConfig::new_for_testing(codex_home.as_path().abs()),
            "test-provider".to_string(),
            &telemetry,
        )
        .await
        .expect("state runtime should initialize");

        let phases = telemetry
            .counters()
            .into_iter()
            .filter(|event| event.name == DB_INIT_METRIC)
            .filter(|event| event.tags.get("status").map(String::as_str) == Some("success"))
            .filter_map(|event| event.tags.get("phase").cloned())
            .collect::<BTreeSet<_>>();
        let expected = [
            "open_state",
            "migrate_state",
            "open_logs",
            "migrate_logs",
            "open_goals",
            "migrate_goals",
            "open_memories",
            "migrate_memories",
            "open_queue",
            "migrate_queue",
            "ensure_backfill_state",
            "post_init_query",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
        assert_eq!(phases, expected);

        runtime.close().await;
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
        STATE_MIGRATOR
            .run(&pool)
            .await
            .expect("apply current state schema");
        sqlx::query("UPDATE _sqlx_migrations SET description = ?, checksum = ? WHERE version = ?")
            .bind("thread loop jobs management")
            .bind(vec![9_u8])
            .bind(28_i64)
            .execute(&pool)
            .await
            .expect("rewrite version 28 as legacy conflicting migration");
        pool.close().await;

        let migrator = runtime_state_migrator();
        let sqlite = crate::SqliteConfig::new_for_testing(codex_home.as_path().abs());
        let pool = open_state_sqlite(&sqlite, state_path.as_path(), &migrator, None)
            .await
            .expect("runtime migrator reconciles legacy migration record");
        let description: String =
            sqlx::query_scalar("SELECT description FROM _sqlx_migrations WHERE version = 28")
                .fetch_one(&pool)
                .await
                .expect("version 28 migration row");
        assert_eq!(description, "device key bindings");
        pool.close().await;

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

        sqlx::query(
            r#"
CREATE TABLE _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    success BOOLEAN NOT NULL,
    checksum BLOB NOT NULL,
    execution_time BIGINT NOT NULL
)
"#,
        )
        .execute(&pool)
        .await
        .expect("create sqlx migrations table");
        sqlx::query(
            r#"
CREATE TABLE thread_loop_jobs (
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
)
"#,
        )
        .execute(&pool)
        .await
        .expect("create legacy loop table");
        sqlx::query(
            r#"
INSERT INTO thread_loop_jobs (
    id, thread_id, label, goal_text, prompt_text, interval_seconds, enabled,
    run_policy, auto_remove_on_completion, created_by, next_run_ms, last_run_ms,
    last_status, last_error, pending_tick, created_at_ms, updated_at_ms
)
VALUES ('loop-1', 'thread-1', 'ci', 'goal', 'tick', 300, 1, 'queue_one', 1, 'agent', 1, NULL, NULL, NULL, 0, 1, 1)
"#,
        )
        .execute(&pool)
        .await
        .expect("insert legacy loop row");
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(27_i64)
        .bind("thread loop jobs")
        .bind(true)
        .bind(vec![7_u8])
        .bind(1_i64)
        .execute(&pool)
        .await
        .expect("insert legacy version 27");
        pool.close().await;

        let migrator = runtime_state_migrator();
        let sqlite = crate::SqliteConfig::new_for_testing(codex_home.as_path().abs());
        let pool = open_state_sqlite(&sqlite, state_path.as_path(), &migrator, None)
            .await
            .expect("runtime migrator reconciles legacy loop migrations");
        let loop_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vl_thread_loop_jobs")
            .fetch_one(&pool)
            .await
            .expect("count migrated loop jobs");
        assert_eq!(loop_count, 1);
        let description: String =
            sqlx::query_scalar("SELECT description FROM _sqlx_migrations WHERE version = 27")
                .fetch_one(&pool)
                .await
                .expect("version 27 migration row");
        assert_eq!(description, "threads cwd sort indexes");
        pool.close().await;

        let _ = tokio::fs::remove_dir_all(codex_home).await;
    }

    #[test]
    fn sqlite_runtime_mode_android_compat_uses_delete_and_single_connection() {
        let mode = SqliteRuntimeMode::android_compat();
        assert_eq!(mode.journal_mode, SqliteJournalMode::Delete);
        assert_eq!(mode.max_connections, 1);
    }

    #[test]
    fn runtime_state_migrator_keeps_vl_migrations_outside_upstream_number_range() {
        let migrations = runtime_state_migrator();
        let vl_versions = migrations
            .iter()
            .filter(|migration| migration.description.as_ref().starts_with("vl "))
            .map(|migration| migration.version)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            vl_versions,
            BTreeSet::from([
                930_i64, 931_i64, 932_i64, 933_i64, 934_i64, 935_i64, 936_i64, 937_i64
            ])
        );

        let descriptions = migrations
            .iter()
            .map(|migration| (migration.version, migration.description.to_string()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            descriptions.get(&27).map(String::as_str),
            Some("threads cwd sort indexes")
        );
        assert_eq!(
            descriptions.get(&28).map(String::as_str),
            Some("device key bindings")
        );
        assert_eq!(
            descriptions.get(&29).map(String::as_str),
            Some("thread goals")
        );
        assert_eq!(
            descriptions.get(&937).map(String::as_str),
            Some("vl loop strategy override")
        );
    }
    #[tokio::test]
    async fn init_restores_independent_thread_timestamp_maxima() {
        let codex_home = unique_temp_dir();
        let sqlite = crate::SqliteConfig::new_for_testing(codex_home.as_path().abs());
        let runtime = StateRuntime::init(sqlite.clone(), "test-provider".to_string())
            .await
            .expect("state runtime should initialize");

        for (thread_id, updated_at_ms, recency_at_ms) in [
            ("00000000-0000-0000-0000-000000000101", 3_000, 1_000),
            ("00000000-0000-0000-0000-000000000102", 1_000, 4_000),
        ] {
            let thread_id = ThreadId::from_string(thread_id).expect("valid thread id");
            runtime
                .upsert_thread(&test_thread_metadata(
                    &codex_home,
                    thread_id,
                    codex_home.clone(),
                ))
                .await
                .expect("thread should be stored");
            sqlx::query("UPDATE threads SET updated_at_ms = ?, recency_at_ms = ? WHERE id = ?")
                .bind(updated_at_ms)
                .bind(recency_at_ms)
                .bind(thread_id.to_string())
                .execute(runtime.pool.as_ref())
                .await
                .expect("thread timestamps should be updated");
        }

        runtime.close().await;
        drop(runtime);

        let runtime = StateRuntime::init(sqlite, "test-provider".to_string())
            .await
            .expect("state runtime should restore thread timestamps");
        assert_eq!(
            (
                runtime.thread_updated_at_millis.load(Ordering::Relaxed),
                runtime.thread_recency_at_millis.load(Ordering::Relaxed),
            ),
            (3_000, 4_000)
        );

        runtime.close().await;
        let _ = tokio::fs::remove_dir_all(codex_home).await;
    }
}
