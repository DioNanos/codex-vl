//! codex-vl loop_controller: T6 m1 — pure builder of the fixed-format loop
//! tick summary (`loop · label · esito · durata · prossimo run · runner`),
//! plus manager and runner with their respective reasons (matrice T2).
//!
//! The struct is the contract: every field of the fixed format is a required
//! field, so a missing field is a compile error, never a runtime hole. No
//! `Default`, no silent fallback: [`NextRun::Terminal`] and
//! [`LoopTickSummary::suspend_reason`] carry real domain absence (a terminal
//! one-shot has no next instant; a suspension reason only exists when the
//! delegation is actually suspended), not convenience defaults.
//!
//! R11 gates honoured here: `persist_before_emit` writes the row (or the
//! notification pending) **before** any emit can happen, and `event_id` is
//! the persisted dedup key. Emission itself (bounded queue, notifier worker,
//! `ticks.rs` seam) lands in m2 — nothing here touches the tick path.

use codex_protocol::ThreadId;
use codex_state::LOOP_NOTIFICATION_KIND_PENDING;
use codex_state::LOOP_NOTIFICATION_KIND_SUMMARY;
use codex_state::LoopNotificationRecord;
use codex_state::LoopRunnerKind;

/// Schedule kind of the tick's descriptor — decides whether a successful
/// tick is silent (interval/at) or must notify (one-shot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopScheduleKind {
    Interval,
    At,
    OneShot,
}

impl LoopScheduleKind {
    pub(crate) fn from_descriptor(raw: &str) -> Self {
        match raw {
            "at" => LoopScheduleKind::At,
            "one_shot" => LoopScheduleKind::OneShot,
            _ => LoopScheduleKind::Interval,
        }
    }

    pub(crate) fn render(self) -> &'static str {
        match self {
            LoopScheduleKind::Interval => "interval",
            LoopScheduleKind::At => "at",
            LoopScheduleKind::OneShot => "one_shot",
        }
    }
}

/// Terminal esito of the tick. `Ok` covers the silent success of an
/// interval/at tick; every other value is anomalous for R3 notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopTickOutcome {
    Ok,
    Failed,
    SkippedBusy,
    OneShotExpired,
}

impl LoopTickOutcome {
    fn render(self) -> &'static str {
        match self {
            LoopTickOutcome::Ok => "ok",
            LoopTickOutcome::Failed => "failed",
            LoopTickOutcome::SkippedBusy => "skipped_busy",
            LoopTickOutcome::OneShotExpired => "one_shot_expired",
        }
    }
}

/// Next run instant of the job after this tick. `Terminal` carries the
/// reason there is no next instant (an expired one-shot never reschedules).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NextRun {
    At(i64),
    Terminal(String),
}

impl NextRun {
    fn render(&self) -> String {
        match self {
            NextRun::At(ms) => format!("{ms}"),
            NextRun::Terminal(reason) => format!("none ({reason})"),
        }
    }
}

/// Who manages the loop (matrice T2 gestore), with the resolution reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopManager {
    Main,
    Vivling,
}

impl LoopManager {
    fn render(self) -> &'static str {
        match self {
            LoopManager::Main => "main",
            LoopManager::Vivling => "vivling",
        }
    }
}

/// The fixed-format summary of one finished loop tick. Every field is
/// required: constructing one with a missing field does not compile.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoopTickSummary {
    pub thread_id: ThreadId,
    pub job_id: String,
    pub label: String,
    pub schedule_kind: LoopScheduleKind,
    pub outcome: LoopTickOutcome,
    /// Wall-clock duration of the tick execution.
    pub duration_ms: i64,
    pub next_run: NextRun,
    pub runner: LoopRunnerKind,
    pub runner_reason: String,
    pub manager: LoopManager,
    pub manager_reason: String,
    /// Present only when the T5 delegation is actually suspended.
    pub suspend_reason: Option<String>,
    /// Claimed occurrence instant bound into the dedup event id; a manual
    /// trigger or a completion path without the carried key carries `None`
    /// (the descriptor-derived `scheduled_key` is the stable fallback).
    pub occurrence_ms: Option<i64>,
    /// FIX-J (4) — descriptor-derived occurrence key: stable across retries
    /// and across finish instants, so the dedup id never depends on timing.
    pub scheduled_key: String,
    pub finished_at_ms: i64,
}

impl LoopTickSummary {
    /// Fixed-format §T6 line, plus manager/runner reasons and, when present,
    /// the suspension reason. One format, no variants.
    pub(crate) fn render(&self) -> String {
        let mut rendered = format!(
            "loop · {} · {} · {}ms · {} · {}",
            self.label,
            self.outcome.render(),
            self.duration_ms,
            self.next_run.render(),
            self.runner.as_str(),
        );
        rendered.push_str(&format!(
            "\nmanager: {} ({}) · runner reason: {}",
            self.manager.render(),
            self.manager_reason,
            self.runner_reason,
        ));
        if let Some(reason) = &self.suspend_reason {
            rendered.push_str(&format!("\nsuspended: {reason}"));
        }
        rendered
    }

