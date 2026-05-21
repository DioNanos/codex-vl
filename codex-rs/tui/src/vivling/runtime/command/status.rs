use super::super::*;

impl Vivling {
    pub(crate) fn help_message(&self) -> String {
        let mut lines = vec![
            "Vivling commands:".to_string(),
            "Ctrl+J - open or close the Vivling chat panel".to_string(),
            "/vivling hatch - hatch a new top-level Vivling while slots are free".to_string(),
            "/vivling status - show active Vivling status and slot usage".to_string(),
            "/vivling roster - list known Vivlings".to_string(),
            "/vivling list - alias for roster".to_string(),
            "/vivling focus <vivling_id_or_name> - switch active Vivling".to_string(),
            "/vivling switch <vivling_id_or_name> - alias for focus".to_string(),
            "/vivling card - open the current Vivling card".to_string(),
            "/vivling upgrade - open the ZED upgrade card".to_string(),
            "/vivling zed - open the ZED Companion panel (bond + gene snapshot)".to_string(),
            "/vivling assist <task> - ask the Vivling brain for adult help".to_string(),
            "/vivling brain <on|off> - enable or disable the Vivling brain".to_string(),
            "/vivling model - show the current Vivling brain profile".to_string(),
            "/vivling model list - show assignable Vivling brain models".to_string(),
            "/vivling model <profile> - assign an existing config profile".to_string(),
            "/vivling model <model> [provider] [effort] - create or update the per-Vivling profile"
                .to_string(),
            "/vivling recap - summarize learned memory and current focus".to_string(),
            "/vivling promote 10 - apply the early growth baseline".to_string(),
            "/vivling promote 60 - apply the adult seed baseline".to_string(),
            "/vivling mode <on|off> - toggle active help once adult".to_string(),
            "/vivling spawn - create a local lineage copy once unlocked".to_string(),
            "/vivling export [path.vivegg] - export the active Vivling from level 30".to_string(),
            "/vivling import <path.vivegg> - import a packaged Vivling".to_string(),
            "/vivling remove <vivling_id_or_name> - remove a non-active Vivling".to_string(),
            "/vivling reset - remove the current Vivling state".to_string(),
            "/vivling <message> - talk directly to the active Vivling".to_string(),
            "/vl <message> - short alias for direct Vivling chat".to_string(),
        ];

        if let Some(state) = self.state.as_ref().filter(|state| state.hatched) {
            lines.push(String::new());
            lines.push(format!(
                "Current: {} {} Lv {} [{}]",
                state.name,
                species_for_id(&state.species).name,
                state.level,
                state.lineage_role_label()
            ));
            lines.push(state.brain_summary());
            // Memory V2 §8.1 (P0.2): loop ownership readiness is gated
            // on adult + brain enabled. Missing `brain_profile` no
            // longer blocks readiness — the dispatcher falls back to
            // SessionDefault. Mirror the runtime's `active_loop_owner_identity`.
            let loop_owner_ready = state.stage() == Stage::Adult && state.brain_enabled;
            lines.push(
                if loop_owner_ready {
                    "loop-owner eligible: yes"
                } else {
                    "loop-owner eligible: no"
                }
                .to_string(),
            );
        }

        lines.join("\n")
    }

    pub(crate) fn dashboard_message(&mut self) -> Result<String, String> {
        let mut lines = Vec::new();
        lines.push("Vivling control".to_string());
        lines.push("Ctrl+J opens the Vivling chat panel.".to_string());

        match self.status() {
            Ok(status) => lines.push(format!("Active: {status}")),
            Err(_) => lines.push("No active Vivling. Use /vivling hatch.".to_string()),
        }

        match self.roster_summary() {
            Ok(roster) => {
                lines.push(String::new());
                lines.push(roster);
            }
            Err(_) => {
                lines.push(String::new());
                lines.push("Roster: empty".to_string());
            }
        }

        lines.push(String::new());
        lines.push("Quick commands:".to_string());
        lines.push("/vivling hatch".to_string());
        lines.push("/vivling roster or /vivling list".to_string());
        lines.push("/vivling focus <id|name|alias> or /vivling switch <id|name|alias>".to_string());
        lines.push("/vivling card".to_string());
        lines.push("/vivling help".to_string());

        Ok(lines.join("\n"))
    }

    pub(crate) fn status(&mut self) -> Result<String, String> {
        self.ensure_hatched()?;
        let snapshot = {
            let state = self.state.as_mut().expect("state checked");
            state.apply_decay(Utc::now());
            state.clone()
        };
        let lineage_states = self
            .load_lineage_states(&snapshot.primary_vivling_id)
            .map_err(|err| err.to_string())?;
        let local_spawn_used = lineage_states
            .iter()
            .filter(|entry| !entry.is_primary && !entry.is_imported)
            .count();
        let local_spawn_unlocked = if snapshot.is_primary {
            snapshot.local_spawn_slots_unlocked()
        } else {
            lineage_states
                .iter()
                .find(|entry| entry.vivling_id == snapshot.primary_vivling_id)
                .map(|entry| entry.local_spawn_slots_unlocked())
                .unwrap_or(0)
        };
        let mut status = format!(
            "{} - local spawn slots {}/{} - top-level slots {}/{}",
            snapshot.status_summary(),
            local_spawn_used,
            local_spawn_unlocked,
            self.top_level_slot_usage().map_err(|err| err.to_string())?,
            EXTERNAL_SLOT_LIMIT
        );
        // Memory V2 §8.1 (P0.2): loop ownership readiness mirrors
        // `active_loop_owner_identity` — adult + brain_enabled is
        // sufficient. `brain_profile = None` resolves to
        // `BrainTarget::SessionDefault` at dispatch time.
        let loop_owner_ready = snapshot.stage() == Stage::Adult && snapshot.brain_enabled;
        status.push_str(if loop_owner_ready {
            " - loop owner ready"
        } else {
            " - loop owner not ready"
        });
        self.save_state().map_err(|err| err.to_string())?;
        Ok(status)
    }
}
