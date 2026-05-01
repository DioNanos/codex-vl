use super::*;

const SYLLO_ID: &str = "syllo";
const ORCHESTRA_ID: &str = "orchestra";
const CHRONOSWORN_ID: &str = "chronosworn";

impl VivlingState {
    /// Fresh-save unlock set: only Syllo is hatchable.
    pub(crate) fn default_unlocked_species() -> Vec<String> {
        vec![SYLLO_ID.to_string()]
    }

    /// Idempotently grant a species id. Returns true when the id was newly
    /// added (so callers can decide whether to surface a notification).
    pub(crate) fn unlock_species(&mut self, id: &str) -> bool {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return false;
        }
        if self
            .unlocked_species
            .iter()
            .any(|existing| existing == trimmed)
        {
            return false;
        }
        self.unlocked_species.push(trimmed.to_string());
        true
    }

    /// Drop empty/duplicate ids and ensure Syllo is always present. Called
    /// from `normalize_loaded_state` so legacy saves without the field
    /// migrate transparently.
    pub(super) fn normalize_unlocked_species(&mut self) {
        let mut seen = std::collections::BTreeSet::new();
        let mut cleaned: Vec<String> = Vec::with_capacity(self.unlocked_species.len() + 1);
        for entry in self.unlocked_species.drain(..) {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                continue;
            }
            if seen.insert(trimmed.to_string()) {
                cleaned.push(trimmed.to_string());
            }
        }
        if !cleaned.iter().any(|id| id == SYLLO_ID) {
            cleaned.insert(0, SYLLO_ID.to_string());
        }
        self.unlocked_species = cleaned;
    }

    /// Stage-up unlock chain. Called when `recompute_level` detects a stage
    /// transition; idempotent — re-running on the same Adult state will not
    /// double-grant. Returns the list of newly granted ids (empty if no-op).
    pub(crate) fn apply_stage_unlocks(&mut self, stage: Stage) -> Vec<String> {
        if stage != Stage::Adult {
            return Vec::new();
        }
        let mut granted = Vec::new();
        match self.species.as_str() {
            SYLLO_ID => {
                if self.unlock_species(ORCHESTRA_ID) {
                    granted.push(ORCHESTRA_ID.to_string());
                }
            }
            ORCHESTRA_ID => {
                if self.unlock_species(CHRONOSWORN_ID) {
                    granted.push(CHRONOSWORN_ID.to_string());
                }
            }
            _ => {}
        }
        granted
    }
}
