//! codex-vl multi-origin spawn sort (Fase 4 iter 1B) — unit tests.
//!
//! Pin the eligibility filter and the uniform sort behaviour:
//! - PrimaryChild always eligible when caller invariants hold (the
//!   `spawn_vivling` entry already enforces hatched + is_primary + lv30+);
//! - VeteranChild eligible only when a non-primary lv30+ hatched
//!   visible non-imported state exists in the roster;
//! - ZedHatch eligible only when an unlocked + phase-active species
//!   different from the primary's species exists;
//! - non-eligible origins are removed from the pool (the roll never
//!   fails when at least one origin is eligible).

use crate::vivling::model::SeedIdentity;
use crate::vivling::model::VivlingState;
use crate::vivling::model::constants::JUVENILE_LEVEL;
use crate::vivling::model::hatch_species_from_unlocked;
use crate::vivling::runtime::spawn_origin::SpawnOrigin;
use crate::vivling::runtime::spawn_origin::pick_spawn_origin;

fn primary_at_level(level: u64, species: &str) -> VivlingState {
    let unlocked = vec![species.to_string(), "orchestra".to_string()];
    let species_def = hatch_species_from_unlocked(0xdead_beef_0001, &unlocked);
    let mut state = VivlingState::new_with_species_and_unlocks(
        SeedIdentity {
            value: "primary-seed".to_string(),
            install_id: Some("viv-primary".to_string()),
        },
        species_def,
        unlocked,
    );
    // Force the species id to the requested one (registry pick can land
    // on any of the unlocked); tests assert species behaviour so we
    // pin it explicitly here.
    state.species = species.to_string();
    state.level = level;
    state.work_xp = level * 100;
    state
}

fn veteran(parent_id: &str, level: u64, species: &str) -> VivlingState {
    let mut child = primary_at_level(level, species);
    child.vivling_id = format!("viv-veteran-{level}");
    child.is_primary = false;
    child.parent_vivling_id = Some(parent_id.to_string());
    child.primary_vivling_id = parent_id.to_string();
    child
}

#[test]
fn primary_only_pool_returns_primary_child() {
    let primary = primary_at_level(JUVENILE_LEVEL, "syllo");
    // Constrain the unlocked set to the primary's species so ZED has no
    // alternative — primary remains the only eligible origin.
    let mut primary = primary;
    primary.unlocked_species = vec!["syllo".to_string()];
    let lineage = Vec::new();
    let origin = pick_spawn_origin(&primary, &lineage, 0).expect("primary always eligible");
    assert!(matches!(origin, SpawnOrigin::PrimaryChild));
}

#[test]
fn veteran_origin_eligible_when_lv30_non_primary_exists() {
    let mut primary = primary_at_level(JUVENILE_LEVEL, "syllo");
    primary.unlocked_species = vec!["syllo".to_string()]; // disable ZED
    let veteran = veteran(&primary.vivling_id, JUVENILE_LEVEL, "syllo");
    let lineage = vec![veteran];
    // Roll 1: with primary + veteran in pool (pool len 2), idx = 1 % 2 = 1 → veteran.
    let origin = pick_spawn_origin(&primary, &lineage, 1).expect("pool not empty");
    assert!(
        matches!(origin, SpawnOrigin::VeteranChild(_)),
        "roll=1 with pool=[primary,veteran] must pick veteran",
    );
}

#[test]
fn veteran_origin_excluded_when_no_qualifying_member() {
    let mut primary = primary_at_level(JUVENILE_LEVEL, "syllo");
    primary.unlocked_species = vec!["syllo".to_string()];
    let mut sub_30 = veteran(&primary.vivling_id, JUVENILE_LEVEL - 1, "syllo");
    sub_30.level = JUVENILE_LEVEL - 1;
    let lineage = vec![sub_30];
    // Any roll: only Primary in pool.
    for roll in [0u64, 1, 42, 999] {
        let origin = pick_spawn_origin(&primary, &lineage, roll).expect("primary always eligible");
        assert!(
            matches!(origin, SpawnOrigin::PrimaryChild),
            "sub-30 veteran must be excluded from the pool (roll={roll})",
        );
    }
}