    /// Stable dedup key: bound to (job_id, occurrence key) — FIX-J (4).
    /// `finished_at_ms` must NOT enter the id: a retried persist, or a tick
    /// that finishes later than another attempt, is the same event. When the
    /// claimed occurrence is carried (`occurrence_ms`) it is the key;
    /// otherwise the descriptor-derived `scheduled_key` is the stable
    /// fallback for legacy paths.
    pub(crate) fn event_id(&self) -> String {
        format!(
            "{}:{}:{}",
            self.job_id,
            self.schedule_kind.render(),
            self.occurrence_ms
                .map_or_else(
                    || format!("sched:{}", self.scheduled_key),
                    |ms| format!("occ:{ms}")
                ),
        )
    }

    /// R3 — a notification pending exists only for anomalous outcomes and
    /// one-shot ticks. Silent interval/at successes never create one.
    pub(crate) fn pending_needed(&self) -> bool {
        match self.outcome {
            LoopTickOutcome::Ok => self.schedule_kind == LoopScheduleKind::OneShot,
            LoopTickOutcome::Failed
            | LoopTickOutcome::SkippedBusy
            | LoopTickOutcome::OneShotExpired => true,
        }
    }

    /// R11 gate 1 — persist-before-emit: `record_loop_notification` writes
    /// the row (with the dedup verdict) before the caller is allowed to emit.
    /// `Ok(true)` = first persistence, emit is allowed; `Ok(false)` =
    /// duplicate event id, already persisted, never emit again.
    pub(crate) async fn persist_before_emit(
        &self,
        state_runtime: &codex_state::StateRuntime,
        kind: &'static str,
    ) -> anyhow::Result<bool> {
        state_runtime
            .record_loop_notification(LoopNotificationRecord {
                event_id: self.event_id(),
                thread_id: self.thread_id,
                job_id: self.job_id.clone(),
                label: self.label.clone(),
                kind,
                summary_json: self.to_persisted_json()?,
                created_at_ms: self.finished_at_ms,
            })
            .await
    }

    /// Explicit projection for persistence: the typed summary stays the
    /// single source of truth, the JSON form is derived from it field by
    /// field (no generic value bags).
    pub(crate) fn to_persisted_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&PersistedSummary {
            thread_id: self.thread_id.to_string(),
            job_id: &self.job_id,
            label: &self.label,
            schedule_kind: self.schedule_kind.render(),
            outcome: self.outcome.render(),
            duration_ms: self.duration_ms,
            next_run: self.next_run.render(),
            runner: self.runner.as_str(),
            runner_reason: &self.runner_reason,
            manager: self.manager.render(),
            manager_reason: &self.manager_reason,
            suspend_reason: self.suspend_reason.as_deref(),
            occurrence_ms: self.occurrence_ms,
            finished_at_ms: self.finished_at_ms,
        })
    }
}

#[derive(serde::Serialize)]
struct PersistedSummary<'a> {
    thread_id: String,
    job_id: &'a str,
    label: &'a str,
    schedule_kind: &'static str,
    outcome: &'static str,
    duration_ms: i64,
    next_run: String,
    runner: &'a str,
    runner_reason: &'a str,
    manager: &'static str,
    manager_reason: &'a str,
    suspend_reason: Option<&'a str>,
    occurrence_ms: Option<i64>,
    finished_at_ms: i64,
}

/// R11 gate 1 — the pending flavor of `persist_before_emit`, valid only for
/// the events R3 admits; callers must check [`LoopTickSummary::pending_needed`]
/// before reaching for it.
pub(crate) fn pending_kind() -> &'static str {
    LOOP_NOTIFICATION_KIND_PENDING
}

pub(crate) fn summary_kind() -> &'static str {
    LOOP_NOTIFICATION_KIND_SUMMARY
}

#[cfg(test)]
mod tests {
    use super::LoopManager;
    use super::LoopScheduleKind;
    use super::LoopTickOutcome;
    use super::LoopTickSummary;
    use super::NextRun;
    use codex_protocol::ThreadId;
    use codex_state::LoopRunnerKind;

    fn summary(outcome: LoopTickOutcome) -> LoopTickSummary {
        LoopTickSummary {
            thread_id: ThreadId::new(),
            job_id: "job-nightly".to_string(),
            label: "nightly".to_string(),
            schedule_kind: LoopScheduleKind::Interval,
            outcome,
            duration_ms: 1_250,
            next_run: NextRun::At(1_700_000_060_000),
            runner: LoopRunnerKind::Main,
            runner_reason: "runner=main".to_string(),
            manager: LoopManager::Main,
            manager_reason: "not_delegated".to_string(),
            suspend_reason: None,
            occurrence_ms: Some(1_700_000_000_000),
            scheduled_key: "interval:60".to_string(),
            finished_at_ms: 1_700_000_000_500,
        }
    }

