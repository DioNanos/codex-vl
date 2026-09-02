use super::*;
use crate::LoopDescriptor;
use crate::LoopDescriptorUpsertParams;

impl StateRuntime {
    pub async fn get_loop_descriptor(
        &self,
        job_id: &str,
    ) -> anyhow::Result<Option<LoopDescriptor>> {
        let row = sqlx::query(
            r#"
SELECT job_id, runner_kind, runner_model, runner_reasoning_effort, tz,
       schedule_kind, schedule_at, one_shot_at_ms, rearm_on_boot, in_flight,
       updated_at_ms
FROM vl_loop_descriptors
WHERE job_id = ?
            "#,
        )
        .bind(job_id)
        .fetch_optional(self.pool.as_ref())
        .await?;

        row.map(|row| {
            crate::model::LoopDescriptorRow::try_from_row(&row)
                .and_then(crate::LoopDescriptor::try_from)
        })
        .transpose()
    }

    pub async fn upsert_loop_descriptor(
        &self,
        params: LoopDescriptorUpsertParams,
    ) -> anyhow::Result<LoopDescriptor> {
        sqlx::query(
            r#"
INSERT INTO vl_loop_descriptors (
    job_id, runner_kind, runner_model, runner_reasoning_effort, tz,
    schedule_kind, schedule_at, one_shot_at_ms, rearm_on_boot, updated_at_ms
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(job_id) DO UPDATE SET
    runner_kind = excluded.runner_kind,
    runner_model = excluded.runner_model,
    runner_reasoning_effort = excluded.runner_reasoning_effort,
    tz = excluded.tz,
    schedule_kind = excluded.schedule_kind,
    schedule_at = excluded.schedule_at,
    one_shot_at_ms = excluded.one_shot_at_ms,
    rearm_on_boot = excluded.rearm_on_boot,
    updated_at_ms = excluded.updated_at_ms
            "#,
        )
        .bind(&params.job_id)
        .bind(params.runner_kind.as_str())
        .bind(&params.runner_model)
        .bind(&params.runner_reasoning_effort)
        .bind(&params.tz)
        .bind(&params.schedule_kind)
        .bind(&params.schedule_at)
        .bind(params.one_shot_at_ms)
        .bind(params.rearm_on_boot)
        .bind(params.updated_at_ms)
        .execute(self.pool.as_ref())
        .await?;

        self.get_loop_descriptor(&params.job_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("loop descriptor disappeared after upsert"))
    }

    pub async fn delete_loop_descriptor(&self, job_id: &str) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM vl_loop_descriptors WHERE job_id = ?")
            .bind(job_id)
            .execute(self.pool.as_ref())
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Atomically claims one tick for a job before any runner is spawned.
    pub async fn try_begin_loop_tick(&self, job_id: &str, now_ms: i64) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE vl_loop_descriptors SET in_flight = 1, updated_at_ms = ? WHERE job_id = ? AND in_flight = 0",
        )
        .bind(now_ms)
        .bind(job_id)
        .execute(self.pool.as_ref())
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Releases the per-job tick guard on every terminal runner outcome.
    pub async fn finish_loop_tick(&self, job_id: &str, now_ms: i64) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE vl_loop_descriptors SET in_flight = 0, updated_at_ms = ? WHERE job_id = ? AND in_flight = 1",
        )
        .bind(now_ms)
        .bind(job_id)
        .execute(self.pool.as_ref())
        .await?;
        Ok(result.rows_affected() == 1)
    }
}
