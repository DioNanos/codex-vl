use super::species_registry;
use super::types::VivlingSpeciesDefinition;

// 0.126.0 canonical pilot: only Syllo and Orchestra appear in the random
// hatch pool. Other Common/Rare species stay in the registry (so saves for
// legacy ids continue to resolve via `species_for_id`) but are filtered out
// of the active roster. Chronosworn and ZED are gated by rarity already.
const PHASE_ACTIVE_IDS: &[&str] = &["syllo", "orchestra"];

fn is_phase_active(id: &str) -> bool {
    PHASE_ACTIVE_IDS.iter().any(|candidate| *candidate == id)
}

pub(crate) fn active_species_registry() -> Vec<&'static VivlingSpeciesDefinition> {
    species_registry()
        .iter()
        .filter(|species| species.availability.is_user_visible())
        .filter(|species| is_phase_active(&species.id))
        .collect()
}

/// Pick a hatchable species honouring the caller's unlock set. Falls back to
/// Syllo when the unlocked intersection is empty (defensive: fresh saves and
/// migrated legacy saves always include Syllo via `normalize_unlocked_species`).
pub(crate) fn hatch_species_from_unlocked(
    hash: u64,
    unlocked: &[String],
) -> &'static VivlingSpeciesDefinition {
    let candidates: Vec<&'static VivlingSpeciesDefinition> = active_species_registry()
        .into_iter()
        .filter(|species| unlocked.iter().any(|id| id == &species.id))
        .collect();
    if candidates.is_empty() {
        return species_for_id("syllo");
    }
    let idx = (hash as usize) % candidates.len();
    candidates[idx]
}

/// Fresh-hatch helper: uses the canonical fresh unlock set (Syllo only).
#[cfg(test)]
pub(crate) fn hatch_species(hash: u64) -> &'static VivlingSpeciesDefinition {
    hatch_species_from_unlocked(hash, &[String::from("syllo")])
}

pub(crate) fn species_for_id(id: &str) -> &'static VivlingSpeciesDefinition {
    species_registry()
        .iter()
        .find(|species| species.id == id)
        .unwrap_or(&species_registry()[0])
}
