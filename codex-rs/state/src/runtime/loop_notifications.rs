use super::*;
use crate::model::{
    LOOP_NOTIFICATION_KIND_PENDING, LOOP_NOTIFICATION_PENDING_MAX_AGE_MS,
    LOOP_NOTIFICATION_SUMMARY_RETENTION, LoopNotificationRecord,
};

impl StateRuntime {
    /// Persist-before-emit with dedup. The INSERT OR
    /// IGNORE over the persisted `event_id` is the verdict: `true` = first
    /// time seen, the caller may emit; `false` = duplicate, must not be
    /// emitted again.
    pub async fn record_loop_notification(
        &self,
        record: LoopNotificationRecord,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            r#"
INSERT OR IGNORE INTO vl_loop_notifications
    (event_id, thread_id, job_id, label, kind, summary_json, created_at_ms)
VALUES (?, ?, ?, ?, ?, ?, ?)
"#,
        )
        .bind(&record.event_id)
        .bind(record.thread_id.to_string())
        .bind(&record.job_id)
        .bind(&record.label)
        .bind(record.kind)
        .bind(&record.summary_json)
        .bind(record.created_at_ms)
        .execute(self.pool.as_ref())
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// atomic variant: the summary and its notification pending
    /// land in ONE transaction, so a failed pending write can never leave a
    /// persisted summary whose retry then dedups against and never recreates
    /// the pending. Same dedup verdict as `record_loop_notification`:
    /// `true` = first time seen (emit allowed), `false` = duplicate.
    pub async fn record_loop_notification_with_pending(
        &self,
        record: LoopNotificationRecord,
        pending: Option<LoopNotificationRecord>,
    ) -> anyhow::Result<bool> {
        let mut tx = self.pool.begin().await?;
        let summary = sqlx::query(
            r#"
INSERT OR IGNORE INTO vl_loop_notifications
    (event_id, thread_id, job_id, label, kind, summary_json, created_at_ms)
VALUES (?, ?, ?, ?, ?, ?, ?)
"#,
        )
        .bind(&record.event_id)
        .bind(record.thread_id.to_string())
        .bind(&record.job_id)
        .bind(&record.label)
        .bind(record.kind)
        .bind(&record.summary_json)
        .bind(record.created_at_ms)
        .execute(&mut *tx)
        .await?;
        if summary.rows_affected() == 0 {
            // Duplicate event: nothing was written by this call, the pending
            // of the first attempt is already durable (or intentionally
            // absent). Commit the no-op and refuse the re-emit.
            tx.commit().await?;
            return Ok(false);
        }
        if let Some(pending) = pending {
            sqlx::query(
                r#"
INSERT OR IGNORE INTO vl_loop_notifications
    (event_id, thread_id, job_id, label, kind, summary_json, created_at_ms)
VALUES (?, ?, ?, ?, ?, ?, ?)
"#,
            )
            .bind(&pending.event_id)
            .bind(pending.thread_id.to_string())
            .bind(&pending.job_id)
            .bind(&pending.label)
            .bind(pending.kind)
            .bind(&pending.summary_json)
            .bind(pending.created_at_ms)
            .execute(&mut *tx)
            .await?;
        }
        // real retention, same transaction as the insert:
        // summaries keep the last N per job; pending rows older than the
        // configured age are dropped (canal-less runs do not accumulate).
        sqlx::query(
            r#"
DELETE FROM vl_loop_notifications
WHERE kind = 'summary' AND job_id = ? AND event_id NOT IN (
    SELECT event_id FROM (
        SELECT event_id FROM vl_loop_notifications
        WHERE kind = 'summary' AND job_id = ?
        ORDER BY created_at_ms DESC
        LIMIT ?
    )
)
"#,
        )
        .bind(&record.job_id)
        .bind(&record.job_id)
        .bind(LOOP_NOTIFICATION_SUMMARY_RETENTION)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
DELETE FROM vl_loop_notifications
WHERE kind = 'pending' AND created_at_ms < ?
"#,
        )
        .bind(record.created_at_ms - LOOP_NOTIFICATION_PENDING_MAX_AGE_MS)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// number of notification rows of a job for one kind; the
    /// retention test counts through it.
    pub async fn count_loop_notifications(
        &self,
        job_id: &str,
        kind: &'static str,
    ) -> anyhow::Result<i64> {
        let row: Option<(i64,)> = sqlx::query_as(
            r#"
SELECT COUNT(*) FROM vl_loop_notifications
WHERE job_id = ? AND kind = ?
"#,
        )
        .bind(job_id)
        .bind(kind)
        .fetch_optional(self.pool.as_ref())
        .await?;
        Ok(row.map(|(count,)| count).unwrap_or(0))
    }

