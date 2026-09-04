use super::*;
use crate::THREAD_LOOP_OWNER_KIND_MAIN;
use crate::ThreadLoopJob;
use crate::ThreadLoopJobCreateParams;
use crate::ThreadLoopJobRuntimeUpdate;
use crate::ThreadLoopOwner;

impl StateRuntime {
    pub async fn get_thread_loop_owner(
        &self,
        thread_id: ThreadId,
    ) -> anyhow::Result<ThreadLoopOwner> {
        let row = sqlx::query(
            r#"
SELECT
    thread_id,
    owner_kind,
    owner_vivling_id,
    updated_at_ms
FROM vl_thread_loop_owners
WHERE thread_id = ?
            "#,
        )
        .bind(thread_id.to_string())
        .fetch_optional(self.pool.as_ref())
        .await?;

        row.map(|row| {
            crate::model::ThreadLoopOwnerRow::try_from_row(&row)
                .and_then(crate::ThreadLoopOwner::try_from)
        })
        .transpose()
        .map(|owner| {
            owner.unwrap_or(ThreadLoopOwner {
                thread_id,
                owner_kind: THREAD_LOOP_OWNER_KIND_MAIN.to_string(),
                owner_vivling_id: None,
                updated_at_ms: 0,
            })
        })
    }

    pub async fn set_thread_loop_owner(
        &self,
        owner: ThreadLoopOwner,
    ) -> anyhow::Result<ThreadLoopOwner> {
        sqlx::query(
            r#"
INSERT INTO vl_thread_loop_owners (
    thread_id,
    owner_kind,
    owner_vivling_id,
    updated_at_ms
) VALUES (?, ?, ?, ?)
ON CONFLICT(thread_id) DO UPDATE SET
    owner_kind = excluded.owner_kind,
    owner_vivling_id = excluded.owner_vivling_id,
    updated_at_ms = excluded.updated_at_ms
            "#,
        )
        .bind(owner.thread_id.to_string())
        .bind(&owner.owner_kind)
        .bind(&owner.owner_vivling_id)
        .bind(owner.updated_at_ms)
        .execute(self.pool.as_ref())
        .await?;

        self.get_thread_loop_owner(owner.thread_id).await
    }

    pub async fn create_or_replace_thread_loop_job(
        &self,
        params: ThreadLoopJobCreateParams,
    ) -> anyhow::Result<ThreadLoopJob> {
        let ThreadLoopJobCreateParams {
            id,
            thread_id,
            label,
            prompt_text,
            goal_text,
            interval_seconds,
            enabled,
            run_policy,
            auto_remove_on_completion,
            created_by,
            next_run_ms,
            created_at_ms,
            updated_at_ms,
        } = params;
        sqlx::query(
            r#"
INSERT INTO vl_thread_loop_jobs (
    id,
    thread_id,
    label,
    prompt_text,
    goal_text,
    interval_seconds,
    enabled,
    run_policy,
    auto_remove_on_completion,
    created_by,
    next_run_ms,
    last_run_ms,
    last_status,
    last_error,
    pending_tick,
    created_at_ms,
    updated_at_ms
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, 0, ?, ?)
ON CONFLICT(thread_id, label) DO UPDATE SET
    prompt_text = excluded.prompt_text,
    goal_text = excluded.goal_text,
    interval_seconds = excluded.interval_seconds,
    enabled = excluded.enabled,
    run_policy = excluded.run_policy,
    auto_remove_on_completion = excluded.auto_remove_on_completion,
    created_by = excluded.created_by,
    next_run_ms = excluded.next_run_ms,
    last_run_ms = NULL,
    last_status = NULL,
    last_error = NULL,
    pending_tick = 0,
    created_at_ms = excluded.created_at_ms,
    updated_at_ms = excluded.updated_at_ms
            "#,
        )
        .bind(&id)
        .bind(thread_id.to_string())
        .bind(&label)
        .bind(prompt_text)
        .bind(goal_text)
        .bind(interval_seconds)
        .bind(enabled)
        .bind(run_policy)
        .bind(auto_remove_on_completion)
        .bind(created_by)
        .bind(next_run_ms)
        .bind(created_at_ms)
        .bind(updated_at_ms)
        .execute(self.pool.as_ref())
        .await?;

        let job = self
            .get_thread_loop_job_by_label(thread_id, &label)
            .await?
            .ok_or_else(|| anyhow::anyhow!("loop job disappeared after upsert"))?;
        // Every persisted loop has one descriptor. Preserve an existing
        // descriptor when replacing the job by label.
        sqlx::query(
            r#"
INSERT INTO vl_loop_descriptors (job_id, updated_at_ms)
VALUES (?, ?)
ON CONFLICT(job_id) DO NOTHING
            "#,
        )
        .bind(&job.id)
        .bind(updated_at_ms)
        .execute(self.pool.as_ref())
        .await?;
        Ok(job)
    }

