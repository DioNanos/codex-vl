//! codex-vl: loop subsystem stub for upstream merge.
//!
//! The full loop runtime (recurring prompts, loop owners, vivling loop events)
//! lives in codex-vl as our /loop and /vivling additive layer. The methods on
//! `ChatWidget` declared here are the stable surface called by
//! `app/loop_controller.rs`. They are currently no-op stubs to keep the build
//! green during the upstream merge; the real bodies will be re-ported on top
//! of the new upstream APIs in a follow-up session.

use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use super::*;
use crate::vivling::VivlingLoopEvent;
use crate::vivling::VivlingLoopEventKind;
use crate::vivling::VivlingLoopEventSource;
use crate::vl::loop_runtime::LoopJobPayload;

fn epoch_millis_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Initial delay of the blocked-tick retry ladder, in milliseconds. A
/// pending tick whose occurrence is already past due cannot dispatch
/// (busy user turn, review mode, side conversation, stuck claim, invalid
/// runner model): instead of re-arming at delay zero and re-firing on
/// every refresh, the safety timer backs off, doubling the delay on each
/// consecutive blocked re-arm.
const BLOCKED_RETRY_INITIAL_DELAY_MS: u64 = 1_000;
/// Saturation point of the doubling shift: `1s << 6` already exceeds the
/// cap, so every attempt past this one maps to the cap (no overflow).
const BLOCKED_RETRY_MAX_SHIFT: u32 = 6;
/// Upper bound of the retry ladder: at most one safety retry per minute.
const BLOCKED_RETRY_DELAY_CAP_MS: u64 = 60_000;
/// The retry attempt at which the ladder first hits the cap; warn once
/// here so a permanently blocked job stays visible instead of silently
/// settling at one retry per minute.
const BLOCKED_RETRY_CAP_ATTEMPT: u32 = BLOCKED_RETRY_MAX_SHIFT;

/// Delay before the next safety retry for a loop job whose tick stays
/// blocked, as a function of the number of consecutive blocked re-arms:
/// 1s, 2s, 4s, ... capped at 60s. Pure so the scheduling decision can be
/// tested without a clock.
fn blocked_retry_delay_ms(attempt: u32) -> u64 {
    let shift = attempt.min(BLOCKED_RETRY_MAX_SHIFT);
    (BLOCKED_RETRY_INITIAL_DELAY_MS << shift).min(BLOCKED_RETRY_DELAY_CAP_MS)
}

/// Timer delay, in milliseconds, used to (re-)arm a loop job, derived
/// from the persisted row, the in-memory blocked-retry count, and the
/// current time. A past-due pending tick is a blocked retry — the
/// previous dispatch of that occurrence was refused — so the delay comes
/// from the backoff ladder and is never zero, whatever the blocking
/// cause was. Everything else schedules a plain one-shot timer to
/// `next_run_ms` (zero when the occurrence is already due).
pub(crate) fn compute_loop_timer_delay(
    job: &codex_state::ThreadLoopJob,
    blocked_retries: u32,
    now_ms: i64,
) -> u64 {
    let Some(next_run_ms) = job.next_run_ms else {
        // Callers arm no timer without an occurrence; keep the helper
        // total for direct use in tests.
        return 0;
    };
    if job.pending_tick && next_run_ms <= now_ms {
        blocked_retry_delay_ms(blocked_retries)
    } else {
        (next_run_ms - now_ms).max(0) as u64
    }
}

/// Runtime wrapper around a persisted `ThreadLoopJob` while it is scheduled
/// in this `ChatWidget`. Includes the spawned timer task (if any) so the
/// widget can abort it on thread switch or shutdown.
pub(crate) struct LoopJobRuntime {
    pub(crate) job: codex_state::ThreadLoopJob,
    pub(crate) task: Option<tokio::task::JoinHandle<()>>,
    /// Consecutive blocked re-arms of this job (a past-due pending tick
    /// that could not dispatch). In-memory only: preserved across
    /// refreshes for the same job id, reset when a refreshed row is no
    /// longer pending, dropped on thread switch or restart so a fresh
    /// process restarts the ladder at the bottom.
    pub(crate) blocked_retries: u32,
}

