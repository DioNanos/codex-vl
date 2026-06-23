use super::super::model::VivlingWorkMemoryEntry;
use super::brain_context::BrainPromptKind;
use super::brain_context::compose_brain_prompt;
use super::request::brain_target_from_profile;
use super::request::resolve_expression_target;
use super::*;
use codex_vivling_core::model::VivlingSkill;
use codex_vivling_core::paths::skills_file_path;

/// Memory V2 Step 9.A — load the planner-written `_skills.json`
/// sidecar. Best-effort: missing file or malformed JSON yields an
/// empty list, never an error. The brain prompt simply omits the
/// `Skill library:` section when nothing usable is available, so a
/// corrupt sidecar can never break Chat/Assist/LoopTick.
fn load_vivling_skills(roster_dir: &Path, vivling_id: &str) -> Vec<VivlingSkill> {
    let path = skills_file_path(roster_dir, vivling_id);
    let body = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            if err.kind() != io::ErrorKind::NotFound {
                tracing::debug!(
                    target: "vivling::skills",
                    "failed to read {}: {err}",
                    path.display()
                );
            }
            return Vec::new();
        }
    };
    match serde_json::from_str::<Vec<VivlingSkill>>(&body) {
        Ok(list) => list,
        Err(err) => {
            tracing::debug!(
                target: "vivling::skills",
                "malformed skills sidecar at {}: {err}",
                path.display()
            );
            Vec::new()
        }
    }
}

