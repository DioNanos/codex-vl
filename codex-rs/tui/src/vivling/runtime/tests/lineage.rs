use super::common::*;
use crate::vivling::model::VivlingGeneVector;

#[test]
fn spawn_requires_level_30_and_persists_new_roster_member() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = configured_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::Hatch, temp.path())
        .expect("hatch");
    let err = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect_err("spawn should be gated");
    assert!(err.contains("level 30"));

    let mut state = vivling.state.clone().expect("hatched state");
    state.level = 30;
    state.active_work_days = 30;
    state.work_xp = WORK_XP_PER_LEVEL * 29;
    state.xp = state.work_xp;
    state.recompute_level();
    vivling.active_vivling_id = Some(state.vivling_id.clone());
    vivling.state = Some(state.clone());
    vivling.save_state().expect("save primary");

    let message = match vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect("spawn should work")
    {
        VivlingCommandOutcome::SpawnNarration { message, .. } => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(message.contains("Spawned"));

    let roster = vivling.load_roster().expect("load roster");
    assert_eq!(roster.vivling_ids.len(), 2);
    let spawned_id = roster
        .vivling_ids
        .iter()
        .find(|entry| *entry != &state.vivling_id)
        .expect("spawned id");
    let spawned = vivling
        .load_state_for_id(spawned_id)
        .expect("load spawned")
        .expect("spawned state");
    assert_eq!(spawned.primary_vivling_id, state.primary_vivling_id);
    assert!(!spawned.is_primary);
    assert_eq!(spawned.lineage_role_label(), "spawned");
    assert_eq!(spawned.level, 1);
    assert_eq!(spawned.work_xp, 0);
    assert_eq!(spawned.active_work_days, 0);
    assert!(!spawned.brain_enabled);
    assert!(spawned.brain_profile.is_none());
    assert!(spawned.work_memory.is_empty());
    assert!(spawned.distilled_summaries.is_empty());
    assert_eq!(spawned.loop_exposure, 0);
    assert_eq!(spawned.turns_observed, 0);
    assert_ne!(spawned.gene_vector, VivlingGeneVector::default());
    assert_ne!(spawned.gene_vector, state.gene_vector);
    let mut inherited = [0usize; 4];
    for index in 0..4 {
        if (spawned.gene_vector.affinity_mod[index] - state.gene_vector.affinity_mod[index]).abs()
            <= 0.05
        {
            inherited[index] = 1;
        }
    }
    assert!(
        inherited.iter().sum::<usize>() >= 2,
        "spawn should inherit at least two close gene affinities"
    );
}

#[test]
fn remove_blocks_active_vivling() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = configured_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::Hatch, temp.path())
        .expect("hatch");
    let active_id = vivling.active_vivling_id.clone().expect("active id");
    let err = vivling
        .command(VivlingAction::Remove(active_id), temp.path())
        .expect_err("active remove should fail");
    assert!(err.contains("Focus another one first"));
}

#[test]
fn spawn_slot_progression_enforces_level_30_60_90_thresholds() {
    for (level, expected_capacity) in [(29, 0usize), (30, 1), (60, 2), (90, 3)] {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = hatched_vivling(temp.path());
        let primary = set_active_level(&mut vivling, level);

        for spawn_index in 0..expected_capacity {
            let message = match vivling
                .command(VivlingAction::Spawn, temp.path())
                .expect("spawn attempt")
            {
                VivlingCommandOutcome::SpawnNarration { message, .. } => message,
                other => panic!("unexpected outcome: {other:?}"),
            };
            assert!(message.contains("Local spawn slots now"));
            assert!(message.contains(&(spawn_index + 1).to_string()));
        }

        let roster = vivling.load_roster().expect("roster");
        assert_eq!(roster.vivling_ids.len(), expected_capacity + 1);

        let next_spawn = vivling.command(VivlingAction::Spawn, temp.path());
        if expected_capacity == 0 {
            let err = next_spawn.expect_err("spawn should be gated");
            assert!(err.contains("level 30"));
        } else {
            let err = next_spawn.expect_err("quota should block extra spawn");
            assert!(err.contains("No free local spawn slots"));
        }

        let lineage_states = vivling
            .load_lineage_states(&primary.primary_vivling_id)
            .expect("lineage");
        let spawned_count = lineage_states
            .iter()
            .filter(|entry| !entry.is_primary && !entry.is_imported)
            .count();
        assert_eq!(spawned_count, expected_capacity);
        for spawned in lineage_states
            .iter()
            .filter(|entry| !entry.is_primary && !entry.is_imported)
        {
            assert_eq!(spawned.level, 1);
            assert!(!spawned.adult_bootstrap);
            assert!(spawned.brain_profile.is_none());
        }
    }
}