impl ChatWidget {
    pub(crate) fn record_vivling_loop_event(
        &mut self,
        kind: VivlingLoopEventKind,
        source: VivlingLoopEventSource,
        action: &str,
        label: &str,
        runtime_state: Option<&str>,
        last_status: Option<&str>,
        goal: Option<&str>,
    ) {
        self.sync_vivling_live_context();
        self.bottom_pane.record_vivling_loop_event(
            &self.config,
            VivlingLoopEvent {
                kind,
                source,
                action: action.to_string(),
                label: label.to_string(),
                runtime_state: runtime_state.map(str::to_string),
                last_status: last_status.map(str::to_string),
                goal: goal.map(str::to_string),
            },
        );
        // Memory V2 Step 12.B.D.4 — post-loop Expression refresh.
        // Stricter than the turn hook: Adult-only + 5min throttle +
        // 50% budget headroom. Best-effort: every refusal layer can
        // drop the dispatch silently. Closes the gap Gemini 3.1 Pro
        // flagged in the 12.B.D.3 audit ("feature limitante, non
        // bug, appropriata per coprire lo scope D.4 futuro").
        self.maybe_trigger_vivling_loop_expression_refresh();
    }

    pub(crate) fn record_vivling_turn_completed(&mut self, summary: Option<&str>) {
        self.sync_vivling_live_context();
        self.bottom_pane
            .record_vivling_turn_completed(&self.config, summary);
        // Memory V2 Step 12.B.D.3 — post-turn Expression refresh.
        // Best-effort: planner / reservation / save_state can all
        // refuse and the helper silently drops the dispatch in that
        // case. Wired here so the freshly-updated work memory has a
        // chance to feed the next CRT phrase + proactive message.
        self.maybe_trigger_vivling_expression_refresh();
        // Snapshot del context bus volatile (§5.1). active_loops
        // dalle label dei job correnti; blockers mai inventati (vuoto qui:
        // in 5A non c'e un segnale strutturato di blocker dal worker turn).
        let active_loops: Vec<String> = self
            .loop_jobs
            .values()
            .map(|runtime| runtime.job.label.clone())
            .collect();
        self.app_event_tx
            .send_vl(crate::vl::VlEvent::ContextBusTurn {
                summary: summary.unwrap_or("").to_string(),
                active_loops,
                blockers: Vec::new(),
            });
    }

    #[allow(dead_code)]
    pub(crate) fn replace_loop_jobs(
        &mut self,
        thread_id: ThreadId,
        jobs: Vec<codex_state::ThreadLoopJob>,
    ) {
        self.replace_loop_jobs_with_owner(
            thread_id,
            jobs,
            codex_state::ThreadLoopOwner {
                thread_id,
                owner_kind: codex_state::THREAD_LOOP_OWNER_KIND_MAIN.to_string(),
                owner_vivling_id: None,
                updated_at_ms: 0,
            },
        );
    }

    pub(crate) fn replace_loop_jobs_with_owner(
        &mut self,
        thread_id: ThreadId,
        jobs: Vec<codex_state::ThreadLoopJob>,
        owner: codex_state::ThreadLoopOwner,
    ) {
        if self.thread_id != Some(thread_id) {
            self.abort_all_loop_job_tasks();
            self.loop_jobs.clear();
            self.bottom_pane.set_loop_context_label(None);
            self.sync_vivling_live_context();
            return;
        }

        let mut next_jobs = BTreeMap::new();
        for job in jobs {
            let key = job.id.clone();
            let mut runtime = self.loop_jobs.remove(&key).unwrap_or(LoopJobRuntime {
                job: job.clone(),
                task: None,
                blocked_retries: 0,
            });
            if let Some(task) = runtime.task.take() {
                task.abort();
            }
            runtime.job = job;
            self.schedule_loop_job_task(&mut runtime);
            next_jobs.insert(key, runtime);
        }

        for (_, runtime) in std::mem::take(&mut self.loop_jobs) {
            if let Some(task) = runtime.task {
                task.abort();
            }
        }
        self.loop_jobs = next_jobs;
        let loop_count = self.loop_jobs.len();
        let pending_job = self
            .loop_jobs
            .values()
            .find(|runtime| runtime.job.enabled && runtime.job.pending_tick)
            .map(|runtime| runtime.job.label.clone())
            .or_else(|| {
                self.loop_jobs
                    .values()
                    .find(|runtime| runtime.job.enabled)
                    .map(|runtime| runtime.job.label.clone())
            });
        let owner_label = match owner.owner_kind.as_str() {
            codex_state::THREAD_LOOP_OWNER_KIND_VIVLING => "vivling",
            _ => "main",
        };
        let label = if loop_count == 0 {
            None
        } else {
            Some(match pending_job {
                Some(next_label) => {
                    format!("loops: {loop_count} · owner: {owner_label} · next: {next_label}")
                }
                None => format!("loops: {loop_count} · owner: {owner_label}"),
            })
        };
        self.bottom_pane.set_loop_context_label(label);
        self.sync_vivling_live_context();
    }

