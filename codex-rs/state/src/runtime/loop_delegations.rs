use super::*;
use crate::LoopDelegation;
use crate::LoopDelegationUpsertParams;

impl StateRuntime {
    pub async fn get_loop_delegation(
        &self,
        thread_id: ThreadId,
        job_id: &str,
    ) -> anyhow::Result<Option<LoopDelegation>> {
        let row = sqlx::query(
            r#"
SELECT
    thread_id,
    job_id,
    loop_label,
    vivling_id,
    strategy,
    ticks_managed,
    recent_results_json,
    last_plan_approved,
    strategy_override,
    override_main,
    cooldown_until_ms,
    suspend_reason,
    created_at_ms,
    updated_at_ms
FROM vl_loop_delegations
WHERE thread_id = ? AND job_id = ?
            "#,
        )
        .bind(thread_id.to_string())
        .bind(job_id)
        .fetch_optional(self.pool.as_ref())
        .await?;

        row.map(|row| {
            crate::model::LoopDelegationRow::try_from_row(&row)
                .and_then(crate::LoopDelegation::try_from)
        })
        .transpose()
    }

    pub async fn list_loop_delegations(
        &self,
        thread_id: ThreadId,
    ) -> anyhow::Result<Vec<LoopDelegation>> {
        let rows = sqlx::query(
            r#"
SELECT
    thread_id,
    job_id,
    loop_label,
    vivling_id,
    strategy,
    ticks_managed,
    recent_results_json,
    last_plan_approved,
    strategy_override,
    override_main,
    cooldown_until_ms,
    suspend_reason,
    created_at_ms,
    updated_at_ms
FROM vl_loop_delegations
WHERE thread_id = ?
ORDER BY loop_label ASC, job_id ASC
            "#,
        )
        .bind(thread_id.to_string())
        .fetch_all(self.pool.as_ref())
        .await?;

        rows.into_iter()
            .map(|row| {
                crate::model::LoopDelegationRow::try_from_row(&row)
                    .and_then(crate::LoopDelegation::try_from)
            })
            .collect()
    }

    pub async fn upsert_loop_delegation(
        &self,
        params: LoopDelegationUpsertParams,
    ) -> anyhow::Result<LoopDelegation> {
        sqlx::query(
            r#"
INSERT INTO vl_loop_delegations (
    thread_id,
    job_id,
    loop_label,
    vivling_id,
    strategy,
    ticks_managed,
    recent_results_json,
    last_plan_approved,
    strategy_override,
    override_main,
    cooldown_until_ms,
    suspend_reason,
    created_at_ms,
    updated_at_ms
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(thread_id, job_id) DO UPDATE SET
    loop_label = excluded.loop_label,
    vivling_id = excluded.vivling_id,
    strategy = excluded.strategy,
    ticks_managed = excluded.ticks_managed,
    recent_results_json = excluded.recent_results_json,
    last_plan_approved = excluded.last_plan_approved,
    strategy_override = excluded.strategy_override,
    override_main = excluded.override_main,
    cooldown_until_ms = excluded.cooldown_until_ms,
    suspend_reason = excluded.suspend_reason,
    updated_at_ms = excluded.updated_at_ms
            "#,
        )
        .bind(params.thread_id.to_string())
        .bind(&params.job_id)
        .bind(&params.loop_label)
        .bind(&params.vivling_id)
        .bind(params.strategy.as_str())
        .bind(params.ticks_managed)
        .bind(&params.recent_results_json)
        .bind(params.last_plan_approved)
        .bind(
            params
                .strategy_override
                .map(|strategy| strategy.as_str().to_string()),
        )
        .bind(params.override_main)
        .bind(params.cooldown_until_ms)
        .bind(&params.suspend_reason)
        .bind(params.created_at_ms)
        .bind(params.updated_at_ms)
        .execute(self.pool.as_ref())
        .await?;

        self.get_loop_delegation(params.thread_id, &params.job_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("loop delegation disappeared after upsert"))
    }

    pub async fn delete_loop_delegation(
        &self,
        thread_id: ThreadId,
        job_id: &str,
    ) -> anyhow::Result<bool> {
        let result =
            sqlx::query("DELETE FROM vl_loop_delegations WHERE thread_id = ? AND job_id = ?")
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
    use crate::LoopDelegationStrategy;
    use crate::runtime::test_support::unique_temp_dir;
    use codex_utils_absolute_path::test_support::PathExt;

    #[tokio::test]
    async fn loop_delegation_crud_roundtrip_preserves_override() -> anyhow::Result<()> {
        let codex_home = unique_temp_dir();
        let runtime = StateRuntime::init(
            crate::SqliteConfig::new_for_testing(codex_home.as_path().abs()),
            "test-provider".to_string(),
        )
        .await?;
        let thread_id = ThreadId::new();
        let params = LoopDelegationUpsertParams {
            thread_id,
            job_id: "job-1".to_string(),
            loop_label: "ci".to_string(),
            vivling_id: "vivling-1".to_string(),
            strategy: LoopDelegationStrategy::Observe,
            ticks_managed: 2,
            recent_results_json: "[]".to_string(),
            last_plan_approved: Some(true),
            strategy_override: Some(LoopDelegationStrategy::Suggest),
            override_main: true,
            cooldown_until_ms: None,
            suspend_reason: None,
            created_at_ms: 1_700_000_000_000,
            updated_at_ms: 1_700_000_000_001,
        };

        let saved = runtime.upsert_loop_delegation(params).await?;
        assert_eq!(saved.thread_id, thread_id);
        assert_eq!(saved.strategy, LoopDelegationStrategy::Observe);
        assert_eq!(
            saved.strategy_override,
            Some(LoopDelegationStrategy::Suggest)
        );
        assert!(saved.override_main);
        assert_eq!(runtime.list_loop_delegations(thread_id).await?.len(), 1);
        assert!(runtime.delete_loop_delegation(thread_id, "job-1").await?);
        assert!(
            runtime
                .get_loop_delegation(thread_id, "job-1")
                .await?
                .is_none()
        );
        Ok(())
    }
}
