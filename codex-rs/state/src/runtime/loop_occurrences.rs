use super::*;
use crate::LoopOccurrence;

impl StateRuntime {
    /// Atomically claims the occurrence (job_id, scheduled_at_ms) BEFORE any
    /// dispatch. The INSERT OR IGNORE is the CAS over the
    /// occurrence key and `rows_affected` is the verdict: `true` = this timer
    /// owns the tick, `false` = the occurrence was already claimed and the
    /// second tick must skip.
    pub async fn claim_loop_occurrence(
        &self,
        job_id: &str,
        scheduled_at_ms: i64,
        now_ms: i64,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            r#"
INSERT OR IGNORE INTO vl_loop_occurrences
    (job_id, scheduled_at_ms, fired_count, last_fired_at_ms, claimed_at_ms)
VALUES (?, ?, 0, NULL, ?)
"#,
        )
        .bind(job_id)
        .bind(scheduled_at_ms)
        .bind(now_ms)
        .execute(self.pool.as_ref())
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Records the dispatch of a claimed occurrence. Accounting only: the
    /// claim above already guarantees at-most-once dispatch, so
    /// `fired_count` > 1 for one occurrence would mean the claim was
    /// bypassed (audited as a bug, never expected).
    pub async fn mark_loop_occurrence_fired(
        &self,
        job_id: &str,
        scheduled_at_ms: i64,
        now_ms: i64,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            r#"
UPDATE vl_loop_occurrences
SET fired_count = fired_count + 1, last_fired_at_ms = ?
WHERE job_id = ? AND scheduled_at_ms = ?
"#,
        )
        .bind(now_ms)
        .bind(job_id)
        .bind(scheduled_at_ms)
        .execute(self.pool.as_ref())
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Occurrence rows of a job, oldest first (riepilogo/audit).
    pub async fn list_loop_occurrences(&self, job_id: &str) -> anyhow::Result<Vec<LoopOccurrence>> {
        let rows = sqlx::query(
            r#"
SELECT job_id, scheduled_at_ms, fired_count, last_fired_at_ms, claimed_at_ms
FROM vl_loop_occurrences
WHERE job_id = ?
ORDER BY scheduled_at_ms ASC
"#,
        )
        .bind(job_id)
        .fetch_all(self.pool.as_ref())
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                Ok::<_, sqlx::Error>(LoopOccurrence {
                    job_id: row.try_get("job_id")?,
                    scheduled_at_ms: row.try_get("scheduled_at_ms")?,
                    fired_count: row.try_get("fired_count")?,
                    last_fired_at_ms: row.try_get("last_fired_at_ms")?,
                    claimed_at_ms: row.try_get("claimed_at_ms")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// Whether the occurrence (job_id, scheduled_at_ms) was already claimed
    /// (re-arm idempotency: a claimed occurrence dispatches at-most-once,
    /// so a schedule pointer onto it is dead and must never be re-armed
    /// as-is).
    pub async fn has_loop_occurrence(
        &self,
        job_id: &str,
        scheduled_at_ms: i64,
    ) -> anyhow::Result<bool> {
        let row: Option<(i64,)> = sqlx::query_as(
            r#"
SELECT 1 FROM vl_loop_occurrences
WHERE job_id = ? AND scheduled_at_ms = ?
LIMIT 1
"#,
        )
        .bind(job_id)
        .bind(scheduled_at_ms)
        .fetch_optional(self.pool.as_ref())
        .await?;
        Ok(row.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::test_support::unique_temp_dir;
    use codex_protocol::ThreadId;
    use codex_utils_absolute_path::test_support::PathExt;

    async fn runtime_with_job() -> anyhow::Result<(std::sync::Arc<StateRuntime>, String)> {
        let codex_home = unique_temp_dir();
        let runtime = StateRuntime::init(
            crate::SqliteConfig::new_for_testing(codex_home.as_path().abs()),
            "test-provider".to_string(),
        )
        .await?;
        let thread_id = ThreadId::new();
        let job_id = "job-occ-1".to_string();
        runtime
            .create_or_replace_thread_loop_job(crate::ThreadLoopJobCreateParams {
                id: job_id.clone(),
                thread_id,
                label: "ci-watch".to_string(),
                prompt_text: "check".to_string(),
                goal_text: None,
                interval_seconds: 60,
                enabled: true,
                run_policy: "queue_one".to_string(),
                auto_remove_on_completion: true,
                created_by: "user".to_string(),
                next_run_ms: None,
                created_at_ms: 1_700_000_000_000,
                updated_at_ms: 1_700_000_000_000,
            })
            .await?;
        Ok((runtime, job_id))
    }

    #[tokio::test]
    async fn claim_is_atomic_and_marks_one_fire_per_occurrence() -> anyhow::Result<()> {
        let (runtime, job_id) = runtime_with_job().await?;
        let scheduled_at_ms: i64 = 1_700_000_100_000;

        // First timer claims the occurrence; the second one for the SAME
        // occurrence is refused by the (job_id, scheduled_at_ms) key.
        assert!(
            runtime
                .claim_loop_occurrence(&job_id, scheduled_at_ms, 1_700_000_100_100)
                .await?
        );
        assert!(
            !runtime
                .claim_loop_occurrence(&job_id, scheduled_at_ms, 1_700_000_100_200)
                .await?
        );

        // Dispatch accounting: exactly one fire, never a second one.
        assert!(
            runtime
                .mark_loop_occurrence_fired(&job_id, scheduled_at_ms, 1_700_000_100_300)
                .await?
        );
        let occurrences = runtime.list_loop_occurrences(&job_id).await?;
        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].scheduled_at_ms, scheduled_at_ms);
        assert_eq!(occurrences[0].fired_count, 1);
        assert_eq!(occurrences[0].last_fired_at_ms, Some(1_700_000_100_300));

        // Re-arm idempotency probe: a claimed occurrence is visible, a
        // never-claimed instant is not.
        assert!(
            runtime
                .has_loop_occurrence(&job_id, scheduled_at_ms)
                .await?
        );
        assert!(
            !runtime
                .has_loop_occurrence(&job_id, scheduled_at_ms + 30_000)
                .await?
        );

        // A distinct occurrence of the same job claims normally.
        assert!(
            runtime
                .claim_loop_occurrence(&job_id, scheduled_at_ms + 60_000, 1_700_000_160_000)
                .await?
        );
        assert_eq!(runtime.list_loop_occurrences(&job_id).await?.len(), 2);
        Ok(())
    }
}
