use codex_protocol::ThreadId;
use sqlx::Row;

pub const LOOP_DELEGATION_STRATEGY_OBSERVE: &str = "observe";
pub const LOOP_DELEGATION_STRATEGY_SUGGEST: &str = "suggest";
pub const LOOP_DELEGATION_STRATEGY_MANAGE: &str = "manage";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopDelegationStrategy {
    Observe,
    Suggest,
    Manage,
}

impl LoopDelegationStrategy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observe => LOOP_DELEGATION_STRATEGY_OBSERVE,
            Self::Suggest => LOOP_DELEGATION_STRATEGY_SUGGEST,
            Self::Manage => LOOP_DELEGATION_STRATEGY_MANAGE,
        }
    }
}

impl TryFrom<&str> for LoopDelegationStrategy {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            LOOP_DELEGATION_STRATEGY_OBSERVE => Ok(Self::Observe),
            LOOP_DELEGATION_STRATEGY_SUGGEST => Ok(Self::Suggest),
            LOOP_DELEGATION_STRATEGY_MANAGE => Ok(Self::Manage),
            other => Err(anyhow::anyhow!("unknown loop delegation strategy: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopDelegation {
    pub thread_id: ThreadId,
    pub job_id: String,
    pub loop_label: String,
    pub vivling_id: String,
    pub strategy: LoopDelegationStrategy,
    pub ticks_managed: i64,
    pub recent_results_json: String,
    pub last_plan_approved: Option<bool>,
    pub strategy_override: Option<LoopDelegationStrategy>,
    pub override_main: bool,
    pub cooldown_until_ms: Option<i64>,
    pub suspend_reason: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopDelegationUpsertParams {
    pub thread_id: ThreadId,
    pub job_id: String,
    pub loop_label: String,
    pub vivling_id: String,
    pub strategy: LoopDelegationStrategy,
    pub ticks_managed: i64,
    pub recent_results_json: String,
    pub last_plan_approved: Option<bool>,
    pub strategy_override: Option<LoopDelegationStrategy>,
    pub override_main: bool,
    pub cooldown_until_ms: Option<i64>,
    pub suspend_reason: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

pub(crate) struct LoopDelegationRow {
    pub(crate) thread_id: String,
    pub(crate) job_id: String,
    pub(crate) loop_label: String,
    pub(crate) vivling_id: String,
    pub(crate) strategy: String,
    pub(crate) ticks_managed: i64,
    pub(crate) recent_results_json: String,
    pub(crate) last_plan_approved: Option<bool>,
    pub(crate) strategy_override: Option<String>,
    pub(crate) override_main: bool,
    pub(crate) cooldown_until_ms: Option<i64>,
    pub(crate) suspend_reason: Option<String>,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
}

impl LoopDelegationRow {
    pub(crate) fn try_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<Self> {
        Ok(Self {
            thread_id: row.try_get("thread_id")?,
            job_id: row.try_get("job_id")?,
            loop_label: row.try_get("loop_label")?,
            vivling_id: row.try_get("vivling_id")?,
            strategy: row.try_get("strategy")?,
            ticks_managed: row.try_get("ticks_managed")?,
            recent_results_json: row.try_get("recent_results_json")?,
            last_plan_approved: row.try_get("last_plan_approved")?,
            strategy_override: row.try_get("strategy_override")?,
            override_main: row.try_get("override_main")?,
            cooldown_until_ms: row.try_get("cooldown_until_ms")?,
            suspend_reason: row.try_get("suspend_reason")?,
            created_at_ms: row.try_get("created_at_ms")?,
            updated_at_ms: row.try_get("updated_at_ms")?,
        })
    }
}

impl TryFrom<LoopDelegationRow> for LoopDelegation {
    type Error = anyhow::Error;

    fn try_from(value: LoopDelegationRow) -> Result<Self, Self::Error> {
        Ok(Self {
            thread_id: ThreadId::from_string(&value.thread_id)?,
            job_id: value.job_id,
            loop_label: value.loop_label,
            vivling_id: value.vivling_id,
            strategy: LoopDelegationStrategy::try_from(value.strategy.as_str())?,
            ticks_managed: value.ticks_managed,
            recent_results_json: value.recent_results_json,
            last_plan_approved: value.last_plan_approved,
            strategy_override: value
                .strategy_override
                .as_deref()
                .map(LoopDelegationStrategy::try_from)
                .transpose()?,
            override_main: value.override_main,
            cooldown_until_ms: value.cooldown_until_ms,
            suspend_reason: value.suspend_reason,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
        })
    }
}
