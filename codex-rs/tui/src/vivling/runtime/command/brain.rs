use super::super::*;

impl Vivling {
    /// codex-vl ZED Companion panel: bond + gene snapshot dispatched through
    /// the existing `OpenUpgrade` ZED channel (per Codex design review iter 1
    /// §1: do not reuse `OpenCard` which opens `VivlingCardView` and would
    /// blur the identity-card / ZED-panel separation).
    pub(crate) fn open_zed_companion(&mut self) -> Result<VivlingPanelData, String> {
        self.ensure_hatched()?;
        let panel = {
            let state = self.state.as_mut().expect("state checked");
            state.apply_decay(Utc::now());
            let summary = super::super::super::zed::zed_companion_summary(state);
            state.last_zed_topic = Some("companion".to_string());
            let zed = zed_panel_data(super::super::super::zed::ZedTopic::Companion, &summary);
            VivlingPanelData {
                title: zed.title,
                narrow_lines: zed.narrow_lines,
                wide_lines: zed.wide_lines,
            }
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(panel)
    }

    pub(crate) fn chat(&mut self, text: &str) -> Result<VivlingCommandOutcome, String> {
        self.ensure_hatched()?;
        // Memory V2 Step 12.B.E (post-alpha smoke test): every stage
        // dispatches `/vl` through the LLM. Stage tone/scope is now
        // modulated by `stage_guidance_section` in the brain prompt
        // (Baby = tiny voice, observing, simple words; Juvenile =
        // observations + advice; Adult = full agency). The previous
        // Baby local-ack path is removed — preserving the "true value"
        // of LLM responses across the whole lifecycle.
        //
        // Budget/throttle/etc. still applies via `try_reserve_llm_call`;
        // on Err the local template fallback still answers so chat
        // never silently fails. Step 12.B.K: the fallback marker now
        // names the skip reason so the user can tell budget exhaustion
        // apart from throttle/dedup/opt-out without `/vivling crt-brain
        // show`.
        let now = Utc::now();
        let reservation = {
            let state = self.state.as_mut().expect("state checked");
            state.try_reserve_llm_call(
                codex_vivling_core::model::VivlingLlmCallKind::Chat,
                now,
                None,
            )
        };
        match reservation {
            Ok(()) => {
                // Persist the reservation before dispatch so a crash
                // between here and the response handler cannot let
                // the Vivling spend past its daily budget.
                self.save_state().map_err(|err| err.to_string())?;
                self.prepare_chat_request(text)
                    .map(VivlingCommandOutcome::DispatchAssist)
            }
            Err(reason) => {
                let label = {
                    let state = self.state.as_ref().expect("state checked");
                    chat_skip_reason_label(reason, state)
                };
                self.update_existing_result(|state| state.direct_chat_reply(text))
                    .map(|reply| format!("Local fallback ({label}): {reply}"))
                    .map(VivlingCommandOutcome::Message)
            }
        }
    }

    // Step 12.B.K — human-readable label for the `/vl` fallback
    // marker. We only spell out the skip reason in chat (no internal
    // counters dump): users who want the full picture run `/vivling
    // crt-brain show`.
}

fn chat_skip_reason_label(
    reason: codex_vivling_core::model::LlmCallSkipReason,
    state: &VivlingState,
) -> String {
    use codex_vivling_core::model::LlmCallSkipReason;
    use codex_vivling_core::model::stage_llm_budget;
    match reason {
        LlmCallSkipReason::BudgetExhausted => {
            let cap = stage_llm_budget(state.stage());
            format!(
                "daily LLM budget {}/{} exhausted, resets UTC midnight",
                state.daily_llm_call_count, cap
            )
        }
        LlmCallSkipReason::Throttle => "throttled — wait a few seconds and retry".to_string(),
        LlmCallSkipReason::Dedup => "deduplicated — same prompt is still cached".to_string(),
        LlmCallSkipReason::OptOut => {
            "CRT brain off — toggle with `/vivling crt-brain on`".to_string()
        }
        LlmCallSkipReason::NotEligibleStage => "not eligible for this stage".to_string(),
    }
}

impl Vivling {
    pub(crate) fn set_brain_enabled_with_guidance(
        &mut self,
        enabled: bool,
    ) -> Result<String, String> {
        if !enabled {
            return self.update_existing_result(|state| state.set_brain_enabled(false));
        }
        self.ensure_hatched()?;
        // Memory V2 §8.1 (P0.2): brain enable is gated on adulthood
        // only. With no pinned `brain_profile`, the brain dispatcher
        // inherits from the session (`BrainTarget::SessionDefault`).
        // The previous "you must pick a profile first" hard block made
        // the inheritance path unreachable from the normal flow.
        let needs_guidance = {
            let state = self.state.as_ref().expect("state checked");
            if state.stage() != Stage::Adult {
                return Err("Vivling brain unlocks only at level 60.".to_string());
            }
            state.brain_profile.is_none()
        };
        let enable_message = self.update_existing_result(|state| state.set_brain_enabled(true))?;
        if !needs_guidance {
            return Ok(enable_message);
        }
        let profiles = self.model_list().unwrap_or_default();
        let mut lines = vec![
            enable_message,
            "No brain profile pinned: this Vivling will use the active session's model."
                .to_string(),
            "To pin a specific brain instead, use `/vivling model <profile>`.".to_string(),
        ];
        if profiles.contains("Vivling brain profiles:") {
            lines.push("Available profiles:".to_string());
        } else if !profiles.is_empty() {
            lines.push(
                "Hint: `/vivling model <model> [provider] [effort]` to create one.".to_string(),
            );
        }
        if !profiles.is_empty() {
            lines.push(profiles);
        }
        Ok(lines.join("\n"))
    }
}
