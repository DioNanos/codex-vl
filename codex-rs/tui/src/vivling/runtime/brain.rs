use super::super::model::VivlingWorkMemoryEntry;
use super::brain_context::BrainPromptKind;
use super::brain_context::compose_brain_prompt;
use super::*;

impl Vivling {
    pub(crate) fn prepare_assist_request(
        &mut self,
        task: &str,
    ) -> Result<VivlingAssistRequest, String> {
        self.ensure_hatched()?;
        let live_snapshot = self.live_context.borrow().clone();
        let (vivling_id, vivling_name, brain_profile, prompt_context, task) = {
            let state = self.state.as_mut().expect("state checked");
            let now = Utc::now();
            state.apply_decay(now);
            let prompt_context = compose_brain_prompt(
                state,
                BrainPromptKind::Assist,
                task,
                live_snapshot.as_ref(),
                self.msa.as_deref(),
            )?;
            let brain_profile = state.brain_profile.clone().ok_or_else(|| {
                "Set a Vivling brain profile first with `/vivling model ...`.".to_string()
            })?;
            // codex-vl bond: only credit Assist after pre-dispatch validation
            // succeeds, so a failed precondition does not mutate bond state.
            state
                .bond
                .record_interaction(crate::vivling::VivlingInteractionKind::Assist, now);
            (
                state.vivling_id.clone(),
                state.name.clone(),
                brain_profile,
                prompt_context,
                task.trim().to_string(),
            )
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(VivlingAssistRequest {
            vivling_id,
            vivling_name,
            brain_profile,
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
        let (vivling_id, vivling_name, brain_profile, prompt_context, task) = {
            let state = self.state.as_mut().expect("state checked");
            let now = Utc::now();
            state.apply_decay(now);
            let prompt_context = compose_brain_prompt(
                state,
                BrainPromptKind::Chat,
                text,
                live_snapshot.as_ref(),
                self.msa.as_deref(),
            )?;
            let brain_profile = state.brain_profile.clone().ok_or_else(|| {
                "Set a Vivling brain profile first with `/vivling model ...`.".to_string()
            })?;
            // codex-vl bond: only credit Chat after pre-dispatch validation
            // succeeds, so a failed precondition does not mutate bond state.
            state
                .bond
                .record_interaction(crate::vivling::VivlingInteractionKind::Chat, now);
            (
                state.vivling_id.clone(),
                state.name.clone(),
                brain_profile,
                prompt_context,
                text.trim().to_string(),
            )
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(VivlingAssistRequest {
            vivling_id,
            vivling_name,
            brain_profile,
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
        if state.brain_profile.is_none() {
            return Err("Set a Vivling brain profile first with `/vivling model ...`.".to_string());
        }
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
        let brain_profile = state.brain_profile.clone().ok_or_else(|| {
            format!(
                "Vivling owner `{}` has no brain profile configured.",
                state.name
            )
        })?;
        let goal = job
            .goal_text
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&job.prompt_text)
            .to_string();
        let live_snapshot = self.live_context.borrow().clone();
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
        )?;
        Ok(VivlingLoopTickRequest {
            vivling_id: state.vivling_id.clone(),
            vivling_name: state.name.clone(),
            brain_profile,
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
        })
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
                    msa.index_capsule(id, capsule);
                }
            }
            self.mark_recent_activity(ACTIVE_FOOTER_TAIL);
        })
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