    pub async fn list_thread_loop_jobs(
        &self,
        thread_id: ThreadId,
    ) -> anyhow::Result<Vec<ThreadLoopJob>> {
        let rows = sqlx::query(
            r#"
SELECT
    id,
    thread_id,
    label,
    prompt_text,
    goal_text,
    interval_seconds,
    enabled,
    run_policy,
    auto_remove_on_completion,
    created_by,
    next_run_ms,
    last_run_ms,
    last_status,
    last_error,
    pending_tick,
    created_at_ms,
    updated_at_ms
FROM vl_thread_loop_jobs
WHERE thread_id = ?
ORDER BY label ASC
            "#,
        )
        .bind(thread_id.to_string())
        .fetch_all(self.pool.as_ref())
        .await?;

        rows.into_iter()
            .map(|row| {
                crate::model::ThreadLoopJobRow::try_from_row(&row)
                    .and_then(crate::ThreadLoopJob::try_from)
            })
            .collect()
    }

    pub async fn get_thread_loop_job_by_label(
        &self,
        thread_id: ThreadId,
        label: &str,
    ) -> anyhow::Result<Option<ThreadLoopJob>> {
        let row = sqlx::query(
            r#"
SELECT
    id,
    thread_id,
    label,
    prompt_text,
    goal_text,
    interval_seconds,
    enabled,
    run_policy,
    auto_remove_on_completion,
    created_by,
    next_run_ms,
    last_run_ms,
    last_status,
    last_error,
    pending_tick,
    created_at_ms,
    updated_at_ms
FROM vl_thread_loop_jobs
WHERE thread_id = ? AND label = ?
            "#,
        )
        .bind(thread_id.to_string())
        .bind(label)
        .fetch_optional(self.pool.as_ref())
        .await?;

        row.map(|row| {
            crate::model::ThreadLoopJobRow::try_from_row(&row)
                .and_then(crate::ThreadLoopJob::try_from)
        })
        .transpose()
    }

    pub async fn get_thread_loop_job_by_id(
        &self,
        thread_id: ThreadId,
        job_id: &str,
    ) -> anyhow::Result<Option<ThreadLoopJob>> {
        let row = sqlx::query(
            r#"
SELECT
    id,
    thread_id,
    label,
    prompt_text,
    goal_text,
    interval_seconds,
    enabled,
    run_policy,
    auto_remove_on_completion,
    created_by,
    next_run_ms,
    last_run_ms,
    last_status,
    last_error,
    pending_tick,
    created_at_ms,
    updated_at_ms
FROM vl_thread_loop_jobs
WHERE thread_id = ? AND id = ?
            "#,
        )
        .bind(thread_id.to_string())
        .bind(job_id)
        .fetch_optional(self.pool.as_ref())
        .await?;

        row.map(|row| {
            crate::model::ThreadLoopJobRow::try_from_row(&row)
                .and_then(crate::ThreadLoopJob::try_from)
        })
        .transpose()
    }

