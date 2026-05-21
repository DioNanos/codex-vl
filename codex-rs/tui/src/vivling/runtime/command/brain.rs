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
        // Memory V2 Step 12.B.C: `/vl` chat decoupled from
        // `brain_enabled`. Stage decides the path:
        //   - Baby: local-ack, no LLM (the Vivling watches and learns).
        //   - Juvenile/Adult: try to reserve a Chat-kind LLM call;
        //     on success dispatch through the brain (session model
        //     unless `brain on` + pinned profile); on Err
        //     (budget/throttle/etc.) fall back to the local template
        //     so chat never silently fails.
        let stage = self.state.as_ref().expect("state checked").stage();
        if stage == Stage::Baby {
            return self
                .update_existing_result(|state| {
                    let language = state.language_state.effective_language(None);
                    Ok(Self::baby_local_ack(&state.name, &language))
                })
                .map(VivlingCommandOutcome::Message);
        }

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
            Err(_reason) => self
                .update_existing_result(|state| state.direct_chat_reply(text))
                .map(|reply| format!("Local fallback: {reply}"))
                .map(VivlingCommandOutcome::Message),
        }
    }

    /// Memory V2 Step 12.B.C — bounded language-aware local reply
    /// the Baby Vivling shows for every `/vl <text>` (no LLM spend).
    /// Bounded to 80 chars so the chat panel never overflows.
    fn baby_local_ack(name: &str, language: &str) -> String {
        // Note: the `name` is already redacted upstream (V8→V9 save
        // path normalises it); we still keep this output bounded to
        // 80 chars as defence in depth against future state-mutation
        // bugs.
        let raw = match language {
            "it" => format!("{name} osserva e impara."),
            "es" => format!("{name} observa y aprende."),
            "fr" => format!("{name} observe et apprend."),
            "de" => format!("{name} beobachtet und lernt."),
            _ => format!("{name} watches and learns."),
        };
        codex_vivling_core::model::truncate_summary(&raw, 80)
    }

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
