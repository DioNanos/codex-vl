use codex_protocol::ThreadId;
use sqlx::Row;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadLoopJob {
    pub id: String,
    pub thread_id: ThreadId,
    pub label: String,
    pub prompt_text: String,
    pub goal_text: Option<String>,
    pub interval_seconds: i64,
    pub enabled: bool,
    pub run_policy: String,
    pub auto_remove_on_completion: bool,
    pub created_by: String,
    pub next_run_ms: Option<i64>,
    pub last_run_ms: Option<i64>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub pending_tick: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadLoopJobCreateParams {
    pub id: String,
    pub thread_id: ThreadId,
    pub label: String,
    pub prompt_text: String,
    pub goal_text: Option<String>,
    pub interval_seconds: i64,
    pub enabled: bool,
    pub run_policy: String,
    pub auto_remove_on_completion: bool,
    pub created_by: String,
    pub next_run_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadLoopJobRuntimeUpdate {
    pub next_run_ms: Option<i64>,
    pub last_run_ms: Option<i64>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub pending_tick: bool,
    pub updated_at_ms: i64,
}

pub(crate) struct ThreadLoopJobRow {
    pub(crate) id: String,
    pub(crate) thread_id: String,
    pub(crate) label: String,
    pub(crate) prompt_text: String,
    pub(crate) goal_text: Option<String>,
    pub(crate) interval_seconds: i64,
    pub(crate) enabled: bool,
    pub(crate) run_policy: String,
    pub(crate) auto_remove_on_completion: bool,
    pub(crate) created_by: String,
    pub(crate) next_run_ms: Option<i64>,
    pub(crate) last_run_ms: Option<i64>,
    pub(crate) last_status: Option<String>,
    pub(crate) last_error: Option<String>,
    pub(crate) pending_tick: bool,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
}

impl ThreadLoopJobRow {
    pub(crate) fn try_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            thread_id: row.try_get("thread_id")?,
            label: row.try_get("label")?,
            prompt_text: row.try_get("prompt_text")?,
            goal_text: row.try_get("goal_text")?,
            interval_seconds: row.try_get("interval_seconds")?,
            enabled: row.try_get("enabled")?,
            run_policy: row.try_get("run_policy")?,
            auto_remove_on_completion: row.try_get("auto_remove_on_completion")?,
            created_by: row.try_get("created_by")?,
            next_run_ms: row.try_get("next_run_ms")?,
            last_run_ms: row.try_get("last_run_ms")?,
            last_status: row.try_get("last_status")?,
            last_error: row.try_get("last_error")?,
            pending_tick: row.try_get("pending_tick")?,
            created_at_ms: row.try_get("created_at_ms")?,
            updated_at_ms: row.try_get("updated_at_ms")?,
        })
    }
}

impl TryFrom<ThreadLoopJobRow> for ThreadLoopJob {
    type Error = anyhow::Error;

    fn try_from(value: ThreadLoopJobRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            thread_id: ThreadId::from_string(&value.thread_id)?,
            label: value.label,
            prompt_text: value.prompt_text,
            goal_text: value.goal_text,
            interval_seconds: value.interval_seconds,
            enabled: value.enabled,
            run_policy: value.run_policy,
            auto_remove_on_completion: value.auto_remove_on_completion,
            created_by: value.created_by,
            next_run_ms: value.next_run_ms,
            last_run_ms: value.last_run_ms,
            last_status: value.last_status,
            last_error: value.last_error,
            pending_tick: value.pending_tick,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
        })
    }
}