    pub(crate) fn submit_loop_prompt(
        &mut self,
        job: &codex_state::ThreadLoopJob,
        owner: &codex_state::ThreadLoopOwner,
    ) -> LoopPromptSubmissionOutcome {
        let Some(thread_id) = self.thread_id else {
            return LoopPromptSubmissionOutcome::BlockedMissingThread;
        };
        if self.active_side_conversation {
            return LoopPromptSubmissionOutcome::BlockedSideConversation;
        }
        if self.is_review_mode {
            return LoopPromptSubmissionOutcome::BlockedReviewMode;
        }
        if self.is_user_turn_pending_or_running() {
            return LoopPromptSubmissionOutcome::BlockedUserTurn;
        }
        let payload = LoopJobPayload::from_storage_text(&job.prompt_text);
        let goal = job
            .goal_text
            .as_deref()
            .filter(|goal| !goal.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| payload.display_text());
        self.add_info_message(
            format!("Loop `{}` triggered on thread {thread_id}.", job.label),
            None,
        );
        let mut prompt = payload
            .prompt_text()
            .map(str::to_string)
            .unwrap_or_else(|| payload.display_text());
        prompt.push_str("\n\n[LOOP_CONTEXT]");
        prompt.push_str(&format!("\nlabel: {}", job.label));
        prompt.push_str(&format!("\ngoal: {goal}"));
        prompt.push_str(&format!(
            "\nauto_remove_on_completion: {}",
            job.auto_remove_on_completion
        ));
        prompt.push_str(&format!("\nrun_policy: {}", job.run_policy));
        prompt.push_str(&format!("\ncreated_by: {}", job.created_by));
        prompt.push_str(&format!("\nowner: {}", owner.owner_kind));
        if let Some(owner_vivling_id) = owner.owner_vivling_id.as_deref() {
            prompt.push_str(&format!("\nowner_vivling_id: {owner_vivling_id}"));
        }
        prompt.push_str("\ncompletion_action: manage_via_manage_loops");
        prompt.push_str("\nThis message was triggered by a local recurring loop.");
        if job.auto_remove_on_completion {
            prompt.push_str(
                "\nIf the goal is complete after this turn, call the dynamic tool `manage_loops` with action `remove` for this label.",
            );
        } else {
            prompt.push_str(
                "\nIf this loop should stop or change, call the dynamic tool `manage_loops` with action `disable`, `remove`, or `update` for this label.",
            );
        }
        self.submit_user_message(UserMessage::from(prompt));
        LoopPromptSubmissionOutcome::Submitted
    }

    pub(crate) fn clear_loop_jobs(&mut self) {
        self.abort_all_loop_job_tasks();
        self.loop_jobs.clear();
        self.bottom_pane.set_loop_context_label(None);
        self.sync_vivling_live_context();
    }