    // §T6 fixed format, one render per esito — four different outcomes all
    // render from the same single format.
    #[test]
    fn render_is_fixed_for_ok_interval_tick() {
        assert_eq!(
            summary(LoopTickOutcome::Ok).render(),
            "loop · nightly · ok · 1250ms · 1700000060000 · main\n\
             manager: main (not_delegated) · runner reason: runner=main"
        );
    }

    #[test]
    fn render_is_fixed_for_failed_tick_with_suspension() {
        let mut failed = summary(LoopTickOutcome::Failed);
        failed.schedule_kind = LoopScheduleKind::OneShot;
        failed.suspend_reason = Some("vivling_unavailable".to_string());
        failed.manager = LoopManager::Vivling;
        failed.runner = LoopRunnerKind::ChildAgent;
        failed.next_run = NextRun::Terminal("one-shot terminal".to_string());
        assert_eq!(
            failed.render(),
            "loop · nightly · failed · 1250ms · none (one-shot terminal) · child_agent\n\
             manager: vivling (not_delegated) · runner reason: runner=main\n\
             suspended: vivling_unavailable"
        );
    }

    #[test]
    fn render_is_fixed_for_skipped_busy_tick() {
        let mut busy = summary(LoopTickOutcome::SkippedBusy);
        busy.next_run = NextRun::Terminal("child in flight".to_string());
        assert_eq!(
            busy.render(),
            "loop · nightly · skipped_busy · 1250ms · none (child in flight) · main\n\
             manager: main (not_delegated) · runner reason: runner=main"
        );
    }

    #[test]
    fn render_is_fixed_for_one_shot_expired_tick() {
        let mut expired = summary(LoopTickOutcome::OneShotExpired);
        expired.schedule_kind = LoopScheduleKind::OneShot;
        expired.next_run = NextRun::Terminal("past grace window".to_string());
        assert_eq!(
            expired.render(),
            "loop · nightly · one_shot_expired · 1250ms · none (past grace window) · main\n\
             manager: main (not_delegated) · runner reason: runner=main"
        );
    }

    // R3 — a pending exists only for anomalous outcomes and one-shot ticks:
    // a silent interval (or at) success must never create one.
    #[test]
    fn pending_not_created_for_silent_interval_tick() {
        assert!(!summary(LoopTickOutcome::Ok).pending_needed());
        let mut at_tick = summary(LoopTickOutcome::Ok);
        at_tick.schedule_kind = LoopScheduleKind::At;
        assert!(!at_tick.pending_needed());
    }

    #[test]
    fn pending_created_for_one_shot_success_and_anomalies() {
        let mut one_shot_ok = summary(LoopTickOutcome::Ok);
        one_shot_ok.schedule_kind = LoopScheduleKind::OneShot;
        assert!(one_shot_ok.pending_needed());
        assert!(summary(LoopTickOutcome::Failed).pending_needed());
        assert!(summary(LoopTickOutcome::SkippedBusy).pending_needed());
        assert!(summary(LoopTickOutcome::OneShotExpired).pending_needed());
    }

    // FIX-J (4) — the dedup id is bound to (job_id, occurrence key): it is
    // stable across retries and MUST NOT change when the finish instant
    // moves; a different occurrence is a different event.
    #[test]
    fn event_id_is_stable_per_occurrence_and_finish_instant() {
        let summary = summary(LoopTickOutcome::Ok);
        assert_eq!(summary.event_id(), summary.event_id());

        let mut retried_later = summary.clone();
        retried_later.finished_at_ms += 9_999;
        assert_eq!(
            summary.event_id(),
            retried_later.event_id(),
            "moving the finish instant must not mint a new event id"
        );

        let mut other_occurrence = summary.clone();
        other_occurrence.occurrence_ms = Some(1_700_000_060_000);
        assert_ne!(summary.event_id(), other_occurrence.event_id());

        // Legacy path without a carried occurrence: the descriptor-derived
        // scheduled key is the stable fallback.
        let mut legacy = summary.clone();
        legacy.occurrence_ms = None;
        assert_eq!(legacy.event_id(), legacy.event_id());
        let mut legacy_later = legacy.clone();
        legacy_later.finished_at_ms += 5_000;
        assert_eq!(legacy.event_id(), legacy_later.event_id());
    }

    // The persisted JSON projection is derived field-by-field from the typed
    // summary: no generic value bags, no fields dropped.
    #[test]
    fn persisted_json_carries_every_required_field() {
        let json = summary(LoopTickOutcome::Ok)
            .to_persisted_json()
            .expect("summary serializes");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        for field in [
            "thread_id",
            "job_id",
            "label",
            "schedule_kind",
            "outcome",
            "duration_ms",
            "next_run",
            "runner",
            "runner_reason",
            "manager",
            "manager_reason",
            "suspend_reason",
            "occurrence_ms",
            "finished_at_ms",
        ] {
            assert!(
                value.get(field).is_some(),
                "persisted summary drops `{field}`"
            );
        }
    }
}
