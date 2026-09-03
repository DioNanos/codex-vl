/// One claimed loop occurrence (migration 0934). The (job_id,
/// scheduled_at_ms) pair is the occurrence key: claiming it is at-most-once
/// dispatch, and `fired_count` > 1 would mean the claim was bypassed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopOccurrence {
    pub job_id: String,
    pub scheduled_at_ms: i64,
    pub fired_count: i64,
    pub last_fired_at_ms: Option<i64>,
    pub claimed_at_ms: i64,
}
