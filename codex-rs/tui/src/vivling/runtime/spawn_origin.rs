//! codex-vl multi-origin spawn sort (Fase 4 iter 1B).
//!
//! `/vivling spawn` rolls one of three biological origins **uniformly**
//! over the eligible subset; the user never picks. Cultural parent is
//! always the active primary, regardless of biological origin.
//!
//! Origins:
//! - **PrimaryChild**: clone species/gene from the active primary.
//! - **VeteranChild**: clone species/gene from a non-primary lv30+
//!   roster member.
//! - **ZedHatch**: bypass clone; pick a random unlocked/phase-active
//!   species **different from** the primary's species.
//!
//! Eligibility is filtered before the roll; non-eligible origins are
//! removed from the pool (the roll never fails).

use super::super::model::VivlingGeneVector;
use super::super::model::VivlingState;
use super::super::model::constants::JUVENILE_LEVEL;
use super::super::registry::VivlingSpeciesDefinition;
use super::super::registry::active_species_registry;

/// One of the three biological origins for a new spawn. Holds the
/// payload needed downstream (species clone source or species
/// override).
#[derive(Debug)]
pub(crate) enum SpawnOrigin<'a> {
    PrimaryChild,
    VeteranChild(&'a VivlingState),
    ZedHatch(&'static VivlingSpeciesDefinition),
}

impl SpawnOrigin<'_> {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::PrimaryChild => "primary_child",
            Self::VeteranChild(_) => "veteran_child",
            Self::ZedHatch(_) => "zed_hatch",
        }
    }
}

/// Choose the biological origin for a new spawn given the current
/// roster and a deterministic `roll` value (the caller picks
/// `hash`/`Uuid` bits — this function only does the eligibility math
/// and the modulo pick).
///
/// Returns `None` only in the degenerate case where the primary itself
/// is not eligible (shouldn't happen — the `spawn_vivling` entry
/// already guards that), or the eligible pool ends up empty.
pub(crate) fn pick_spawn_origin<'a>(
    primary: &VivlingState,
    lineage_states: &'a [VivlingState],
    roll: u64,
) -> Option<SpawnOrigin<'a>> {
    let mut pool: Vec<SpawnOrigin<'a>> = Vec::with_capacity(3);

    if is_primary_eligible(primary) {
        pool.push(SpawnOrigin::PrimaryChild);
    }
    if let Some(veteran) = first_eligible_veteran(primary, lineage_states, roll) {
        pool.push(SpawnOrigin::VeteranChild(veteran));
    }
    if let Some(species) = pick_zed_alternative_species(primary, roll) {
        pool.push(SpawnOrigin::ZedHatch(species));
    }

    if pool.is_empty() {
        return None;
    }
    let idx = (roll % pool.len() as u64) as usize;
    Some(pool.swap_remove(idx))
}

fn is_primary_eligible(primary: &VivlingState) -> bool {
    primary.hatched && primary.is_primary && primary.level >= JUVENILE_LEVEL
}

/// Pick the first non-primary lv30+ hatched veteran in the roster.
/// Imports are excluded in iter 1B. When multiple veterans qualify,
/// `roll` (bit-shifted to stay independent of the origin pick) selects
/// one deterministically.
fn first_eligible_veteran<'a>(
    primary: &VivlingState,
    lineage_states: &'a [VivlingState],
    roll: u64,
) -> Option<&'a VivlingState> {
    let veterans: Vec<&VivlingState> = lineage_states
        .iter()
        .filter(|state| {
            state.vivling_id != primary.vivling_id
                && state.hatched
                && state.visible
                && !state.is_imported
                && !state.is_primary
                && state.level >= JUVENILE_LEVEL
        })
        .collect();
    if veterans.is_empty() {
        return None;
    }
    let idx = ((roll >> 16) as usize) % veterans.len();
    Some(veterans[idx])
}

/// Pick a ZED-origin species: any unlocked + phase-active definition
/// whose `id` differs from the primary's species. Returns `None` when
/// no alternative exists.
fn pick_zed_alternative_species(
    primary: &VivlingState,
    roll: u64,
) -> Option<&'static VivlingSpeciesDefinition> {
    let alternatives: Vec<&'static VivlingSpeciesDefinition> = active_species_registry()
        .into_iter()
        .filter(|species| {
            primary.unlocked_species.iter().any(|id| id == &species.id)
                && species.id != primary.species
        })
        .collect();
    if alternatives.is_empty() {
        return None;
    }
    let idx = ((roll >> 32) as usize) % alternatives.len();
    Some(alternatives[idx])
}

/// Build the offspring `VivlingState` for a given `SpawnOrigin`, with
/// the cultural-parent override applied (`cultural_parent_vivling_id`
/// is always the active primary, regardless of biological origin).
///
/// ZED-origin specifics (DAG design directive 2026-05-15):
/// - `species` and `rarity` come from the picked alternative (not the
///   primary's clone);
/// - `gene_vector` is **freshly generated** from the offspring's own
///   `seed_hash`, **not** inherited from the primary. The primary
///   determines culture, never biology, on ZED-origin hatches.
///
/// Primary- and Veteran-origin keep the existing semantics of
/// `create_spawned_offspring` (species + gene clone from the bio
/// parent, then mutated by `inherit_from`).
pub(crate) fn build_offspring_for_origin(
    origin: &SpawnOrigin<'_>,
    primary: &VivlingState,
    new_id: String,
    instance_label: String,
) -> VivlingState {
    let mut spawned = match origin {
        SpawnOrigin::PrimaryChild => primary.create_spawned_offspring(new_id, instance_label),
        SpawnOrigin::VeteranChild(veteran) => {
            veteran.create_spawned_offspring(new_id, instance_label)
        }
        SpawnOrigin::ZedHatch(species) => {
            let mut child = primary.create_spawned_offspring(new_id, instance_label);
            child.species = species.id.clone();
            child.rarity = species.rarity.label().to_string();
            // Fresh biology: ZED introduces a new bloodline so the
            // gene_vector must not carry the primary's inheritance.
            child.gene_vector = VivlingGeneVector::generate(&child.seed_hash);
            child
        }
    };

    spawned.cultural_parent_vivling_id = Some(primary.vivling_id.clone());
    spawned
}
