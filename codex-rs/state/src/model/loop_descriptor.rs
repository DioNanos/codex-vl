use sqlx::Row;

pub const LOOP_RUNNER_KIND_MAIN: &str = "main";
pub const LOOP_RUNNER_KIND_CHILD_AGENT: &str = "child_agent";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopRunnerKind {
    Main,
    ChildAgent,
}

impl LoopRunnerKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Main => LOOP_RUNNER_KIND_MAIN,
            Self::ChildAgent => LOOP_RUNNER_KIND_CHILD_AGENT,
        }
    }
}

impl TryFrom<&str> for LoopRunnerKind {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            LOOP_RUNNER_KIND_MAIN => Ok(Self::Main),
            LOOP_RUNNER_KIND_CHILD_AGENT => Ok(Self::ChildAgent),
            other => Err(anyhow::anyhow!("unknown loop runner kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopDescriptor {
    pub job_id: String,
    pub runner_kind: LoopRunnerKind,
    pub runner_model: Option<String>,
    pub runner_reasoning_effort: Option<String>,
    pub tz: Option<String>,
    pub schedule_kind: String,
    pub schedule_at: Option<String>,
    pub one_shot_at_ms: Option<i64>,
    pub rearm_on_boot: bool,
    pub in_flight: bool,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopDescriptorUpsertParams {
    pub job_id: String,
    pub runner_kind: LoopRunnerKind,
    pub runner_model: Option<String>,
    pub runner_reasoning_effort: Option<String>,
    pub tz: Option<String>,
    pub schedule_kind: String,
    pub schedule_at: Option<String>,
    pub one_shot_at_ms: Option<i64>,
    pub rearm_on_boot: bool,
    pub updated_at_ms: i64,
}

pub(crate) struct LoopDescriptorRow {
    pub(crate) job_id: String,
    pub(crate) runner_kind: String,
    pub(crate) runner_model: Option<String>,
    pub(crate) runner_reasoning_effort: Option<String>,
    pub(crate) tz: Option<String>,
    pub(crate) schedule_kind: String,
    pub(crate) schedule_at: Option<String>,
    pub(crate) one_shot_at_ms: Option<i64>,
    pub(crate) rearm_on_boot: bool,
    pub(crate) in_flight: bool,
    pub(crate) updated_at_ms: i64,
}

impl LoopDescriptorRow {
    pub(crate) fn try_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<Self> {
        Ok(Self {
            job_id: row.try_get("job_id")?,
            runner_kind: row.try_get("runner_kind")?,
            runner_model: row.try_get("runner_model")?,
            runner_reasoning_effort: row.try_get("runner_reasoning_effort")?,
            tz: row.try_get("tz")?,
            schedule_kind: row.try_get("schedule_kind")?,
            schedule_at: row.try_get("schedule_at")?,
            one_shot_at_ms: row.try_get("one_shot_at_ms")?,
            rearm_on_boot: row.try_get("rearm_on_boot")?,
            in_flight: row.try_get("in_flight")?,
            updated_at_ms: row.try_get("updated_at_ms")?,
        })
    }
}

impl TryFrom<LoopDescriptorRow> for LoopDescriptor {
    type Error = anyhow::Error;

    fn try_from(value: LoopDescriptorRow) -> Result<Self, Self::Error> {
        Ok(Self {
            job_id: value.job_id,
            runner_kind: LoopRunnerKind::try_from(value.runner_kind.as_str())?,
            runner_model: value.runner_model,
            runner_reasoning_effort: value.runner_reasoning_effort,
            tz: value.tz,
            schedule_kind: value.schedule_kind,
            schedule_at: value.schedule_at,
            one_shot_at_ms: value.one_shot_at_ms,
            rearm_on_boot: value.rearm_on_boot,
            in_flight: value.in_flight,
            updated_at_ms: value.updated_at_ms,
        })
    }
}