    /// Pending notification rows, oldest first: the m2 consumer replays them
    /// at bootstrap (anomalies and one-shots only ever land here).
    pub async fn list_pending_loop_notifications(
        &self,
    ) -> anyhow::Result<Vec<LoopNotificationPendingRow>> {
        let rows = sqlx::query(
            r#"
SELECT event_id, thread_id, job_id, label, summary_json, created_at_ms
FROM vl_loop_notifications
WHERE kind = ?
ORDER BY created_at_ms ASC
"#,
        )
        .bind(LOOP_NOTIFICATION_KIND_PENDING)
        .fetch_all(self.pool.as_ref())
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                Ok::<_, sqlx::Error>(LoopNotificationPendingRow {
                    event_id: row.try_get("event_id")?,
                    thread_id: row.try_get("thread_id")?,
                    job_id: row.try_get("job_id")?,
                    label: row.try_get("label")?,
                    summary_json: row.try_get("summary_json")?,
                    created_at_ms: row.try_get("created_at_ms")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// Delivery receipt: a delivered pending row leaves the pending set
    /// (replay at bootstrap only picks up undelivered rows).
    pub async fn mark_loop_notification_delivered(&self, event_id: &str) -> anyhow::Result<bool> {
        let result = sqlx::query(
            r#"
DELETE FROM vl_loop_notifications
WHERE event_id = ? AND kind = 'pending'
"#,
        )
        .bind(event_id)
        .execute(self.pool.as_ref())
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

/// A pending notification row as the m2 consumer reads it (thread_id stays
/// the persisted TEXT form; the consumer owns the routing decision).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopNotificationPendingRow {
    pub event_id: String,
    pub thread_id: String,
    pub job_id: String,
    pub label: String,
    pub summary_json: String,
    pub created_at_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LOOP_NOTIFICATION_KIND_SUMMARY;
    use crate::runtime::test_support::unique_temp_dir;
    use codex_protocol::ThreadId;
    use codex_utils_absolute_path::test_support::PathExt;

    fn record(event_id: &str, kind: &'static str, created_at_ms: i64) -> LoopNotificationRecord {
        LoopNotificationRecord {
            event_id: event_id.to_string(),
            thread_id: ThreadId::new(),
            job_id: "job-notify".to_string(),
            label: "nightly".to_string(),
            kind,
            summary_json: format!(r#"{{"label":"nightly"}}"#),
            created_at_ms,
        }
    }

    // The same event_id persists once: the second insert is
    // refused (false = already persisted, must not be emitted again), and a
    // distinct event_id persists normally.
    #[tokio::test]
    async fn notification_record_dedups_on_event_id() -> anyhow::Result<()> {
        let codex_home = unique_temp_dir();
        let runtime = StateRuntime::init(
            crate::SqliteConfig::new_for_testing(codex_home.as_path().abs()),
            "test-provider".to_string(),
        )
        .await?;
        let first = record("evt-1", LOOP_NOTIFICATION_KIND_SUMMARY, 1_700_000_000_000);
        let duplicate = record("evt-1", LOOP_NOTIFICATION_KIND_SUMMARY, 1_700_000_000_001);
        let distinct = record("evt-2", LOOP_NOTIFICATION_KIND_PENDING, 1_700_000_000_002);

        assert!(runtime.record_loop_notification(first).await?);
        assert!(
            !runtime.record_loop_notification(duplicate).await?,
            "a duplicate event_id must be refused by the persisted dedup key"
        );
        assert!(runtime.record_loop_notification(distinct).await?);
        Ok(())
    }

    // Only `pending` rows replay: summaries never surface as pending, and
    // replay order is oldest-first.
    #[tokio::test]
    async fn pending_replay_lists_only_pending_rows_oldest_first() -> anyhow::Result<()> {
        let codex_home = unique_temp_dir();
        let runtime = StateRuntime::init(
            crate::SqliteConfig::new_for_testing(codex_home.as_path().abs()),
            "test-provider".to_string(),
        )
        .await?;

        runtime
            .record_loop_notification(record(
                "sum-1",
                LOOP_NOTIFICATION_KIND_SUMMARY,
                1_700_000_000_000,
            ))
            .await?;
        runtime
            .record_loop_notification(record(
                "pend-2",
                LOOP_NOTIFICATION_KIND_PENDING,
                1_700_000_000_001,
            ))
            .await?;
        runtime
            .record_loop_notification(record(
                "pend-3",
                LOOP_NOTIFICATION_KIND_PENDING,
                1_700_000_000_002,
            ))
            .await?;

        let pending = runtime.list_pending_loop_notifications().await?;
        assert_eq!(
            pending
                .iter()
                .map(|row| row.event_id.as_str())
                .collect::<Vec<_>>(),
            vec!["pend-2", "pend-3"],
            "only pending rows replay, oldest first"
        );

        // Delivery receipt: a delivered pending row leaves the pending set
        // (and a summary row is never touched by the delivery receipt).
        assert!(runtime.mark_loop_notification_delivered("pend-2").await?);
        let pending = runtime.list_pending_loop_notifications().await?;
        assert_eq!(
            pending
                .iter()
                .map(|row| row.event_id.as_str())
                .collect::<Vec<_>>(),
            vec!["pend-3"],
            "only the delivered row leaves the pending set"
        );
        assert!(
            !runtime.mark_loop_notification_delivered("sum-1").await?,
            "delivery receipt must never remove a summary row"
        );
        Ok(())
    }
}
