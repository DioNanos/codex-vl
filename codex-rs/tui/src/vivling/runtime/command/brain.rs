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
        let should_use_brain = {
            let state = self.state.as_ref().expect("state checked");
            state.stage() == Stage::Adult && state.brain_enabled && state.brain_profile.is_some()
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
        {
            let state = self.state.as_ref().expect("state checked");
            if state.stage() != Stage::Adult {
                return Err("Vivling brain unlocks only at level 60.".to_string());
            }
            if state.brain_profile.is_some() {
                return self.update_existing_result(|state| state.set_brain_enabled(true));
            }
        }

        let profiles = self.model_list()?;
        let mut lines =
            vec!["Select a Vivling brain profile before enabling the brain.".to_string()];
        if profiles.contains("Vivling brain profiles:") {
            lines.push("Use `/vivling model <profile>` with one of these profiles:".to_string());
        } else {
            lines.push("Create one with `/vivling model <model> [provider] [effort]`.".to_string());
        }
        lines.push(profiles);
        Ok(lines.join("\n"))
    }
}
