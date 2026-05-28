use crate::AgentJob;
use crate::AgentJobCreateParams;
use crate::AgentJobItem;
use crate::AgentJobItemCreateParams;
use crate::AgentJobItemStatus;
use crate::AgentJobProgress;
use crate::AgentJobStatus;
use crate::GOALS_DB_FILENAME;
use crate::LOGS_DB_FILENAME;
use crate::LogEntry;
use crate::LogQuery;
use crate::LogRow;
use crate::MEMORIES_DB_FILENAME;
use crate::STATE_DB_FILENAME;
use crate::SortKey;
use crate::ThreadMetadata;
use crate::ThreadMetadataBuilder;
use crate::ThreadsPage;
use crate::apply_rollout_item;
use crate::migrations::runtime_goals_migrator;
use crate::migrations::runtime_logs_migrator;
use crate::migrations::runtime_memories_migrator;
use crate::migrations::runtime_state_migrator;
use crate::model::AgentJobRow;
use crate::model::ThreadRow;
use crate::model::anchor_from_item;
use crate::model::datetime_to_epoch_millis;
use crate::model::datetime_to_epoch_seconds;
use crate::model::epoch_millis_to_datetime;
use crate::paths::file_modified_time_utc;
use crate::telemetry::DbKind;
use crate::telemetry::DbTelemetry;
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
use std::time::Instant;
use tracing::warn;

mod agent_jobs;
mod backfill;
mod goals;
mod logs;
mod memories;
mod remote_control;
#[cfg(test)]
mod test_support;
mod thread_loop_jobs;
mod threads;

pub use goals::GoalAccountingMode;
pub use goals::GoalAccountingOutcome;
pub use goals::GoalStore;
pub use goals::GoalUpdate;
pub use memories::MemoryStore;
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

const LOGS_DB: RuntimeDbSpec = RuntimeDbSpec {
    label: "log DB",
    filename: LOGS_DB_FILENAME,
    kind: DbKind::Logs,
    open_phase: "open_logs",
    migrate_phase: "migrate_logs",
};

const GOALS_DB: RuntimeDbSpec = RuntimeDbSpec {
    label: "goals DB",
    filename: GOALS_DB_FILENAME,
    kind: DbKind::Goals,
    open_phase: "open_goals",
    migrate_phase: "migrate_goals",
};

const MEMORIES_DB: RuntimeDbSpec = RuntimeDbSpec {
    label: "memories DB",
    filename: MEMORIES_DB_FILENAME,
    kind: DbKind::Memories,
    open_phase: "open_memories",
    migrate_phase: "migrate_memories",
};

const RUNTIME_DBS: [RuntimeDbSpec; 4] = [STATE_DB, LOGS_DB, GOALS_DB, MEMORIES_DB];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeDbPath {
    pub label: &'static str,
    pub path: PathBuf,
}

#[derive(Clone)]
pub struct StateRuntime {
    codex_home: PathBuf,
    default_provider: String,
    pool: Arc<sqlx::SqlitePool>,
    logs_pool: Arc<sqlx::SqlitePool>,
    thread_goals: GoalStore,
    memories: MemoryStore,
    thread_updated_at_millis: Arc<AtomicI64>,
}

