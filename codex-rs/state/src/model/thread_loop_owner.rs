use codex_protocol::ThreadId;
use sqlx::Row;

pub const THREAD_LOOP_OWNER_KIND_MAIN: &str = "main";
pub const THREAD_LOOP_OWNER_KIND_VIVLING: &str = "vivling";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadLoopOwner {
    pub thread_id: ThreadId,
    pub owner_kind: String,
    pub owner_vivling_id: Option<String>,
    pub updated_at_ms: i64,
}

pub(crate) struct ThreadLoopOwnerRow {
    pub(crate) thread_id: String,
    pub(crate) owner_kind: String,
    pub(crate) owner_vivling_id: Option<String>,
    pub(crate) updated_at_ms: i64,
}

impl ThreadLoopOwnerRow {
    pub(crate) fn try_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<Self> {
        Ok(Self {
            thread_id: row.try_get("thread_id")?,
            owner_kind: row.try_get("owner_kind")?,
            owner_vivling_id: row.try_get("owner_vivling_id")?,
            updated_at_ms: row.try_get("updated_at_ms")?,
        })
    }
}

impl TryFrom<ThreadLoopOwnerRow> for ThreadLoopOwner {
    type Error = anyhow::Error;

    fn try_from(value: ThreadLoopOwnerRow) -> Result<Self, Self::Error> {
        Ok(Self {
            thread_id: ThreadId::from_string(&value.thread_id)?,
            owner_kind: value.owner_kind,
            owner_vivling_id: value.owner_vivling_id,
            updated_at_ms: value.updated_at_ms,
        })
    }
}