    fn schedule_loop_job_task(&self, runtime: &mut LoopJobRuntime) {
        if !runtime.job.enabled {
            return;
        }
        let Some(next_run_ms) = runtime.job.next_run_ms else {
            return;
        };
        let now_ms = epoch_millis_now();
        let blocked_retry = runtime.job.pending_tick && next_run_ms <= now_ms;
        let delay_ms = compute_loop_timer_delay(&runtime.job, runtime.blocked_retries, now_ms);
        if blocked_retry {
            // The previous dispatch of this occurrence was blocked (busy
            // user turn, review mode, side conversation, ...): re-arm on
            // the backoff ladder. A zero delay here would let the
            // refresh/timer pair re-fire the tick at every iteration
            // until the turn ends.
            let attempt = runtime.blocked_retries;
            if attempt == BLOCKED_RETRY_CAP_ATTEMPT {
                tracing::warn!(
                    job_id = %runtime.job.id,
                    label = %runtime.job.label,
                    delay_ms,
                    "loop job tick still blocked; retry delay reached its cap"
                );
            }
            runtime.blocked_retries = attempt.saturating_add(1);
        } else if !runtime.job.pending_tick {
            // The refreshed row is no longer pending (dispatched, expired,
            // or disarmed): the blocked episode is over and the next one
            // restarts the ladder from the bottom.
            runtime.blocked_retries = 0;
        }
        let thread_id = runtime.job.thread_id;
        let job_id = runtime.job.id.clone();
        let tx = self.app_event_tx.clone();
        runtime.task = Some(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            tx.send_vl(crate::vl::VlEvent::LoopTick { thread_id, job_id });
        }));
    }

    /// Test-only seam: the blocked-retry counter of a scheduled job, so
    /// loop-controller level tests can pin the backoff behavior.
    #[cfg(test)]
    pub(crate) fn loop_job_blocked_retries(&self, job_id: &str) -> Option<u32> {
        self.loop_jobs
            .get(job_id)
            .map(|runtime| runtime.blocked_retries)
    }

    pub(super) fn abort_all_loop_job_tasks(&mut self) {
        for runtime in self.loop_jobs.values_mut() {
            if let Some(task) = runtime.task.take() {
                task.abort();
            }
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopPromptSubmissionOutcome {
    Submitted,
    BlockedMissingThread,
    BlockedSideConversation,
    BlockedReviewMode,
    BlockedUserTurn,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_command::AppCommand as Op;
    use crate::chatwidget::tests::helpers::make_chatwidget_manual;
    use crate::chatwidget::tests::helpers::next_submit_op;
    use codex_app_server_protocol::UserInput;
    use codex_protocol::ThreadId;

    fn test_job(thread_id: ThreadId, next_run_ms: Option<i64>) -> codex_state::ThreadLoopJob {
        codex_state::ThreadLoopJob {
            id: "job-main".to_string(),
            thread_id,
            label: "main-loop".to_string(),
            prompt_text: "check the main thread".to_string(),
            goal_text: Some("keep the main turn alive".to_string()),
            interval_seconds: 300,
            enabled: true,
            run_policy: "queue_one".to_string(),
            auto_remove_on_completion: true,
            created_by: "agent".to_string(),
            next_run_ms,
            last_run_ms: None,
            last_status: None,
            last_error: None,
            pending_tick: false,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    fn main_owner(thread_id: ThreadId) -> codex_state::ThreadLoopOwner {
        codex_state::ThreadLoopOwner {
            thread_id,
            owner_kind: codex_state::THREAD_LOOP_OWNER_KIND_MAIN.to_string(),
            owner_vivling_id: None,
            updated_at_ms: 1,
        }
    }

    #[tokio::test]
    async fn main_runner_schedules_and_submits_a_main_turn() {
        let (mut widget, _events, mut ops) = make_chatwidget_manual(None).await;
        let thread_id = ThreadId::new();
        widget.thread_id = Some(thread_id);
        widget.replace_loop_jobs(thread_id, vec![test_job(thread_id, Some(i64::MAX))]);
        assert!(widget.loop_jobs["job-main"].task.is_some());

        let owner = codex_state::ThreadLoopOwner {
            thread_id,
            owner_kind: codex_state::THREAD_LOOP_OWNER_KIND_MAIN.to_string(),
            owner_vivling_id: None,
            updated_at_ms: 1,
        };
        assert_eq!(
            widget.submit_loop_prompt(&test_job(thread_id, None), &owner),
            LoopPromptSubmissionOutcome::Submitted
        );
        let Op::UserTurn { items, .. } = next_submit_op(&mut ops) else {
            panic!("runner=main must submit a main user turn");
        };
        assert!(items.iter().any(|item| {
            matches!(
                item,
                UserInput::Text { text, .. } if text.contains("\nowner: main")
            )
        }));
    }

    #[test]
    fn blocked_retry_delay_grows_and_caps() {
        assert_eq!(blocked_retry_delay_ms(0), 1_000);
        assert_eq!(blocked_retry_delay_ms(1), 2_000);
        assert_eq!(blocked_retry_delay_ms(2), 4_000);
        assert_eq!(blocked_retry_delay_ms(5), 32_000);
        assert_eq!(blocked_retry_delay_ms(6), 60_000);
        assert_eq!(blocked_retry_delay_ms(7), 60_000);
        assert_eq!(blocked_retry_delay_ms(u32::MAX), 60_000);
    }

    #[test]
    fn backoff_applies_to_every_blocked_cause() {
        let thread_id = ThreadId::new();
        let now_ms = 5_000_000;
        let mut job = test_job(thread_id, Some(now_ms - 60_000));
        job.pending_tick = true;
        // Every blocked cause (busy turn, review mode, side conversation,
        // stuck claim, invalid runner model, missing owner) persists the
        // same row shape: a past-due occurrence with the tick still
        // pending. The scheduling decision must key only on that shape,
        // whatever `last_status` names the cause.
        for status in [
            "pending_busy",
            "skipped_busy",
            "blocked_review",
            "blocked_side",
            "blocked_owner",
            "invalid_runner_model",
        ] {
            job.last_status = Some(status.to_string());
            assert_eq!(
                compute_loop_timer_delay(&job, 0, now_ms),
                1_000,
                "the first retry must start at the ladder bottom for {status}"
            );
            assert_eq!(
                compute_loop_timer_delay(&job, 2, now_ms),
                4_000,
                "the ladder must grow for {status}"
            );
        }
        // A future occurrence is a normal timer, untouched by the counter.
        job.next_run_ms = Some(now_ms + 30_000);
        assert_eq!(compute_loop_timer_delay(&job, 5, now_ms), 30_000);
        // A past-due row that is no longer pending is an ordinary due
        // tick (delay zero; the dispatch itself reschedules it), not a
        // blocked retry, whatever the counter holds.
        job.pending_tick = false;
        job.next_run_ms = Some(now_ms - 60_000);
        assert_eq!(compute_loop_timer_delay(&job, 5, now_ms), 0);
    }

    #[tokio::test]
    async fn pending_past_due_tick_re_arms_with_backoff_not_zero() {
        let (mut widget, _events, _ops) = make_chatwidget_manual(None).await;
        let thread_id = ThreadId::new();
        widget.thread_id = Some(thread_id);

        let now_ms = epoch_millis_now();
        let mut job = test_job(thread_id, Some(now_ms - 60_000));
        job.pending_tick = true;
        job.last_status = Some("pending_busy".to_string());

        // Refresh cycles of a blocked loop: each refresh re-arms the
        // safety timer on the backoff ladder (never at delay zero, the
        // old behavior) and the counter must survive the runtime reuse
        // keyed by job id.
        widget.replace_loop_jobs_with_owner(thread_id, vec![job.clone()], main_owner(thread_id));
        assert_eq!(
            widget.loop_jobs["job-main"].blocked_retries,
            1,
            "the first blocked re-arm consumes ladder step 0 and bumps the counter"
        );
        assert_eq!(
            compute_loop_timer_delay(&job, 0, now_ms),
            1_000,
            "the first blocked re-arm must wait one second, not zero"
        );

        widget.replace_loop_jobs_with_owner(thread_id, vec![job.clone()], main_owner(thread_id));
        assert_eq!(widget.loop_jobs["job-main"].blocked_retries, 2);
        widget.replace_loop_jobs_with_owner(thread_id, vec![job.clone()], main_owner(thread_id));
        assert_eq!(widget.loop_jobs["job-main"].blocked_retries, 3);
        assert_eq!(
            compute_loop_timer_delay(&job, 2, now_ms),
            4_000,
            "the third blocked refresh must sit on the third ladder step"
        );

        // A refreshed row that is no longer pending (dispatched, expired,
        // or disarmed) ends the blocked episode: the ladder restarts at
        // the bottom for the next one.
        let mut settled = job.clone();
        settled.pending_tick = false;
        widget.replace_loop_jobs_with_owner(thread_id, vec![settled], main_owner(thread_id));
        assert_eq!(widget.loop_jobs["job-main"].blocked_retries, 0);
    }
}