#[test]
fn veteran_origin_excludes_imported_and_primary_self() {
    let mut primary = primary_at_level(JUVENILE_LEVEL, "syllo");
    primary.unlocked_species = vec!["syllo".to_string()];
    let mut imported = veteran(&primary.vivling_id, JUVENILE_LEVEL, "syllo");
    imported.is_imported = true;
    let lineage = vec![imported];
    for roll in [0u64, 7, 13] {
        let origin = pick_spawn_origin(&primary, &lineage, roll).expect("primary always eligible");
        assert!(
            matches!(origin, SpawnOrigin::PrimaryChild),
            "imported veteran must be excluded",
        );
    }
}

#[test]
fn zed_origin_eligible_when_alt_species_unlocked() {
    let mut primary = primary_at_level(JUVENILE_LEVEL, "syllo");
    primary.unlocked_species = vec!["syllo".to_string(), "orchestra".to_string()];
    let lineage = Vec::new();
    // Roll picks idx 1 in [Primary, Zed]: 1 % 2 = 1 → Zed.
    let origin = pick_spawn_origin(&primary, &lineage, 1).expect("pool not empty");
    match origin {
        SpawnOrigin::ZedHatch(species) => {
            assert_ne!(
                species.id, primary.species,
                "ZED must pick a species different from the primary's",
            );
        }
        _ => panic!("expected ZedHatch with roll=1 on [Primary,Zed]"),
    }
}

#[test]
fn zed_origin_excluded_when_no_alternative_species() {
    let mut primary = primary_at_level(JUVENILE_LEVEL, "syllo");
    primary.unlocked_species = vec!["syllo".to_string()];
    let lineage = Vec::new();
    for roll in [0u64, 1, 5, 99] {
        let origin = pick_spawn_origin(&primary, &lineage, roll).expect("primary always eligible");
        assert!(
            matches!(origin, SpawnOrigin::PrimaryChild),
            "ZED must be excluded when no alt species unlocked (roll={roll})",
        );
    }
}

#[test]
fn full_pool_three_origins_sort_uniformly() {
    let mut primary = primary_at_level(JUVENILE_LEVEL, "syllo");
    primary.unlocked_species = vec!["syllo".to_string(), "orchestra".to_string()];
    let vet = veteran(&primary.vivling_id, JUVENILE_LEVEL, "syllo");
    let lineage = vec![vet];

    // With pool len 3 and the modulo dispatch, rolls 0,1,2 cover all
    // three pool slots (Primary, Veteran, Zed). Order is implementation
    // detail; what matters is that each variant *can* be picked.
    let mut seen_primary = false;
    let mut seen_veteran = false;
    let mut seen_zed = false;
    for roll in 0..3 {
        let origin = pick_spawn_origin(&primary, &lineage, roll).expect("pool not empty");
        match origin {
            SpawnOrigin::PrimaryChild => seen_primary = true,
            SpawnOrigin::VeteranChild(_) => seen_veteran = true,
            SpawnOrigin::ZedHatch(_) => seen_zed = true,
        }
    }
    assert!(
        seen_primary,
        "rolls 0..3 must include PrimaryChild at least once"
    );
    assert!(
        seen_veteran,
        "rolls 0..3 must include VeteranChild at least once"
    );
    assert!(seen_zed, "rolls 0..3 must include ZedHatch at least once");
}

#[test]
fn empty_pool_when_primary_below_level_30_returns_none() {
    let mut primary = primary_at_level(JUVENILE_LEVEL - 1, "syllo");
    primary.unlocked_species = vec!["syllo".to_string()];
    let lineage = Vec::new();
    assert!(
        pick_spawn_origin(&primary, &lineage, 0).is_none(),
        "below lv30 primary + no alts + no veterans → empty pool",
    );
}
