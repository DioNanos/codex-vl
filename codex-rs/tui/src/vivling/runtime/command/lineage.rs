use super::super::*;

impl Vivling {
    pub(crate) fn focus(&mut self, target: &str) -> Result<String, String> {
        let target_id = self
            .resolve_vivling_target(target)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| {
                format!(
                    "No Vivling matches `{target}`. Use /vivling roster to see available Vivlings."
                )
            })?;
        let mut state = self
            .load_state_for_id(&target_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("Vivling `{target_id}` is missing on disk."))?;
        let active_message = format!("{} active", state.name);
        state.last_message = Some(active_message.clone());
        state.last_work_summary = Some(active_message);
        let mut roster = self.load_roster().map_err(|err| err.to_string())?;
        roster.active_vivling_id = Some(target_id.clone());
        self.save_roster(&roster).map_err(|err| err.to_string())?;
        self.active_vivling_id = Some(target_id.clone());
        self.state = Some(state.clone());
        self.save_state().map_err(|err| err.to_string())?;
        Ok(format!(
            "Focused {} [{}] {} Lv {}.",
            state.vivling_id,
            state.lineage_role_label(),
            state.name,
            state.level
        ))
    }

    pub(crate) fn spawn_vivling(&mut self) -> Result<(String, VivlingPanelData), String> {
        self.ensure_hatched()?;
        let primary = self.state.as_ref().expect("state checked").clone();
        if !primary.is_primary {
            return Err("Only a primary Vivling can spawn a local lineage copy.".to_string());
        }
        if primary.level < JUVENILE_LEVEL {
            return Err("`/vivling spawn` unlocks only at level 30.".to_string());
        }
        let lineage_states = self
            .load_lineage_states(&primary.primary_vivling_id)
            .map_err(|err| err.to_string())?;
        let local_spawn_used = lineage_states
            .iter()
            .filter(|entry| !entry.is_primary && !entry.is_imported)
            .count();
        let local_spawn_unlocked = primary.local_spawn_slots_unlocked();
        if local_spawn_used >= local_spawn_unlocked {
            return Err(format!(
                "No free local spawn slots. Used {local_spawn_used}/{local_spawn_unlocked}."
            ));
        }

        let new_id = format!("viv-{}", Uuid::new_v4().simple());
        let instance_label = format!("spawn-{}", local_spawn_used + 1);

        // codex-vl iter 1B: multi-origin sort. The origin is rolled
        // uniformly over the eligible pool; the user never picks it.
        let roll = super::super::super::model::text_utils::fnv1a64(new_id.as_bytes());
        let origin = super::super::spawn_origin::pick_spawn_origin(&primary, &lineage_states, roll)
            .ok_or_else(|| "No eligible spawn origin available.".to_string())?;
        let origin_label = origin.label();
        let mut spawned = super::super::spawn_origin::build_offspring_for_origin(
            &origin,
            &primary,
            new_id.clone(),
            instance_label.clone(),
        );

        let existing_name_count = lineage_states
            .iter()
            .filter(|entry| entry.name == primary.name)
            .count();
        if existing_name_count > 0 {
            spawned.name = format!(
                "{} {}",
                primary.name,
                roman_numeral(existing_name_count + 1)
            );
        }
        self.save_state_record(&spawned, false, false)
            .map_err(|err| err.to_string())?;

        // codex-vl: a successful spawn bumps the primary's lineage
        // rarity pressure for the next offspring's dentro-specie
        // quality roll (DAG design directive 2026-05-15). Failed spawns
        // never reach this point — the early returns above keep the
        // pressure untouched on error paths.
        if let Some(state) = self.state.as_mut() {
            state.lineage_rarity_pressure_pct =
                super::super::super::model::lineage::bump_lineage_rarity_pressure(
                    state.lineage_rarity_pressure_pct,
                );
            let primary_after_bump = state.clone();
            self.save_state_record(&primary_after_bump, /*set_active*/ true, false)
                .map_err(|err| err.to_string())?;
        }

        // codex-vl iter 1C: L1 chat-history message + L2 ZED Lineage
        // panel narration. The newborn stays inactive; the panel makes
        // the lineage event visible as ZED-as-presenter, and the
        // message keeps a quick audit trail in chat history.
        let message = format!(
            "Spawned {} [{}] {} via {}. Bio species: {}. Cultural parent: {}. \
             Child stays inactive. Local spawn slots now {}/{}.",
            spawned.vivling_id,
            instance_label,
            spawned.name,
            origin_label,
            spawned.species,
            primary.name,
            local_spawn_used + 1,
            local_spawn_unlocked
        );
        let summary = super::super::super::zed::zed_summary_for_lineage(
            &primary.name,
            &spawned.name,
            &spawned.species,
            origin_label,
        );
        let zed = super::super::super::zed::zed_panel_data(
            super::super::super::zed::ZedTopic::Lineage,
            &summary,
        );
        let panel = VivlingPanelData {
            title: zed.title,
            narrow_lines: zed.narrow_lines,
            wide_lines: zed.wide_lines,
        };
        Ok((message, panel))
    }
}