#[test]
fn spawn_rejects_non_primary_vivling() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let primary = set_active_level(&mut vivling, 30);
    let spawned_id = {
        let _ = vivling
            .command(VivlingAction::Spawn, temp.path())
            .expect("spawn");
        spawn_ids(&vivling, &primary.vivling_id)
            .into_iter()
            .next()
            .expect("spawn id")
    };

    let _ = vivling
        .command(VivlingAction::Focus(spawned_id), temp.path())
        .expect("focus spawn");
    let err = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect_err("spawned vivling cannot spawn");
    assert!(err.contains("Only a primary Vivling"));
}

#[test]
fn roster_focus_and_reload_preserve_active_member_and_alias_resolution() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let primary = set_active_level(&mut vivling, 30);
    let _ = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect("spawn");

    let roster_text = match vivling
        .command(VivlingAction::Roster, temp.path())
        .expect("roster")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(roster_text.contains("Vivling roster"));
    assert!(roster_text.contains("top-level slots 1/3"));
    assert!(roster_text.contains("[primary]"));

    let _ = vivling
        .command(VivlingAction::Focus("spawn-1".to_string()), temp.path())
        .expect("focus by alias");
    let spawned_id = spawn_ids(&vivling, &primary.vivling_id)
        .into_iter()
        .next()
        .expect("spawned id");
    assert_eq!(
        vivling.active_vivling_id.as_deref(),
        Some(spawned_id.as_str())
    );
    let focused = vivling.state.as_ref().expect("focused state");
    assert_eq!(
        focused.last_work_summary.as_deref(),
        Some(format!("{} active", focused.name).as_str())
    );

    let status = match vivling
        .command(VivlingAction::Status, temp.path())
        .expect("status")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(status.contains("spawned"));
    assert!(status.contains("local spawn slots"));

    let reloaded = configured_vivling(temp.path());
    assert_eq!(
        reloaded.active_vivling_id.as_deref(),
        Some(spawned_id.as_str())
    );
    assert_eq!(
        reloaded
            .state
            .as_ref()
            .map(|state| state.lineage_role_label()),
        Some("spawned")
    );
}

#[test]
fn remove_spawned_vivling_frees_local_spawn_capacity() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let primary = set_active_level(&mut vivling, 30);
    let _ = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect("spawn");
    let spawned_id = spawn_ids(&vivling, &primary.vivling_id)
        .into_iter()
        .next()
        .expect("spawned id");

    let removed = match vivling
        .command(VivlingAction::Remove(spawned_id.clone()), temp.path())
        .expect("remove spawned")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(removed.contains("Removed"));

    let respawned = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect("spawn after removal");
    match respawned {
        VivlingCommandOutcome::SpawnNarration { message, .. } => {
            assert!(message.contains("Spawned"))
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn remove_imported_vivling_frees_external_slot() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");

    let mut exporter = hatched_vivling(source.path());
    let state = set_active_level(&mut exporter, 30);
    let export_one = source.path().join("one.vivegg");
    let export_two = source.path().join("two.vivegg");
    let _ = exporter
        .command(
            VivlingAction::Export(Some(export_one.display().to_string())),
            source.path(),
        )
        .expect("export one");

    let second_state = VivlingState {
        vivling_id: "viv-import-two".to_string(),
        name: "Import Two".to_string(),
        ..state.clone()
    };
    let second_manifest = VivlingPackageManifest {
        package_version: VIVPKG_VERSION,
        exported_at: Utc::now(),
        vivling_id: second_state.vivling_id.clone(),
        primary_vivling_id: second_state.primary_vivling_id.clone(),
        species: second_state.species.clone(),
        rarity: second_state.rarity.clone(),
        level: second_state.level,
        is_primary: true,
        is_imported: false,
        spawn_generation: 0,
    };
    make_package(&export_two, &second_manifest, &second_state);

    let mut target_vivling = hatched_vivling(target.path());
    let imported_id = {
        let _ = target_vivling
            .command(
                VivlingAction::Import(export_one.display().to_string()),
                target.path(),
            )
            .expect("import one");
        target_vivling
            .load_roster()
            .expect("roster")
            .external_vivling_ids
            .into_iter()
            .next()
            .expect("imported id")
    };

    let _ = target_vivling
        .command(VivlingAction::Remove(imported_id), target.path())
        .expect("remove imported");
    let _ = target_vivling
        .command(
            VivlingAction::Import(export_two.display().to_string()),
            target.path(),
        )
        .expect("import after free slot");
    assert_eq!(
        target_vivling
            .load_roster()
            .expect("roster")
            .external_vivling_ids
            .len(),
        1
    );
}

#[test]
fn remove_rejects_primary_with_spawned_children() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let primary = set_active_level(&mut vivling, 30);
    let _ = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect("spawn");
    let spawned_id = spawn_ids(&vivling, &primary.vivling_id)
        .into_iter()
        .next()
        .expect("spawned id");
    let _ = vivling
        .command(VivlingAction::Focus(spawned_id), temp.path())
        .expect("focus spawn");
    let err = vivling
        .command(VivlingAction::Remove(primary.vivling_id), temp.path())
        .expect_err("primary remove should fail");
    assert!(err.contains("spawned lineage children"));
}