    pub async fn set_thread_loop_job_enabled(
        &self,
        thread_id: ThreadId,
        label: &str,
        enabled: bool,
        next_run_ms: Option<i64>,
        updated_at_ms: i64,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            r#"
UPDATE vl_thread_loop_jobs
SET enabled = ?, next_run_ms = ?, pending_tick = 0, updated_at_ms = ?
WHERE thread_id = ? AND label = ?
            "#,
        )
        .bind(enabled)
        .bind(next_run_ms)
        .bind(updated_at_ms)
        .bind(thread_id.to_string())
        .bind(label)
        .execute(self.pool.as_ref())
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_thread_loop_job(
        &self,
        thread_id: ThreadId,
        label: &str,
    ) -> anyhow::Result<bool> {
        let result =
            sqlx::query("DELETE FROM vl_thread_loop_jobs WHERE thread_id = ? AND label = ?")
                .bind(thread_id.to_string())
                .bind(label)
                .execute(self.pool.as_ref())
                .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Remove a loop and its loop-owned children as one SQLite transaction.
    /// The explicit deletes keep cleanup atomic even when a connection has
    /// foreign-key enforcement disabled; the migration FK remains a second
    /// integrity boundary. Occurrence rows (0934) are deleted explicitly too:
    /// they are children of the job, so a removal must leave no orphan
    /// occurrence behind regardless of FK enforcement.
    pub async fn delete_thread_loop_job_with_dependents(
        &self,
        thread_id: ThreadId,
        label: &str,
    ) -> anyhow::Result<bool> {
        let mut transaction = self.pool.begin().await?;
        let job_id = sqlx::query_scalar::<_, String>(
            "SELECT id FROM vl_thread_loop_jobs WHERE thread_id = ? AND label = ?",
        )
        .bind(thread_id.to_string())
        .bind(label)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(job_id) = job_id else {
            return Ok(false);
        };
        sqlx::query("DELETE FROM vl_loop_delegations WHERE thread_id = ? AND job_id = ?")
            .bind(thread_id.to_string())
            .bind(&job_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM vl_loop_occurrences WHERE job_id = ?")
            .bind(&job_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM vl_loop_descriptors WHERE job_id = ?")
            .bind(&job_id)
            .execute(&mut *transaction)
            .await?;
        let result = sqlx::query("DELETE FROM vl_thread_loop_jobs WHERE thread_id = ? AND id = ?")
            .bind(thread_id.to_string())
            .bind(job_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn update_thread_loop_job_runtime(
        &self,
        thread_id: ThreadId,
        job_id: &str,
        update: ThreadLoopJobRuntimeUpdate,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            r#"
UPDATE vl_thread_loop_jobs
SET next_run_ms = ?,
    last_run_ms = ?,
    last_status = ?,
    last_error = ?,
    pending_tick = ?,
    updated_at_ms = ?
WHERE thread_id = ? AND id = ?
            "#,
        )
        .bind(update.next_run_ms)
        .bind(update.last_run_ms)
        .bind(update.last_status)
        .bind(update.last_error)
        .bind(update.pending_tick)
        .bind(update.updated_at_ms)
        .bind(thread_id.to_string())
        .bind(job_id)
        .execute(self.pool.as_ref())
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::test_support::unique_temp_dir;
    use codex_utils_absolute_path::test_support::PathExt;

    #[tokio::test]
    async fn thread_loop_job_crud_roundtrip() -> anyhow::Result<()> {
        let codex_home = unique_temp_dir();
        let runtime = StateRuntime::init(
            crate::SqliteConfig::new_for_testing(codex_home.as_path().abs()),
            "test-provider".to_string(),
        )
        .await?;
        let thread_id = ThreadId::new();
        let now = 1_700_000_000_000i64;

        let created = runtime
            .create_or_replace_thread_loop_job(ThreadLoopJobCreateParams {
                id: "job-1".to_string(),
                thread_id,
                label: "ci".to_string(),
                prompt_text: "check ci".to_string(),
                goal_text: Some("monitor ci".to_string()),
                interval_seconds: 300,
                enabled: true,
                run_policy: "queue_one".to_string(),
                auto_remove_on_completion: true,
                created_by: "agent".to_string(),
                next_run_ms: Some(now + 300_000),
                created_at_ms: now,
                updated_at_ms: now,
            })
            .await?;
        assert_eq!(created.label, "ci");
        assert!(created.enabled);

        let listed = runtime.list_thread_loop_jobs(thread_id).await?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].prompt_text, "check ci");
        assert_eq!(listed[0].goal_text.as_deref(), Some("monitor ci"));
        assert!(listed[0].auto_remove_on_completion);
        assert_eq!(listed[0].created_by, "agent");

        assert!(
            runtime
                .set_thread_loop_job_enabled(thread_id, "ci", false, None, now + 1)
                .await?
        );
        let updated = runtime
            .get_thread_loop_job_by_label(thread_id, "ci")
            .await?
            .expect("job should exist");
        assert!(!updated.enabled);

        assert!(
            runtime
                .update_thread_loop_job_runtime(
                    thread_id,
                    "job-1",
                    ThreadLoopJobRuntimeUpdate {
                        next_run_ms: None,
                        last_run_ms: Some(now + 2),
                        last_status: Some("pending".to_string()),
                        last_error: None,
                        pending_tick: true,
                        updated_at_ms: now + 2,
                    },
                )
                .await?
        );
        let updated = runtime
            .get_thread_loop_job_by_id(thread_id, "job-1")
            .await?
            .expect("job should exist");
        assert!(updated.pending_tick);
        assert_eq!(updated.last_status.as_deref(), Some("pending"));

        // occurrence accounting (0934) is job-owned: a second job
        // with its own occurrence proves the cleanup removes only the
        // removed job's occurrences, not the whole table.
        runtime
            .claim_loop_occurrence("job-1", now + 300_000, now + 3)
            .await?;
        runtime
            .create_or_replace_thread_loop_job(ThreadLoopJobCreateParams {
                id: "job-2".to_string(),
                thread_id,
                label: "other".to_string(),
                prompt_text: "check other".to_string(),
                goal_text: None,
                interval_seconds: 300,
                enabled: true,
                run_policy: "queue_one".to_string(),
                auto_remove_on_completion: true,
                created_by: "agent".to_string(),
                next_run_ms: Some(now + 300_000),
                created_at_ms: now,
                updated_at_ms: now,
            })
            .await?;
        runtime
            .claim_loop_occurrence("job-2", now + 300_000, now + 3)
            .await?;

        assert!(
            runtime
                .delete_thread_loop_job_with_dependents(thread_id, "ci")
                .await?
        );
        let remaining = runtime.list_thread_loop_jobs(thread_id).await?;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "job-2");
        assert!(runtime.get_loop_descriptor("job-1").await?.is_none());
        // No orphan occurrence rows may survive for the removed job, while
        // the other job's occurrence stays untouched.
        assert!(runtime.list_loop_occurrences("job-1").await?.is_empty());
        assert_eq!(runtime.list_loop_occurrences("job-2").await?.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn replace_by_label_resets_runtime_metadata() -> anyhow::Result<()> {
        let codex_home = unique_temp_dir();
        let runtime = StateRuntime::init(
            crate::SqliteConfig::new_for_testing(codex_home.as_path().abs()),
            "test-provider".to_string(),
        )
        .await?;
        let thread_id = ThreadId::new();
        let now = 1_700_000_000_000i64;

        runtime
            .create_or_replace_thread_loop_job(ThreadLoopJobCreateParams {
                id: "job-1".to_string(),
                thread_id,
                label: "ci".to_string(),
                prompt_text: "check ci".to_string(),
                goal_text: Some("monitor ci".to_string()),
                interval_seconds: 300,
                enabled: true,
                run_policy: "queue_one".to_string(),
                auto_remove_on_completion: true,
                created_by: "agent".to_string(),
                next_run_ms: Some(now + 300_000),
                created_at_ms: now,
                updated_at_ms: now,
            })
            .await?;

        runtime
            .update_thread_loop_job_runtime(
                thread_id,
                "job-1",
                ThreadLoopJobRuntimeUpdate {
                    next_run_ms: None,
                    last_run_ms: Some(now + 5),
                    last_status: Some("submitted".to_string()),
                    last_error: Some("timeout".to_string()),
                    pending_tick: true,
                    updated_at_ms: now + 5,
                },
            )
            .await?;

        let replaced = runtime
            .create_or_replace_thread_loop_job(ThreadLoopJobCreateParams {
                id: "job-2".to_string(),
                thread_id,
                label: "ci".to_string(),
                prompt_text: "check ci again".to_string(),
                goal_text: Some("monitor ci again".to_string()),
                interval_seconds: 600,
                enabled: true,
                run_policy: "queue_one".to_string(),
                auto_remove_on_completion: false,
                created_by: "user".to_string(),
                next_run_ms: Some(now + 600_000),
                created_at_ms: now + 10,
                updated_at_ms: now + 10,
            })
            .await?;

        assert_eq!(replaced.id, "job-1");
        assert_eq!(replaced.prompt_text, "check ci again");
        assert_eq!(replaced.goal_text.as_deref(), Some("monitor ci again"));
        assert_eq!(replaced.interval_seconds, 600);
        assert!(!replaced.auto_remove_on_completion);
        assert_eq!(replaced.created_by, "user");
        assert_eq!(replaced.next_run_ms, Some(now + 600_000));
        assert_eq!(replaced.last_run_ms, None);
        assert_eq!(replaced.last_status, None);
        assert_eq!(replaced.last_error, None);
        assert!(!replaced.pending_tick);
        assert_eq!(replaced.created_at_ms, now + 10);
        assert_eq!(replaced.updated_at_ms, now + 10);
        let descriptor = runtime
            .get_loop_descriptor("job-1")
            .await?
            .expect("replace should preserve the descriptor key");
        assert_eq!(descriptor.job_id, "job-1");
        Ok(())
    }

    #[tokio::test]
    async fn loop_tick_claim_is_atomic_and_reusable_after_finish() -> anyhow::Result<()> {
        let codex_home = unique_temp_dir();
        let runtime = StateRuntime::init(
            crate::SqliteConfig::new_for_testing(codex_home.as_path().abs()),
            "test-provider".to_string(),
        )
        .await?;
        let thread_id = ThreadId::new();
        runtime
            .create_or_replace_thread_loop_job(ThreadLoopJobCreateParams {
                id: "job-atomic".to_string(),
                thread_id,
                label: "atomic".to_string(),
                prompt_text: "tick".to_string(),
                goal_text: None,
                interval_seconds: 30,
                enabled: true,
                run_policy: "queue_one".to_string(),
                auto_remove_on_completion: true,
                created_by: "agent".to_string(),
                next_run_ms: Some(1_000),
                created_at_ms: 1_000,
                updated_at_ms: 1_000,
            })
            .await?;

        assert!(runtime.try_begin_loop_tick("job-atomic", 2_000).await?);
        assert!(!runtime.try_begin_loop_tick("job-atomic", 2_001).await?);
        assert!(runtime.finish_loop_tick("job-atomic", 2_002).await?);
        assert!(runtime.try_begin_loop_tick("job-atomic", 2_003).await?);
        assert!(runtime.finish_loop_tick("job-atomic", 2_004).await?);
        assert!(!runtime.finish_loop_tick("job-atomic", 2_005).await?);
        Ok(())
    }
}
