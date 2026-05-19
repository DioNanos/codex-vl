use super::super::*;

impl Vivling {
    pub(crate) fn hatch(&mut self) -> Result<String, String> {
        let top_level_used = self.top_level_slot_usage().map_err(|err| err.to_string())?;
        if top_level_used >= EXTERNAL_SLOT_LIMIT {
            return Err(format!(
                "All top-level Vivling slots are full ({EXTERNAL_SLOT_LIMIT}/{EXTERNAL_SLOT_LIMIT})."
            ));
        }
        let Some(seed) = self.seed_identity() else {
            return Err("Vivling cannot find CODEX_HOME yet.".to_string());
        };
        let unlocked_species = self.hatch_unlock_set().map_err(|err| err.to_string())?;
        let hash = crate::vivling::model::text_utils::fnv1a64(seed.value.as_bytes());
        let species = hatch_species_from_unlocked(hash, &unlocked_species);
        let mut state = VivlingState::new_with_species_and_unlocks(seed, species, unlocked_species);
        if top_level_used > 0 {
            let new_id = format!("viv-{}", Uuid::new_v4().simple());
            state.vivling_id = new_id.clone();
            state.primary_vivling_id = new_id;
        }
        self.active_vivling_id = Some(state.vivling_id.clone());
        let species = species_for_id(&state.species);
        let message = format!(
            "A {} {} hatched. Its name is {}. Top-level slots now {}/{}.",
            state.rarity,
            species.name,
            state.name,
            top_level_used + 1,
            EXTERNAL_SLOT_LIMIT
        );
        self.state = Some(state.clone());
        self.save_state_record(&state, true, false)
            .map_err(|err| err.to_string())?;
        Ok(message)
    }

    fn hatch_unlock_set(&self) -> io::Result<Vec<String>> {
        let mut raw = self
            .state
            .as_ref()
            .map(|state| state.unlocked_species.clone())
            .unwrap_or_else(VivlingState::default_unlocked_species);
        let roster = self.load_roster()?;
        for vivling_id in roster.vivling_ids {
            if let Some(state) = self.load_state_for_id(&vivling_id)? {
                for species_id in state.unlocked_species {
                    raw.push(species_id);
                }
            }
        }
        Ok(normalized_hatch_unlock_set(raw))
    }
}

fn normalized_hatch_unlock_set(raw: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut unlocked = Vec::new();
    for entry in raw {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            unlocked.push(trimmed.to_string());
        }
    }
    for default_id in VivlingState::default_unlocked_species() {
        if !unlocked.iter().any(|id| id == &default_id) {
            unlocked.insert(0, default_id);
        }
    }
    unlocked
}