fn spawn_a_baby(
    vivling: &mut Vivling,
    temp_path: &Path,
    setup_parent: impl FnOnce(&mut VivlingState),
) -> (VivlingState, VivlingState) {
    let mut parent = set_active_level(vivling, 30);
    setup_parent(&mut parent);
    vivling.active_vivling_id = Some(parent.vivling_id.clone());
    vivling.state = Some(parent.clone());
    vivling.save_state().expect("save parent");
    let _ = vivling
        .command(VivlingAction::Spawn, temp_path)
        .expect("spawn should work");
    let spawned_id = spawn_ids(vivling, &parent.vivling_id)
        .into_iter()
        .next()
        .expect("spawned id");
    let child = vivling
        .load_state_for_id(&spawned_id)
        .expect("load child state")
        .expect("child state exists");
    (parent, child)
}

#[test]
fn spawn_offspring_preserves_lineage_chain() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let (parent, child) = spawn_a_baby(&mut vivling, temp.path(), |_| {});
    assert_eq!(child.species, parent.species);
    assert_eq!(child.rarity, parent.rarity);
    assert_eq!(child.primary_vivling_id, parent.primary_vivling_id);
    assert_eq!(
        child.parent_vivling_id.as_deref(),
        Some(parent.vivling_id.as_str())
    );
    assert_eq!(child.spawn_generation, parent.spawn_generation + 1);
    assert!(!child.is_primary);
}

#[test]
fn spawn_offspring_inherits_two_strongest_affinities() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let (parent, child) = spawn_a_baby(&mut vivling, temp.path(), |p| {
        p.gene_vector.affinity_mod = [0.80, 1.25, 0.95, 1.18];
    });
    let close_to_parent: Vec<usize> = (0..4)
        .filter(|&i| {
            (child.gene_vector.affinity_mod[i] - parent.gene_vector.affinity_mod[i]).abs() <= 0.05
        })
        .collect();
    assert!(
        close_to_parent.contains(&1) && close_to_parent.contains(&3),
        "child must inherit top-2 indices 1 and 3 within 0.05: got {close_to_parent:?}"
    );
}

#[test]
fn spawn_offspring_resets_xp_and_memory() {
    use crate::vivling::model::VivlingDistilledSummary;
    use crate::vivling::model::VivlingMentalPath;
    use crate::vivling::model::VivlingWorkMemoryEntry;
    use crate::vivling::model::WorkArchetype;

    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let (_, child) = spawn_a_baby(&mut vivling, temp.path(), |p| {
        p.work_xp = WORK_XP_PER_LEVEL.saturating_mul(34);
        p.xp = p.work_xp;
        p.suggestions_made = 12;
        p.turns_observed = 99;
        p.loop_runtime_submissions = 5;
        p.loop_admin_churn = 3;
        p.loop_exposure = 7;
        p.work_memory.push(VivlingWorkMemoryEntry {
            kind: "submission".to_string(),
            summary: "pre-spawn entry".to_string(),
            archetype: WorkArchetype::default(),
            weight: 1,
            created_at: Utc::now(),
        });
        p.distilled_summaries.push(VivlingDistilledSummary {
            topic: "test".to_string(),
            summary: "digest".to_string(),
            kind: "digest".to_string(),
            archetype: WorkArchetype::default(),
            total_weight: 1,
            observations: 1,
            first_seen_at: Utc::now(),
            last_seen_at: Utc::now(),
        });
        p.mental_paths.push(VivlingMentalPath::default());
    });
    assert_eq!(child.level, 1);
    assert_eq!(child.xp, 0);
    assert_eq!(child.work_xp, 0);
    assert!(child.work_memory.is_empty());
    assert!(child.distilled_summaries.is_empty());
    assert!(child.mental_paths.is_empty());
    assert_eq!(child.turns_observed, 0);
    assert_eq!(child.suggestions_made, 0);
    assert_eq!(child.loop_runtime_submissions, 0);
    assert_eq!(child.loop_admin_churn, 0);
    assert_eq!(child.loop_exposure, 0);
    assert_eq!(child.active_work_days, 0);
}

#[test]
fn spawn_offspring_resets_brain_state() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let (_, child) = spawn_a_baby(&mut vivling, temp.path(), |p| {
        p.brain_enabled = true;
        p.brain_last_error = Some("network down".to_string());
        p.brain_last_used_at = Some(Utc::now());
    });
    assert!(!child.brain_enabled);
    assert!(child.brain_profile.is_none());
    assert!(child.brain_last_error.is_none());
    assert!(child.brain_last_used_at.is_none());
}

#[test]
fn spawn_offspring_starts_at_baby_stage_with_join_message() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let (_, child) = spawn_a_baby(&mut vivling, temp.path(), |_| {});
    assert_eq!(child.stage(), Stage::Baby);
    assert_eq!(child.level, 1);
    assert_eq!(child.xp, 0);
    assert_eq!(
        child.last_message.as_deref(),
        Some("joined the roster from a local spawn"),
    );
}