impl Vivling {
    pub(crate) fn prepare_assist_request(
        &mut self,
        task: &str,
    ) -> Result<VivlingAssistRequest, String> {
        self.ensure_hatched()?;
        let live_snapshot = self.live_context.borrow().clone();
        // Memory V2 Step 9.A: load the planner-written skills sidecar
        // (Step 8.B output) BEFORE composing the prompt. Best-effort —
        // missing/malformed sidecar yields an empty list.
        let roster_dir = self.roster_dir();
        let skills = match (roster_dir.as_ref(), self.state.as_ref()) {
            (Some(dir), Some(state)) => load_vivling_skills(dir, &state.vivling_id),
            _ => Vec::new(),
        };
        let (vivling_id, vivling_name, brain_target, prompt_context, task) = {
            let state = self.state.as_mut().expect("state checked");
            let now = Utc::now();
            state.apply_decay(now);
            // Memory V2 Step 5.A: feed the assist payload into the
            // axis-G rolling sample window BEFORE composing the prompt,
            // so the language contract reflects the very task the user
            // just typed.
            state.language_state.record_sample(now, task);
            state.language_state.refresh_detected_language();
            let prompt_context = compose_brain_prompt(
                state,
                BrainPromptKind::Assist,
                task,
                live_snapshot.as_ref(),
                self.msa.as_deref(),
                &skills,
            )?;
            // Memory V2 §8.1 (P0.2): inheritance rule. Absence of an
            // explicit profile means SessionDefault, not an error and
            // not a synthetic profile. `brain_enabled` stays a feature
            // gate and is still enforced by `compose_brain_prompt`.
            let brain_target = brain_target_from_profile(state.brain_profile.as_deref());
            // codex-vl bond: only credit Assist after pre-dispatch validation
            // succeeds, so a failed precondition does not mutate bond state.
            state
                .bond
                .record_interaction(crate::vivling::VivlingInteractionKind::Assist, now);
            (
                state.vivling_id.clone(),
                state.name.clone(),
                brain_target,
                prompt_context,
                task.trim().to_string(),
            )
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(VivlingAssistRequest {
            vivling_id,
            vivling_name,
            brain_target,
            kind: VivlingBrainRequestKind::Assist,
            task,
            prompt_context,
        })
    }

    pub(crate) fn prepare_chat_request(
        &mut self,
        text: &str,
    ) -> Result<VivlingAssistRequest, String> {
        self.ensure_hatched()?;
        let live_snapshot = self.live_context.borrow().clone();
        // Memory V2 Step 9.A: same best-effort sidecar load as assist.
        let roster_dir = self.roster_dir();
        let skills = match (roster_dir.as_ref(), self.state.as_ref()) {
            (Some(dir), Some(state)) => load_vivling_skills(dir, &state.vivling_id),
            _ => Vec::new(),
        };
        let (vivling_id, vivling_name, brain_target, prompt_context, task) = {
            let state = self.state.as_mut().expect("state checked");
            let now = Utc::now();
            state.apply_decay(now);
            // Memory V2 Step 5.A: same sampling hook as `prepare_assist_request`.
            // LoopTick is automation and is intentionally skipped.
            state.language_state.record_sample(now, text);
            state.language_state.refresh_detected_language();
            let prompt_context = compose_brain_prompt(
                state,
                BrainPromptKind::Chat,
                text,
                live_snapshot.as_ref(),
                self.msa.as_deref(),
                &skills,
            )?;
            // Memory V2 Step 12.B.C: `/vl` chat uses the Expression
            // brain target — a pinned profile overrides the session
            // model only when `brain_enabled` is true; otherwise we
            // always inherit `/model` (matches DAG's "STESSO MODELLO
            // CHAT" contract).
            let brain_target =
                resolve_expression_target(state.brain_enabled, state.brain_profile.as_deref());
            // codex-vl bond: only credit Chat after pre-dispatch validation
            // succeeds, so a failed precondition does not mutate bond state.
            state
                .bond
                .record_interaction(crate::vivling::VivlingInteractionKind::Chat, now);
            (
                state.vivling_id.clone(),
                state.name.clone(),
                brain_target,
                prompt_context,
                text.trim().to_string(),
            )
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(VivlingAssistRequest {
            vivling_id,
            vivling_name,
            brain_target,
            kind: VivlingBrainRequestKind::Chat,
            task,
            prompt_context,
        })
    }

    pub(crate) fn active_loop_owner_identity(&mut self) -> Result<(String, String), String> {
        self.ensure_hatched()?;
        let state = self.state.as_mut().expect("state checked");
        state.apply_decay(Utc::now());
        if state.stage() != Stage::Adult {
            return Err("Vivling loop ownership unlocks only at level 60.".to_string());
        }
        if !state.brain_enabled {
            return Err("Enable the Vivling brain first with `/vivling brain on`.".to_string());
        }
        // Memory V2 §8.1 (P0.2): a missing `brain_profile` no longer
        // blocks loop ownership. The dispatcher will fall back to
        // SessionDefault and read `config.model` at run time.
        Ok((state.vivling_id.clone(), state.name.clone()))
    }

    pub(crate) fn prepare_loop_tick_request(
        &mut self,
        owner_vivling_id: &str,
        job: &codex_state::ThreadLoopJob,
    ) -> Result<VivlingLoopTickRequest, String> {
        let state = self
            .load_state_for_id(owner_vivling_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("Vivling owner `{owner_vivling_id}` is missing on disk."))?;
        if state.stage() != Stage::Adult {
            return Err(format!("Vivling owner `{}` is not adult yet.", state.name));
        }
        if !state.brain_enabled {
            return Err(format!(
                "Vivling owner `{}` has brain disabled.",
                state.name
            ));
        }
        // Memory V2 §8.1 (P0.2): inheritance rule. SessionDefault when
        // no profile is pinned; the dispatcher resolves to `config.model`.
        let brain_target = brain_target_from_profile(state.brain_profile.as_deref());
        let goal = job
            .goal_text
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&job.prompt_text)
            .to_string();
        let live_snapshot = self.live_context.borrow().clone();
        // Memory V2 Step 9.A: load owner's skills sidecar (best-effort).
        let skills = self
            .roster_dir()
            .map(|dir| load_vivling_skills(&dir, owner_vivling_id))
            .unwrap_or_default();
        let prompt_context = compose_brain_prompt(
            &state,
            BrainPromptKind::LoopTick {
                label: &job.label,
                goal: &goal,
                prompt_text: &job.prompt_text,
                auto_remove_on_completion: job.auto_remove_on_completion,
            },
            &job.prompt_text,
            live_snapshot.as_ref(),
            self.msa.as_deref(),
            &skills,
        )?;
        Ok(VivlingLoopTickRequest {
            vivling_id: state.vivling_id.clone(),
            vivling_name: state.name.clone(),
            brain_target,
            loop_label: job.label.clone(),
            loop_goal: goal,
            prompt_text: job.prompt_text.clone(),
            auto_remove_on_completion: job.auto_remove_on_completion,
            prompt_context,
        })
    }

    pub(crate) fn prepare_existing_profile_request(
        &mut self,
        profile: String,
    ) -> Result<VivlingBrainProfileRequest, String> {
        self.ensure_hatched()?;
        let state = self.state.as_ref().expect("state checked");
        Ok(VivlingBrainProfileRequest {
            vivling_id: state.vivling_id.clone(),
            vivling_name: state.name.clone(),
            kind: VivlingBrainProfileRequestKind::AssignExisting { profile },
        })
    }

    pub(crate) fn prepare_custom_profile_request(
        &mut self,
        model: String,
        provider: Option<String>,
        effort: Option<ReasoningEffortConfig>,
    ) -> Result<VivlingBrainProfileRequest, String> {
        self.ensure_hatched()?;
        let state = self.state.as_ref().expect("state checked");
        Ok(VivlingBrainProfileRequest {
            vivling_id: state.vivling_id.clone(),
            vivling_name: state.name.clone(),
            kind: VivlingBrainProfileRequestKind::CreateOrUpdate {
                profile: format!("vivling-{}", state.vivling_id),
                model,
                provider,
                effort,
            },
        })
    }

    pub(crate) fn ensure_hatched(&self) -> Result<(), String> {
        if self.state.as_ref().is_some_and(|state| state.hatched) {
            Ok(())
        } else {
            Err("No Vivling yet. Use /vivling hatch.".to_string())
        }
    }

    pub(crate) fn roster_summary(&mut self) -> Result<String, String> {
        let roster = self.load_roster().map_err(|err| err.to_string())?;
        if roster.vivling_ids.is_empty() {
            return Err("No Vivling yet. Use /vivling hatch.".to_string());
        }
        let mut lines = Vec::new();
        lines.push(format!(
            "Vivling roster · active {} · top-level slots {}/{}",
            roster
                .active_vivling_id
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            self.top_level_slot_usage().map_err(|err| err.to_string())?,
            EXTERNAL_SLOT_LIMIT
        ));
        for vivling_id in &roster.vivling_ids {
            if let Some(state) = self
                .load_state_for_id(vivling_id)
                .map_err(|err| err.to_string())?
            {
                let active_mark = if roster.active_vivling_id.as_deref() == Some(vivling_id) {
                    "*"
                } else {
                    "-"
                };
                let label = state
                    .instance_label
                    .as_deref()
                    .map(|value| format!(" · {value}"))
                    .unwrap_or_default();
                lines.push(format!(
                    "{active_mark} {} [{}] {} {} Lv {}{}",
                    state.vivling_id,
                    state.lineage_role_label(),
                    state.name,
                    species_for_id(&state.species).name,
                    state.level,
                    label
                ));
            }
        }
        Ok(lines.join("\n"))
    }

    pub(crate) fn record_loop_event(&mut self, event: VivlingLoopEvent) -> Result<(), String> {
        let live_summary = self
            .live_context
            .borrow()
            .as_ref()
            .and_then(VivlingLiveContext::memory_summary);
        let msa = self.msa.clone();
        let vivling_id = self.state.as_ref().map(|state| state.vivling_id.clone());
        let new_capsules: RefCell<Vec<VivlingWorkMemoryEntry>> = RefCell::new(Vec::new());
        self.update_existing(|state| {
            new_capsules
                .borrow_mut()
                .extend(state.record_loop_event(&event));
            if let Some(summary) = live_summary.as_deref() {
                new_capsules
                    .borrow_mut()
                    .extend(state.record_live_context_summary(summary));
            }
            let proactive = proactive::evaluate_after_loop_event(state, Utc::now());
            if let Some(msg) = proactive.message {
                state.last_message = Some(msg);
            }
            state
                .last_message
                .clone()
                .unwrap_or_else(|| format!("loop {} `{}` noted", event.action, event.label))
        })
        .map(|_| {
            if let (Some(msa), Some(id)) = (msa.as_deref(), vivling_id.as_deref()) {
                for capsule in new_capsules.borrow().iter() {
                    msa.index_capsule(id, capsule);
                }
            }
            self.mark_recent_activity(ACTIVE_FOOTER_TAIL);
        })?;

        // codex-vl lineage passive learning: propagate distillates to
        // direct children after the active primary records a loop event
        // (parallel to record_turn_completed). Best-effort.
        let _ = self.propagate_parent_summaries_to_children();
        Ok(())
    }

    pub(crate) fn record_turn_completed(&mut self, summary: Option<&str>) -> Result<(), String> {
        let live_summary = self
            .live_context
            .borrow()
            .as_ref()
            .and_then(VivlingLiveContext::memory_summary);
        let msa = self.msa.clone();
        let vivling_id = self.state.as_ref().map(|state| state.vivling_id.clone());
        let new_capsules: RefCell<Vec<VivlingWorkMemoryEntry>> = RefCell::new(Vec::new());
        self.update_existing(|state| {
            new_capsules
                .borrow_mut()
                .extend(state.record_turn_completed(summary));
            if let Some(summary) = live_summary.as_deref() {
                new_capsules
                    .borrow_mut()
                    .extend(state.record_live_context_summary(summary));
            }
            let proactive = proactive::evaluate_after_turn(state, Utc::now());
            if let Some(msg) = proactive.message {
                state.last_message = Some(msg);
            }
            state
                .last_message
                .clone()
                .unwrap_or_else(|| "is learning from completed work".to_string())
        })
        .map(|_| {
            if let (Some(msa), Some(id)) = (msa.as_deref(), vivling_id.as_deref()) {
                for capsule in new_capsules.borrow().iter() {
                    // "Capsule ricche": the full pre-truncate turn summary is
                    // only alive here — the adapter gates (turn-kind, low-signal)
                    // and sanitizes. Index artifact only: capsule/state keep the
                    // short summary.
                    msa.index_capsule_rich(id, capsule, summary);
                }
            }
            self.mark_recent_activity(ACTIVE_FOOTER_TAIL);
        })?;

        // codex-vl lineage passive learning (Fase 4 iter 1A): after the
        // active primary has updated its own distilled_summaries via
        // record_turn_completed → maybe_distill_memory →
        // rebuild_learning_profiles, propagate the new/refreshed
        // distillates to all direct children whose cultural parent is
        // this primary. Best-effort: a propagation failure does not
        // mask the successful turn record above.
        let _ = self.propagate_parent_summaries_to_children();
        Ok(())
    }

    pub(crate) fn assign_brain_profile(&mut self, profile: String) -> Result<String, String> {
        self.update_existing(|state| state.assign_brain_profile(profile))
    }

    pub(crate) fn mark_brain_runtime_error(&mut self, error: &str) -> Result<(), String> {
        self.update_existing(|state| {
            state.mark_brain_runtime_error(error);
            state
                .brain_last_error
                .clone()
                .unwrap_or_else(|| "Vivling brain failed.".to_string())
        })
        .map(|_| ())
    }

    pub(crate) fn mark_brain_runtime_error_for(
        &mut self,
        vivling_id: &str,
        error: &str,
    ) -> Result<(), String> {
        let mut state = self
            .load_state_for_id(vivling_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("Vivling `{vivling_id}` is missing on disk."))?;
        state.mark_brain_runtime_error(error);
        self.save_state_record(&state, /*set_active*/ false, state.is_imported)
            .map_err(|err| err.to_string())?;
        if self.active_vivling_id.as_deref() == Some(vivling_id) {
            self.state = Some(state);
        }
        Ok(())
    }

    pub(crate) fn mark_brain_reply(&mut self, reply: &str) -> Result<(), String> {
        self.update_existing(|state| {
            state.mark_brain_reply(reply);
            truncate_summary(reply, MAX_CARD_REPLY_LEN)
        })
        .map(|_| {
            self.mark_recent_activity(ACTIVE_FOOTER_TAIL);
        })
    }

    pub(crate) fn mark_brain_reply_for(
        &mut self,
        vivling_id: &str,
        reply: &str,
    ) -> Result<(), String> {
        let mut state = self
            .load_state_for_id(vivling_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("Vivling `{vivling_id}` is missing on disk."))?;
        state.mark_brain_reply(reply);
        // codex-vl bond: this path is exclusive to successful Vivling loop ticks
        // (only caller is `loop_controller::handle_vivling_loop_tick_finished`
        // on Ok arm). Bond gets +1 LoopTick credit here, never on Err arm.
        state
            .bond
            .record_interaction(crate::vivling::VivlingInteractionKind::LoopTick, Utc::now());
        self.save_state_record(&state, /*set_active*/ false, state.is_imported)
            .map_err(|err| err.to_string())?;
        if self.active_vivling_id.as_deref() == Some(vivling_id) {
            self.state = Some(state);
        }
        self.mark_recent_activity(ACTIVE_FOOTER_TAIL);
        Ok(())
    }

    /// Memory V2 Step 12.B.D.2 — apply a successful Expression LLM
    /// reply to the cache slots of the Vivling identified by
    /// `vivling_id`. The cache fields are `#[serde(skip)]` runtime-only
    /// (Step 12.B.D.1), so the on-disk state is NOT rewritten here;
    /// persistence happened pre-dispatch via `save_state` after the
    /// `try_reserve_llm_call` reservation.
    pub(crate) fn record_expression_result_for(
        &mut self,
        vivling_id: &str,
        reply: &super::expression::VivlingExpressionResult,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        // Step 12.C — chiude il gate expression_in_flight (fail-safe clear).
        self.finish_expression();
        // codex-vl Step 14 Bug 1 fix — first Expression dispatch of
        // this TUI session has resolved (success). Flip the gate that
        // hides state-persistent CRT fallbacks; from now on the chain
        // falls back through `last_work_summary` etc. like before.
        self.crt_first_dispatch_completed.set(true);
        if self.active_vivling_id.as_deref() == Some(vivling_id)
            && let Some(state) = self.state.as_mut()
        {
            super::expression::record_expression_result(state, reply, now);
            return Ok(());
        }
        // Non-active Vivling: the runtime caches live only on the
        // active state struct, so there is nothing to write into a
        // backing record. The reservation counter increment is
        // already persisted on disk from the dispatch path. Treat
        // this as a no-op success rather than an error so a stale
        // background reply for a Vivling the user just switched away
        // from does not surface as a UI failure.
        let _ = vivling_id;
        Ok(())
    }

    /// Memory V2 Step 12.B.D.3 — try to plan + reserve + persist a
    /// fresh Expression dispatch for the active Vivling. Returns the
    /// ready-to-spawn request, or `None` when there is nothing to
    /// dispatch right now (no active state, planner refused, throttle
    /// / dedup / budget / opt-out, save_state failed, …).
    ///
    /// Crash-safety: the daily LLM counter increments mutated by
    /// [`super::expression::try_plan_and_reserve_expression`] are
    /// flushed to disk via `save_state()` BEFORE returning, so the
    /// caller can spawn the dispatch task without risking a re-bill
    /// after a crash. If `save_state` fails the reservation is
    /// abandoned (caller sees `None`) — the planner is deterministic
    /// so a retry on the next turn produces the same prompt hash and
    /// the dedup branch will then short-circuit if the cache happens
    /// to be populated.
    pub(crate) fn try_dispatch_expression_refresh(
        &mut self,
    ) -> Option<super::expression::VivlingExpressionRequest> {
        let live_snapshot = self.live_context.borrow().clone();
        let state = self.state.as_mut()?;
        let now = Utc::now();
        let focus_hint = super::expression::build_focus_hint(state, live_snapshot.as_ref());
        let request = super::expression::try_plan_and_reserve_expression(state, now, focus_hint)?;
        if self.save_state().is_err() {
            return None;
        }
        Some(request)
    }

    /// Memory V2 Step 12.B.H — forced variant invoked by the
    /// `/vivling crt-brain refresh` command. Bypasses the 60s
    /// Expression throttle; budget / opt-out / dedup gates still
    /// apply. Same save-state-before-spawn contract.
    pub(crate) fn try_dispatch_expression_refresh_forced(
        &mut self,
    ) -> Option<super::expression::VivlingExpressionRequest> {
        let live_snapshot = self.live_context.borrow().clone();
        let state = self.state.as_mut()?;
        let now = Utc::now();
        let focus_hint = super::expression::build_focus_hint(state, live_snapshot.as_ref());
        let request =
            super::expression::try_plan_and_reserve_expression_forced(state, now, focus_hint)?;
        if self.save_state().is_err() {
            return None;
        }
        Some(request)
    }

    /// Memory V2 Step 12.B.L — one-shot bootstrap dispatch issued the
    /// first time the chatwidget pre_draw_tick runs after a state
    /// load. Bypasses the 60s throttle (a stale `last_llm_dispatch_at`
    /// from the previous session must not silence the boot greeting);
    /// pre-flight cache-fresh skip is preserved so a still-fresh
    /// phrase from the previous session keeps being shown.
    ///
    /// Idempotency contract: the `startup_dispatched: Cell<bool>` flag
    /// is set UNCONDITIONALLY before the dispatch attempt, so a
    /// failure (planner refused, dedup, budget exhausted, save_state
    /// error) does not cause a retry storm on the next frame. The
    /// flag is wrapper-scoped (lives on the `Vivling` instance, not
    /// in `VivlingState`) and resets naturally on `codex_home` reload
    /// or process restart.
    ///
    /// Returns `None` when no dispatch is warranted (already done,
    /// no state, planner refused, etc.); returns `Some(request)` for
    /// the caller to forward via `VlEvent::RunVivlingExpression`.
    pub(crate) fn try_dispatch_bootstrap_expression(
        &mut self,
    ) -> Option<super::expression::VivlingExpressionRequest> {
        if self.startup_dispatched.get() {
            return None;
        }
        if self.state.is_none() {
            return None;
        }
        // Set BEFORE the dispatch attempt: any refusal downstream
        // must not let the next frame retry.
        self.startup_dispatched.set(true);
        let live_snapshot = self.live_context.borrow().clone();
        let state = self.state.as_mut()?;
        let now = Utc::now();
        let focus_hint = super::expression::build_focus_hint(state, live_snapshot.as_ref());
        let request =
            super::expression::try_plan_and_reserve_expression_bootstrap(state, now, focus_hint)?;
        if self.save_state().is_err() {
            return None;
        }
        Some(request)
    }

    /// Memory V2 Step 12.B.D.4 — loop-event variant of
    /// `try_dispatch_expression_refresh`. Layers Adult-only + 5min
    /// throttle + 50% budget headroom on top of the standard
    /// pipeline. Save-state-before-spawn contract identical to the
    /// turn-driven helper.
    pub(crate) fn try_dispatch_loop_expression_refresh(
        &mut self,
    ) -> Option<super::expression::VivlingExpressionRequest> {
        let live_snapshot = self.live_context.borrow().clone();
        let state = self.state.as_mut()?;
        let now = Utc::now();
        let focus_hint = super::expression::build_focus_hint(state, live_snapshot.as_ref());
        let request =
            super::expression::try_plan_and_reserve_expression_for_loop(state, now, focus_hint)?;
        if self.save_state().is_err() {
            return None;
        }
        Some(request)
    }

    /// Memory V2 Step 12.B.D.2 — bump the persistent
    /// `daily_llm_failure_count` for `vivling_id` after an Expression
    /// LLM call failed (network / parser / model error). Persists via
    /// `save_state_record`. The failure path deliberately does NOT
    /// touch `brain_last_error` (the Expression channel is best-effort
    /// background; failures must not pollute the user-visible error
    /// surface used by `/vl chat` and `/vivling assist`).
    pub(crate) fn record_expression_failure_for(&mut self, vivling_id: &str) -> Result<(), String> {
        // Step 12.C — chiude il gate expression_in_flight (fail-safe clear).
        self.finish_expression();
        // codex-vl Step 14 Bug 1 fix — first Expression dispatch of
        // this TUI session has resolved (failure). Same gate flip as
        // the success path: a stalled / failed dispatch must not
        // freeze the CRT into safety-template-only mode forever, so
        // unlock the persistent fallbacks once any attempt completes.
        self.crt_first_dispatch_completed.set(true);
        let mut state = self
            .load_state_for_id(vivling_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("Vivling `{vivling_id}` is missing on disk."))?;
        super::expression::record_expression_failure(&mut state);
        self.save_state_record(&state, /*set_active*/ false, state.is_imported)
            .map_err(|err| err.to_string())?;
        if self.active_vivling_id.as_deref() == Some(vivling_id) {
            self.state = Some(state);
        }
        Ok(())
    }

    /// codex-vl bond: record the supplementary success bonus on the active
    /// Vivling when a Chat or Assist brain request returned a reply.
    /// Counters stay tied to dispatch — this only modifies `bond.value`.
    /// Called from `vl_handler.rs` `VivlingAssistFinished::Ok(reply)` arm
    /// AFTER `mark_brain_reply`; a failed `mark_brain_reply` must NOT
    /// prevent this call (Codex design review iter 4 §7).
    pub(crate) fn record_brain_success(
        &mut self,
        kind: VivlingBrainRequestKind,
    ) -> Result<(), String> {
        self.ensure_hatched()?;
        let bond_kind = match kind {
            VivlingBrainRequestKind::Chat => {
                crate::vivling::VivlingInteractionKind::BrainChatSucceeded
            }
            VivlingBrainRequestKind::Assist => {
                crate::vivling::VivlingInteractionKind::BrainAssistSucceeded
            }
        };
        let state = self.state.as_mut().expect("state checked");
        state.bond.record_interaction(bond_kind, Utc::now());
        self.save_state().map_err(|err| err.to_string())
    }
}
