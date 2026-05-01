use super::*;
use crate::vivling::VivlingLoopEvent;
use crate::vivling::VivlingLoopEventKind;
use crate::vivling::VivlingLoopEventSource;

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
    }

    pub(crate) fn record_vivling_turn_completed(&mut self, summary: Option<&str>) {
        self.sync_vivling_live_context();
        self.bottom_pane
            .record_vivling_turn_completed(&self.config, summary);
    }

    #[allow(dead_code)]
    pub(crate) fn replace_loop_jobs(&mut self, thread_id: ThreadId, jobs: Vec<ThreadLoopJob>) {
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
        jobs: Vec<ThreadLoopJob>,
        owner: codex_state::ThreadLoopOwner,
    ) {
        if self.thread_id != Some(thread_id) {
            self.abort_all_loop_job_tasks();
            self.loop_jobs.clear();
            self.bottom_pane.set_loop_context_label(None);
            return;
        }

        let mut next_jobs = BTreeMap::new();
        for job in jobs {
            let key = job.id.clone();
            let mut runtime = self.loop_jobs.remove(&key).unwrap_or(LoopJobRuntime {
                job: job.clone(),
                task: None,
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
    }

    pub(crate) fn submit_loop_prompt(
        &mut self,
        job: &ThreadLoopJob,
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
        let goal = job
            .goal_text
            .as_deref()
            .filter(|goal| !goal.trim().is_empty())
            .unwrap_or(&job.prompt_text);
        self.add_info_message(
            format!("Loop `{}` triggered on thread {thread_id}.", job.label),
            /*hint*/ None,
        );
        let mut prompt = job.prompt_text.clone();
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
    }

    fn schedule_loop_job_task(&self, runtime: &mut LoopJobRuntime) {
        if !runtime.job.enabled {
            return;
        }
        let Some(next_run_ms) = runtime.job.next_run_ms else {
            return;
        };
        let delay_ms = (next_run_ms - epoch_millis_now()).max(0) as u64;
        let thread_id = runtime.job.thread_id;
        let job_id = runtime.job.id.clone();
        let tx = self.app_event_tx.clone();
        runtime.task = Some(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            tx.send_vl(crate::vl::VlEvent::LoopTick { thread_id, job_id });
        }));
    }

    pub(super) fn abort_all_loop_job_tasks(&mut self) {
        for runtime in self.loop_jobs.values_mut() {
            if let Some(task) = runtime.task.take() {
                task.abort();
            }
        }
    }
}