impl StateRuntime {
    /// Initialize the state runtime using the provided Codex home and default provider.
    ///
    /// This opens (and migrates) the SQLite databases under `codex_home`,
    /// keeping logs in a dedicated file to reduce lock contention with the
    /// rest of the state store.
    pub async fn init(codex_home: PathBuf, default_provider: String) -> anyhow::Result<Arc<Self>> {
        Self::init_inner(
            codex_home,
            default_provider,
            /*telemetry_override*/ None,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn init_with_telemetry_for_tests(
        codex_home: PathBuf,
        default_provider: String,
        telemetry_override: &dyn DbTelemetry,
    ) -> anyhow::Result<Arc<Self>> {
        Self::init_inner(codex_home, default_provider, Some(telemetry_override)).await
    }

    async fn init_inner(
        codex_home: PathBuf,
        default_provider: String,
        telemetry_override: Option<&dyn DbTelemetry>,
    ) -> anyhow::Result<Arc<Self>> {
        tokio::fs::create_dir_all(&codex_home).await?;
        let state_migrator = runtime_state_migrator();
        let logs_migrator = runtime_logs_migrator();
        let goals_migrator = runtime_goals_migrator();
        let memories_migrator = runtime_memories_migrator();
        let state_path = STATE_DB.path(codex_home.as_path());
        let logs_path = LOGS_DB.path(codex_home.as_path());
        let goals_path = GOALS_DB.path(codex_home.as_path());
        let memories_path = MEMORIES_DB.path(codex_home.as_path());
        let pool = match open_state_sqlite(&state_path, &state_migrator, telemetry_override).await {
            Ok(db) => Arc::new(db),
            Err(err) => {
                if cfg!(target_os = "android") {
                    warn!(
                        "failed to open state db at {} with WAL mode; retrying Android compatibility mode: {err}",
                        state_path.display()
                    );
                    match open_state_sqlite_with_mode(
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
        let logs_pool = match open_logs_sqlite(&logs_path, &logs_migrator, telemetry_override).await
        {
            Ok(db) => Arc::new(db),
            Err(err) => {
                warn!("failed to open logs db at {}: {err}", logs_path.display());
                return Err(err);
            }
        };
        let goals_pool =
            match open_goals_sqlite(&goals_path, &goals_migrator, telemetry_override).await {
                Ok(db) => Arc::new(db),
                Err(err) => {
                    warn!("failed to open goals db at {}: {err}", goals_path.display());
                    return Err(err);
                }
            };
        let memories_pool = match open_memories_sqlite(
            &memories_path,
            &memories_migrator,
            telemetry_override,
        )
        .await
        {
            Ok(db) => Arc::new(db),
            Err(err) => {
                warn!(
                    "failed to open memories db at {}: {err}",
                    memories_path.display()
                );
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
        backfill_state_result?;
        let started = Instant::now();
        let thread_updated_at_millis_result: anyhow::Result<Option<i64>> =
            sqlx::query_scalar("SELECT MAX(threads.updated_at_ms) FROM threads")
                .fetch_one(pool.as_ref())
                .await
                .map_err(anyhow::Error::from);
        crate::telemetry::record_init_result(
            telemetry_override,
            DbKind::State,
            "post_init_query",
            started.elapsed(),
            &thread_updated_at_millis_result,
        );
        let thread_updated_at_millis = thread_updated_at_millis_result?;
        let thread_updated_at_millis = thread_updated_at_millis.unwrap_or(0);
        let runtime = Arc::new(Self {
            thread_goals: GoalStore::new(Arc::clone(&goals_pool)),
            memories: MemoryStore::new(Arc::clone(&memories_pool), Arc::clone(&pool)),
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

    pub fn thread_goals(&self) -> &GoalStore {
        &self.thread_goals
    }

    pub fn memories(&self) -> &MemoryStore {
        &self.memories
    }

    pub async fn clear_memory_data_in_sqlite_home(sqlite_home: &Path) -> anyhow::Result<bool> {
        let memories_path = MEMORIES_DB.path(sqlite_home);
        if !tokio::fs::try_exists(&memories_path).await? {
            return Ok(false);
        }

        let memories_migrator = runtime_memories_migrator();
        let pool = open_memories_sqlite(
            &memories_path,
            &memories_migrator,
            /*telemetry_override*/ None,
        )
        .await?;
        memories::clear_memory_data_in_pool(&pool).await?;
        pool.close().await;
        Ok(true)
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
    path: &Path,
    migrator: &Migrator,
    telemetry_override: Option<&dyn DbTelemetry>,
) -> anyhow::Result<SqlitePool> {
    // New state DBs should use incremental auto-vacuum, but retrofitting an
    // existing DB requires a full VACUUM. Do not attempt that during process
    // startup: it is maintenance work that can contend with foreground writers.
    open_state_sqlite_with_mode(
        path,
        migrator,
        SqliteRuntimeMode::default(),
        telemetry_override,
    )
    .await
}

async fn open_state_sqlite_with_mode(
    path: &Path,
    migrator: &Migrator,
    mode: SqliteRuntimeMode,
    telemetry_override: Option<&dyn DbTelemetry>,
) -> anyhow::Result<SqlitePool> {
    // codex-vl: preserve `SqliteRuntimeMode` + `reconcile_vl_legacy` fork
    // params while adopting upstream `RuntimeDbSpec` consolidation.
    open_sqlite(
        path,
        migrator,
        STATE_DB,
        mode,
        /*reconcile_vl_legacy*/ true,
        telemetry_override,
    )
    .await
}

async fn open_logs_sqlite(
    path: &Path,
    migrator: &Migrator,
    telemetry_override: Option<&dyn DbTelemetry>,
) -> anyhow::Result<SqlitePool> {
    open_sqlite(
        path,
        migrator,
        LOGS_DB,
        SqliteRuntimeMode::default(),
        /*reconcile_vl_legacy*/ false,
        telemetry_override,
    )
    .await
}

async fn open_goals_sqlite(
    path: &Path,
    migrator: &Migrator,
    telemetry_override: Option<&dyn DbTelemetry>,
) -> anyhow::Result<SqlitePool> {
    open_sqlite(
        path,
        migrator,
        GOALS_DB,
        SqliteRuntimeMode::default(),
        /*reconcile_vl_legacy*/ false,
        telemetry_override,
    )
    .await
}

async fn open_memories_sqlite(
    path: &Path,
    migrator: &Migrator,
    telemetry_override: Option<&dyn DbTelemetry>,
) -> anyhow::Result<SqlitePool> {
    open_sqlite(
        path,
        migrator,
        MEMORIES_DB,
        SqliteRuntimeMode::default(),
        /*reconcile_vl_legacy*/ false,
        telemetry_override,
    )
    .await
}

async fn open_sqlite(
    path: &Path,
    migrator: &Migrator,
    spec: RuntimeDbSpec,
    mode: SqliteRuntimeMode,
    reconcile_vl_legacy: bool,
    telemetry_override: Option<&dyn DbTelemetry>,
) -> anyhow::Result<SqlitePool> {
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
    let pool = pool_result?;
    let started = Instant::now();
    let migrate_result = async {
        if reconcile_vl_legacy {
            reconcile_vl_legacy_state_migrations(&pool, migrator).await?;
        }
        migrator.run(&pool).await.map_err(anyhow::Error::from)?;
        Ok(())
    }
    .await;
    crate::telemetry::record_init_result(
        telemetry_override,
        spec.kind,
        spec.migrate_phase,
        started.elapsed(),
        &migrate_result,
    );
    migrate_result?;
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

pub fn state_db_filename() -> String {
    STATE_DB.filename.to_string()
}

pub fn state_db_path(codex_home: &Path) -> PathBuf {
    STATE_DB.path(codex_home)
}

pub fn logs_db_filename() -> String {
    LOGS_DB.filename.to_string()
}

pub fn logs_db_path(codex_home: &Path) -> PathBuf {
    LOGS_DB.path(codex_home)
}

pub fn goals_db_filename() -> String {
    GOALS_DB.filename.to_string()
}

pub fn goals_db_path(codex_home: &Path) -> PathBuf {
    GOALS_DB.path(codex_home)
}

pub fn memories_db_filename() -> String {
    MEMORIES_DB.filename.to_string()
}

pub fn memories_db_path(codex_home: &Path) -> PathBuf {
    MEMORIES_DB.path(codex_home)
}

pub fn runtime_db_paths(codex_home: &Path) -> Vec<RuntimeDbPath> {
    RUNTIME_DBS
        .iter()
        .map(|spec| RuntimeDbPath {
            label: spec.label,
            path: spec.path(codex_home),
        })
        .collect()
}

/// Run SQLite's built-in integrity check against an existing database file.
pub async fn sqlite_integrity_check(path: &Path) -> anyhow::Result<Vec<String>> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true)
        .log_statements(LevelFilter::Off);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let rows = sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
        .fetch_all(&pool)
        .await?;
    pool.close().await;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::SqliteJournalMode;
    use super::SqliteRuntimeMode;
    use super::StateRuntime;
    use super::open_state_sqlite;
    use super::runtime_state_migrator;
    use super::sqlite_integrity_check;
    use super::state_db_path;
    use super::test_support::unique_temp_dir;
    use crate::DB_INIT_METRIC;
    use crate::DbTelemetry;
    use crate::migrations::STATE_MIGRATOR;
    use pretty_assertions::assert_eq;
    use sqlx::SqlitePool;
    use sqlx::migrate::MigrateError;
    use sqlx::sqlite::SqliteConnectOptions;
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::Mutex;

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
    }

    fn tags_to_map(tags: &[(&str, &str)]) -> BTreeMap<String, String> {
        tags.iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

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
    async fn sqlite_integrity_check_reports_ok_for_valid_db() {
        let codex_home = unique_temp_dir();
        tokio::fs::create_dir_all(&codex_home)
            .await
            .expect("create codex home");
        let path = state_db_path(codex_home.as_path());
        let pool = SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .expect("open sqlite db");
        sqlx::query("CREATE TABLE sample (id INTEGER PRIMARY KEY)")
            .execute(&pool)
            .await
            .expect("create sample table");
        pool.close().await;

        let result = sqlite_integrity_check(&path)
            .await
            .expect("integrity check should run");

        assert_eq!(result, vec!["ok".to_string()]);
        let _ = tokio::fs::remove_dir_all(codex_home).await;
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
            /*telemetry_override*/ None,
        )
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
            codex_home.clone(),
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
            "ensure_backfill_state",
            "post_init_query",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
        assert_eq!(phases, expected);

        runtime.pool.close().await;
        runtime.logs_pool.close().await;
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
        let pool = open_state_sqlite(state_path.as_path(), &migrator, None)
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
        let pool = open_state_sqlite(state_path.as_path(), &migrator, None)
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
        assert_eq!(vl_versions, BTreeSet::from([930_i64, 931_i64]));

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
    }
}
