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
        // Memory V2 §8.1 (P0.2): the brain dispatch gate is adult + brain
        // enabled. A missing `brain_profile` is no longer a blocker: the
        // dispatcher falls back to `BrainTarget::SessionDefault` and
        // reads the session's `config.model` at run time.
        let should_use_brain = {
            let state = self.state.as_ref().expect("state checked");
            state.stage() == Stage::Adult && state.brain_enabled
        };
        if should_use_brain {
            self.prepare_chat_request(text)
                .map(VivlingCommandOutcome::DispatchAssist)
        } else {
            self.update_existing_result(|state| state.direct_chat_reply(text))
                .map(|reply| format!("Local fallback: {reply}"))
                .map(VivlingCommandOutcome::Message)
        }
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
