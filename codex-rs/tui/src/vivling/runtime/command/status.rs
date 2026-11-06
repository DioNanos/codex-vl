use super::super::*;

/// What `/vivling status` and the help say about the `manage` loop strategy. It
/// needs everything a loop owner needs AND a pinned brain profile (the strategy
/// gate in the loop controller is stricter than the owner check on purpose).
fn managed_loops_label(owner_ready: bool, has_brain_profile: bool) -> &'static str {
    match (owner_ready, has_brain_profile) {
        (true, true) => "managed loops: ready",
        (true, false) => "managed loops: needs brain profile",
        (false, _) => "managed loops: not available until the loop owner is ready",
    }
}

impl Vivling {
    pub(crate) fn help_message(&self) -> String {
        let mut lines = vec![
            "Vivling commands:".to_string(),
            "Ctrl+J - open or close the Vivling sidebar".to_string(),
            "/vivling - open the Vivling dashboard".to_string(),
            "/vivling help - show this list".to_string(),
            "/vivling hatch - hatch a new top-level Vivling while slots are free".to_string(),
            "/vivling status - show active Vivling status and slot usage".to_string(),
            "/vivling roster - list known Vivlings".to_string(),
            "/vivling list - alias for roster".to_string(),
            "/vivling focus <vivling_id_or_name> - switch active Vivling".to_string(),
            "/vivling switch <vivling_id_or_name> - alias for focus (also: select, use)"
                .to_string(),
            "/vivling card - open the current Vivling card".to_string(),
            "/vivling upgrade - open the ZED upgrade card".to_string(),
            "/vivling zed - open the ZED Companion panel (bond + gene snapshot)".to_string(),
            "/vivling assist <task> - ask the Vivling brain for adult help".to_string(),
            "/vla <task> - short alias for Adult assist + main-worker kickoff".to_string(),
            "/vivling brain <on|off> - enable or disable the Vivling brain (deprecated: use `/vivling model <profile>` and `/vivling crt`)"
                .to_string(),
            "/vivling crt-brain - show CRT brain mode, budget and today's LLM counters (alias: crt)"
                .to_string(),
            "/vivling crt-brain <on|off|default> - toggle the live Expression channel".to_string(),
            "/vivling crt-brain refresh - force an Expression refresh now (bypass throttle)"
                .to_string(),
            "/vivling crt-brain budget [default|unlimited|<N>] - show or set the per-Vivling LLM budget cap"
                .to_string(),
            "/vivling crt-brain reset-budget - zero today's daily counters + retry bootstrap"
                .to_string(),
            "/vivling layout <full|line> - collapse the CRT strip to a single row, or restore the classic three-line strip; the choice is saved in config"
                .to_string(),
            "/vivling language - show effective/detected language, override, mode (alias: lang)"
                .to_string(),
            "/vivling language <it|en|es|fr|de> - pin a language override".to_string(),
            "/vivling language auto - clear the override; fall back to detected/LANG".to_string(),
            "/vivling language mode <mirror-user|dominant-only|strict> - change detection policy"
                .to_string(),
            "/vivling model - show the current Vivling brain profile".to_string(),
            "/vivling model list - show assignable Vivling brain models".to_string(),
            "/vivling model <profile> - assign an existing config profile".to_string(),
            "/vivling model <model> [provider] [effort] - create or update the per-Vivling profile (at least two words: a single word after `model` is always read as a profile name)"
                .to_string(),
            "/vivling memory - technical memory digest: level, archetype, counts, top mental paths, recent capsules"
                .to_string(),
            "/vivling recap - summarize learned memory and current focus".to_string(),
            "/vivling promote 10 - apply the early growth baseline".to_string(),
            "/vivling promote 60 - apply the adult seed baseline".to_string(),
            "/vivling mode <on|off> - toggle active help once adult (deprecated: use `/vivling crt <on|off|default>`)"
                .to_string(),
            "/vivling loop-strategy <label> <observe|suggest|manage|auto> - set the delegation strategy of a delegated loop; auto clears the override"
                .to_string(),
            "/vivling spawn - create a local lineage copy once unlocked".to_string(),
            "/vivling export [path.vivegg] - export the active Vivling from level 30".to_string(),
            "/vivling import <path.vivegg> - import a packaged Vivling".to_string(),
            "/vivling remove <vivling_id_or_name> - remove a non-active Vivling".to_string(),
            "/vivling reset - remove the current Vivling state".to_string(),
            "/vivling <message> - local reply from the active Vivling (no model call); use /vl <message> for model-backed chat"
                .to_string(),
            "/vl <message> - chat with the active Vivling through its brain model (local fallback if refused)"
                .to_string(),
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
            // The same predicate `delegate vivling` / `owner vivling` use.
            let loop_owner_ready =
                Self::vivling_runnable_from(&self.shadow.lifecycle, state).is_ok();
            lines.push(
                if loop_owner_ready {
                    "loop-owner eligible: yes"
                } else {
                    "loop-owner eligible: no"
                }
                .to_string(),
            );
            lines.push(
                managed_loops_label(loop_owner_ready, state.brain_profile.is_some()).to_string(),
            );
        }

        lines.join("\n")
    }

    pub(crate) fn dashboard_message(&mut self) -> Result<String, String> {
        let mut lines = Vec::new();
        lines.push("Vivling control".to_string());
        lines.push("Ctrl+J opens the Vivling sidebar.".to_string());

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
            let Some(state) = self.state.as_mut() else {
                return Err("No Vivling yet. Use /vivling hatch.".to_string());
            };
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
                .map(VivlingState::local_spawn_slots_unlocked)
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
        // The same predicate `delegate vivling` / `owner vivling` use, so the status
        // never says "ready" for an owner that the delegation would then refuse.
        let loop_owner_ready =
            Self::vivling_runnable_from(&self.shadow.lifecycle, &snapshot).is_ok();
        status.push_str(if loop_owner_ready {
            " - loop owner ready"
        } else {
            " - loop owner not ready"
        });
        status.push_str(" - ");
        status.push_str(managed_loops_label(
            loop_owner_ready,
            snapshot.brain_profile.is_some(),
        ));
        self.save_state().map_err(|err| err.to_string())?;
        Ok(status)
    }
}
