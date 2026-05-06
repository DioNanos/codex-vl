use super::common::*;
use crate::vivling::model::VERSION as CURRENT_STATE_VERSION;
use crate::vivling::model::VivlingDistilledSummary;
use crate::vivling::model::VivlingIdentityProfile;
use crate::vivling::model::VivlingLoopProfile;
use crate::vivling::model::VivlingMentalPath;
use crate::vivling::model::VivlingWorkMemoryEntry;
use crate::vivling::model::WorkArchetype;

fn captured_timestamp() -> chrono::DateTime<Utc> {
    "2026-05-01T08:18:18Z"
        .parse()
        .expect("captured fixture timestamp")
}

fn captured_memory_entries(count: usize) -> Vec<VivlingWorkMemoryEntry> {
    (0..count)
        .map(|index| VivlingWorkMemoryEntry {
            kind: "turn".to_string(),
            summary: format!("sanitized captured turn {}", index + 1),
            archetype: WorkArchetype::Builder,
            weight: 12 + index as u64,
            created_at: captured_timestamp(),
        })
        .collect()
}

fn captured_distilled_summaries(count: usize) -> Vec<VivlingDistilledSummary> {
    (0..count)
        .map(|index| VivlingDistilledSummary {
            topic: "work_pattern".to_string(),
            summary: format!("sanitized captured pattern {}", index + 1),
            kind: "turn".to_string(),
            archetype: WorkArchetype::Builder,
            total_weight: 1000 + index as u64,
            observations: 100 + index as u64,
            first_seen_at: captured_timestamp(),
            last_seen_at: captured_timestamp(),
        })
        .collect()
}

fn captured_mental_paths(count: usize) -> Vec<VivlingMentalPath> {
    (0..count)
        .map(|index| VivlingMentalPath {
            from: "topic:work_pattern".to_string(),
            to: "focus:builder".to_string(),
            weight: 500 + index as u64,
            last_seen_at: captured_timestamp(),
        })
        .collect()
}

fn captured_state(id: &str, level: u64, active_days: u64) -> VivlingState {
    let mut state = leveled_state(level, active_days);
    state.version = 6;
    state.hatched = true;
    state.visible = true;
    state.seed_hash = format!("sanitized-{id}");
    state.vivling_id = id.to_string();
    state.install_id = None;
    state.origin_install_id = None;
    state.species = "syllo".to_string();
    state.rarity = "Common".to_string();
    state.name = "Captured".to_string();
    state.primary_vivling_id = id.to_string();
    state.parent_vivling_id = None;
    state.spawn_generation = 0;
    state.is_primary = true;
    state.is_imported = false;
    state.imported_at = None;
    state.import_source = None;
    state.export_count = 0;
    state.instance_label = None;
    state.created_at = Some(captured_timestamp());
    state.last_seen_at = Some(captured_timestamp());
    state.last_fed_at = None;
    state.brain_enabled = false;
    state.brain_profile = None;
    state.brain_last_error = None;
    state.brain_last_used_at = None;
    state.seed_origin = None;
    state.adult_bootstrap = level >= ADULT_LEVEL;
    state.last_work_summary = Some("sanitized captured work summary".to_string());
    state.last_live_context_summary = Some("sanitized captured live context".to_string());
    state.work_memory = captured_memory_entries(64);
    state.distilled_summaries =
        captured_distilled_summaries(if level >= ADULT_LEVEL { 17 } else { 8 });
    state.mental_paths = captured_mental_paths(if level >= ADULT_LEVEL { 16 } else { 13 });
    state.identity_profile = VivlingIdentityProfile {
        tone: "skeptical".to_string(),
        dominant_focus: WorkArchetype::Builder,
        question_bias: if level >= ADULT_LEVEL { 60 } else { 25 },
        caution_bias: if level >= ADULT_LEVEL { 10 } else { 29 },
        verification_bias: if level >= ADULT_LEVEL { 243 } else { 135 },
    };
    state.loop_profile = VivlingLoopProfile {
        clean_submissions: if level >= ADULT_LEVEL { 36 } else { 0 },
        noisy_churn: if level >= ADULT_LEVEL { 433 } else { 220 },
        blocked_runs: if level >= ADULT_LEVEL { 12 } else { 0 },
        milestone_signals: if level >= ADULT_LEVEL { 30 } else { 0 },
        partial_signals: if level >= ADULT_LEVEL { 46 } else { 0 },
        verification_signals: if level >= ADULT_LEVEL { 239 } else { 135 },
        wait_signals: if level >= ADULT_LEVEL { 675 } else { 350 },
    };
    state.last_message = Some("sanitized captured lifecycle message".to_string());
    state.unlocked_species = vec!["syllo".to_string()];
    state
}

fn write_captured_roster(home: &Path, roster: serde_json::Value, states: &[VivlingState]) {
    let roster_dir = home.join(ROSTER_DIR);
    fs::create_dir_all(&roster_dir).expect("captured roster dir");
    fs::write(
        roster_dir.join(ROSTER_FILE),
        serde_json::to_string_pretty(&roster).expect("captured roster json"),
    )
    .expect("write captured roster");
    for state in states {
        fs::write(
            roster_dir.join(format!("{}.json", state.vivling_id)),
            serde_json::to_string_pretty(state).expect("captured state json"),
        )
        .expect("write captured state");
    }
}

fn assert_captured_adult_state(state: &VivlingState, id: &str) {
    assert_eq!(state.vivling_id, id);
    assert_eq!(state.version, CURRENT_STATE_VERSION);
    assert_eq!(state.species, "syllo");
    assert_eq!(state.rarity, "Common");
    assert_eq!(state.primary_vivling_id, id);
    assert!(state.is_primary);
    assert!(!state.is_imported);
    assert_eq!(state.level, 61);
    assert_eq!(state.work_xp, 3600);
    assert_eq!(state.active_work_days, 91);
    assert!(!state.brain_enabled);
    assert!(state.brain_profile.is_none());
    assert_eq!(state.work_memory.len(), 64);
    assert_eq!(state.distilled_summaries.len(), 17);
    assert_eq!(state.mental_paths.len(), 16);
    assert_eq!(
        state.identity_profile.dominant_focus,
        WorkArchetype::Builder
    );
    assert_eq!(state.loop_profile.noisy_churn, 433);
    assert!(
        state
            .unlocked_species
            .iter()
            .any(|species| species == "syllo")
    );
}

#[test]
fn captured_current_one_member_roster_loads_active_state() {
    let temp = TempDir::new().expect("tempdir");
    let state = captured_state("viv-captured-current", 4, 3);
    write_captured_roster(
        temp.path(),
        serde_json::json!({
            "version": 6,
            "active_vivling_id": "viv-captured-current",
            "vivling_ids": ["viv-captured-current"],
            "external_vivling_ids": []
        }),
        std::slice::from_ref(&state),
    );

    let reloaded = configured_vivling(temp.path());

    assert_eq!(
        reloaded.active_vivling_id.as_deref(),
        Some("viv-captured-current")
    );
    let active = reloaded.state.expect("active captured state");
    assert_eq!(active.version, CURRENT_STATE_VERSION);
    assert!(!active.gene_vector.is_neutral());
    assert_eq!(active.vivling_id, "viv-captured-current");
    assert_eq!(active.primary_vivling_id, "viv-captured-current");
    assert!(active.is_primary);
    assert_eq!(active.level, 4);
    assert_eq!(active.work_xp, 180);
    assert_eq!(active.active_work_days, 3);
    assert_eq!(active.work_memory.len(), 64);
    assert_eq!(active.distilled_summaries.len(), 8);
    assert_eq!(active.mental_paths.len(), 13);
    assert!(!active.brain_enabled);
    assert_eq!(
        active.last_message.as_deref(),
        Some("sanitized captured lifecycle message")
    );
}

#[test]
fn captured_primary_memory_rich_state_preserves_progress_and_memory() {
    let temp = TempDir::new().expect("tempdir");
    let mut state = captured_state("viv-captured-primary", 61, 91);
    state.seed_origin = Some("adult_seed_v1".to_string());
    state.export_count = 1;
    write_captured_roster(
        temp.path(),
        serde_json::json!({
            "version": 6,
            "active_vivling_id": "viv-captured-primary",
            "vivling_ids": ["viv-captured-primary"],
            "external_vivling_ids": []
        }),
        std::slice::from_ref(&state),
    );

    let reloaded = configured_vivling(temp.path());
    let active = reloaded.state.expect("active captured state");

    assert_captured_adult_state(&active, "viv-captured-primary");
    assert_eq!(active.export_count, 1);
    assert_eq!(active.seed_origin.as_deref(), Some("adult_seed_v1"));
    assert!(active.adult_bootstrap);
}

#[test]
fn captured_two_member_roster_preserves_spawned_lineage() {
    let temp = TempDir::new().expect("tempdir");
    let primary = captured_state("viv-captured-primary", 61, 91);
    let mut spawned = captured_state("viv-captured-spawned", 61, 91);
    spawned.name = "Captured Spawn".to_string();
    spawned.primary_vivling_id = primary.vivling_id.clone();
    spawned.parent_vivling_id = Some(primary.vivling_id.clone());
    spawned.spawn_generation = 1;
    spawned.is_primary = false;
    spawned.instance_label = Some("spawn-1".to_string());
    spawned.last_message = Some("sanitized captured spawn message".to_string());
    write_captured_roster(
        temp.path(),
        serde_json::json!({
            "version": 6,
            "active_vivling_id": "viv-captured-primary",
            "vivling_ids": ["viv-captured-primary", "viv-captured-spawned"],
            "external_vivling_ids": []
        }),
        &[primary.clone(), spawned.clone()],
    );

    let reloaded = configured_vivling(temp.path());
    assert_eq!(
        reloaded.active_vivling_id.as_deref(),
        Some("viv-captured-primary")
    );
    let roster = reloaded.load_roster().expect("captured roster");
    assert_eq!(
        roster.vivling_ids,
        vec![
            "viv-captured-primary".to_string(),
            "viv-captured-spawned".to_string()
        ]
    );
    let loaded_spawned = reloaded
        .load_state_for_id("viv-captured-spawned")
        .expect("load spawned")
        .expect("spawned state");
    assert_eq!(loaded_spawned.primary_vivling_id, "viv-captured-primary");
    assert_eq!(
        loaded_spawned.parent_vivling_id.as_deref(),
        Some("viv-captured-primary")
    );
    assert_eq!(loaded_spawned.spawn_generation, 1);
    assert!(!loaded_spawned.is_primary);
    assert!(!loaded_spawned.is_imported);
    assert_eq!(loaded_spawned.instance_label.as_deref(), Some("spawn-1"));
    assert_eq!(loaded_spawned.work_memory.len(), 64);
    assert_eq!(loaded_spawned.distilled_summaries.len(), 17);
    assert_eq!(loaded_spawned.mental_paths.len(), 16);
}

#[test]
fn captured_vivegg_package_imports_without_losing_metadata() {
    let package_dir = TempDir::new().expect("package dir");
    let target = TempDir::new().expect("target dir");
    let package_path = package_dir.path().join("captured-primary.vivegg");
    let state = captured_state("viv-captured-package", 61, 91);
    let manifest = VivlingPackageManifest {
        package_version: VIVPKG_VERSION,
        exported_at: captured_timestamp(),
        vivling_id: state.vivling_id.clone(),
        primary_vivling_id: state.primary_vivling_id.clone(),
        species: state.species.clone(),
        rarity: state.rarity.clone(),
        level: state.level,
        is_primary: state.is_primary,
        is_imported: state.is_imported,
        spawn_generation: state.spawn_generation,
    };
    make_package(&package_path, &manifest, &state);
    let mut target_vivling = hatched_vivling(target.path());
    let active_before = target_vivling.active_vivling_id.clone();

    let import_message = match target_vivling
        .command(
            VivlingAction::Import(package_path.display().to_string()),
            target.path(),
        )
        .expect("import captured package")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(import_message.contains("Imported"));
    assert_eq!(target_vivling.active_vivling_id, active_before);
    let imported = target_vivling
        .load_state_for_id("viv-captured-package")
        .expect("load imported")
        .expect("imported captured state");
    assert_eq!(imported.vivling_id, "viv-captured-package");
    assert_eq!(imported.primary_vivling_id, "viv-captured-package");
    assert!(imported.is_imported);
    assert!(imported.imported_at.is_some());
    assert!(
        imported
            .import_source
            .as_deref()
            .is_some_and(|path| path.ends_with("captured-primary.vivegg"))
    );
    assert_eq!(imported.level, 61);
    assert_eq!(imported.work_xp, 3600);
    assert_eq!(imported.work_memory.len(), 64);
    assert_eq!(imported.distilled_summaries.len(), 17);
    assert_eq!(imported.mental_paths.len(), 16);
    assert!(!imported.brain_enabled);
}

#[test]
fn legacy_single_state_migrates_into_roster_on_configure() {
    let temp = TempDir::new().expect("tempdir");
    let legacy_path = temp.path().join("vivling.json");
    let mut legacy_state = exportable_state(30);
    legacy_state.primary_vivling_id = String::new();
    legacy_state.origin_install_id = None;
    legacy_state.is_primary = false;
    fs::write(
        &legacy_path,
        serde_json::to_string_pretty(&legacy_state).expect("legacy json"),
    )
    .expect("write legacy state");

    let vivling = configured_vivling(temp.path());
    assert!(!legacy_path.exists());
    assert_eq!(
        vivling.active_vivling_id.as_deref(),
        Some(legacy_state.vivling_id.as_str())
    );
    let roster = vivling.load_roster().expect("roster");
    assert_eq!(roster.vivling_ids, vec![legacy_state.vivling_id.clone()]);
    let migrated = vivling
        .load_state_for_id(&legacy_state.vivling_id)
        .expect("load migrated")
        .expect("migrated state");
    assert_eq!(migrated.primary_vivling_id, migrated.vivling_id);
    assert!(migrated.is_primary);
}

#[test]
fn legacy_single_state_with_suggest_ai_mode_migrates_into_roster() {
    let temp = TempDir::new().expect("tempdir");
    let legacy_path = temp.path().join("vivling.json");
    let mut legacy_state = exportable_state(30);
    legacy_state.primary_vivling_id = String::new();
    legacy_state.origin_install_id = None;
    legacy_state.is_primary = false;
    let mut raw = serde_json::to_value(&legacy_state).expect("serialize legacy state for rewrite");
    raw["ai_mode"] = serde_json::Value::String("suggest".to_string());
    fs::write(
        &legacy_path,
        serde_json::to_string_pretty(&raw).expect("legacy json"),
    )
    .expect("write legacy state");

    let vivling = configured_vivling(temp.path());
    assert!(!legacy_path.exists());
    let roster = vivling.load_roster().expect("roster");
    assert_eq!(roster.vivling_ids, vec![legacy_state.vivling_id.clone()]);
    let migrated = vivling
        .load_state_for_id(&legacy_state.vivling_id)
        .expect("load migrated")
        .expect("migrated state");
    assert_eq!(migrated.ai_mode, VivlingAiMode::On);
    assert_eq!(migrated.primary_vivling_id, migrated.vivling_id);
    assert!(migrated.is_primary);
}

#[test]
fn legacy_juvenile_state_with_missing_modern_fields_keeps_stage_and_disables_brain() {
    let temp = TempDir::new().expect("tempdir");
    let mut legacy_state = exportable_state(JUVENILE_LEVEL);
    legacy_state.primary_vivling_id = String::new();
    legacy_state.origin_install_id = None;
    legacy_state.is_primary = false;
    legacy_state.brain_enabled = true;
    legacy_state.brain_profile = Some("old-profile".to_string());
    legacy_state.brain_last_error = Some(String::new());
    legacy_state.last_message = None;
    let expected_work_xp = legacy_state.work_xp;
    let expected_days = legacy_state.active_work_days;
    write_legacy_state(temp.path(), &legacy_state, |raw| {
        for key in [
            "origin_install_id",
            "primary_vivling_id",
            "parent_vivling_id",
            "spawn_generation",
            "is_primary",
            "is_imported",
            "imported_at",
            "import_source",
            "export_count",
            "brain_last_used_at",
            "seed_origin",
            "adult_bootstrap",
            "last_message",
            "unlocked_species",
        ] {
            raw.as_object_mut().expect("object").remove(key);
        }
    });

    let vivling = configured_vivling(temp.path());
    let migrated = vivling
        .load_state_for_id(&legacy_state.vivling_id)
        .expect("load migrated")
        .expect("migrated state");
    assert_eq!(migrated.stage(), Stage::Juvenile);
    assert_eq!(migrated.work_xp, expected_work_xp);
    assert_eq!(migrated.active_work_days, expected_days);
    assert_eq!(migrated.primary_vivling_id, migrated.vivling_id);
    assert!(migrated.is_primary);
    assert!(!migrated.brain_enabled);
    assert_eq!(migrated.brain_profile.as_deref(), Some("old-profile"));
    assert!(migrated.brain_last_error.is_none());
    assert_eq!(
        migrated.last_message.as_deref(),
        Some("is watching the session")
    );
    assert!(migrated.unlocked_species.iter().any(|id| id == "syllo"));
}

#[test]
fn legacy_adult_state_with_missing_modern_fields_preserves_brain_and_unlocks() {
    let temp = TempDir::new().expect("tempdir");
    let mut legacy_state = exportable_state(ADULT_LEVEL);
    legacy_state.primary_vivling_id = String::new();
    legacy_state.origin_install_id = None;
    legacy_state.is_primary = false;
    legacy_state.brain_enabled = true;
    legacy_state.brain_profile = Some("adult-profile".to_string());
    legacy_state.brain_last_error = Some(String::new());
    legacy_state.adult_bootstrap = true;
    legacy_state.seed_origin = Some(String::new());
    legacy_state.unlocked_species = vec!["orchestra".to_string(), "syllo".to_string()];
    let expected_work_xp = legacy_state.work_xp;
    let expected_days = legacy_state.active_work_days;
    write_legacy_state(temp.path(), &legacy_state, |raw| {
        for key in [
            "origin_install_id",
            "primary_vivling_id",
            "parent_vivling_id",
            "spawn_generation",
            "is_primary",
            "is_imported",
            "imported_at",
            "import_source",
            "export_count",
            "brain_last_used_at",
            "last_live_context_summary",
            "identity_profile",
            "loop_profile",
        ] {
            raw.as_object_mut().expect("object").remove(key);
        }
    });

    let vivling = configured_vivling(temp.path());
    let migrated = vivling
        .load_state_for_id(&legacy_state.vivling_id)
        .expect("load migrated")
        .expect("migrated state");
    assert_eq!(migrated.stage(), Stage::Adult);
    assert_eq!(migrated.work_xp, expected_work_xp);
    assert_eq!(migrated.active_work_days, expected_days);
    assert_eq!(migrated.primary_vivling_id, migrated.vivling_id);
    assert!(migrated.is_primary);
    assert!(migrated.brain_enabled);
    assert_eq!(migrated.brain_profile.as_deref(), Some("adult-profile"));
    assert!(migrated.brain_last_error.is_none());
    assert!(migrated.adult_bootstrap);
    assert!(migrated.seed_origin.is_none());
    assert!(migrated.unlocked_species.iter().any(|id| id == "syllo"));
    assert!(migrated.unlocked_species.iter().any(|id| id == "orchestra"));
}

#[test]
fn legacy_single_state_with_sparse_memory_entries_migrates_into_roster() {
    let temp = TempDir::new().expect("tempdir");
    let legacy_path = temp.path().join("vivling.json");
    let mut legacy_state = exportable_state(30);
    legacy_state.primary_vivling_id = String::new();
    legacy_state.origin_install_id = None;
    legacy_state.is_primary = false;
    let mut raw = serde_json::to_value(&legacy_state).expect("serialize legacy state");
    raw["work_memory"] = serde_json::json!([
        {
            "kind": "turn",
            "summary": "reviewed docs smoke"
        },
        {
            "summary": ""
        }
    ]);
    raw["distilled_summaries"] = serde_json::json!([
        {
            "topic": "verify"
        }
    ]);
    raw["mental_paths"] = serde_json::json!([
        {
            "from": "kind:turn"
        }
    ]);
    fs::write(
        &legacy_path,
        serde_json::to_string_pretty(&raw).expect("legacy json"),
    )
    .expect("write legacy state");

    let vivling = configured_vivling(temp.path());
    assert!(!legacy_path.exists());
    let roster = vivling.load_roster().expect("roster");
    assert_eq!(roster.vivling_ids, vec![legacy_state.vivling_id.clone()]);
    let migrated = vivling
        .load_state_for_id(&legacy_state.vivling_id)
        .expect("load migrated")
        .expect("migrated state");
    assert_eq!(migrated.primary_vivling_id, migrated.vivling_id);
    assert!(migrated.is_primary);
    assert_eq!(migrated.work_memory.len(), 2);
    assert!(
        migrated
            .work_memory
            .iter()
            .all(|entry| !entry.summary.trim().is_empty())
    );
    assert!(migrated.work_xp > 0);
    assert!(migrated.level >= 1);
}

#[test]
fn roster_with_duplicate_and_missing_entries_heals_active_to_existing_state() {
    let temp = TempDir::new().expect("tempdir");
    let vivling = configured_vivling(temp.path());
    let mut first = exportable_state(30);
    first.vivling_id = "viv-existing-one".to_string();
    first.primary_vivling_id = first.vivling_id.clone();
    vivling
        .save_state_record(&first, true, false)
        .expect("save first");
    let mut second = exportable_state(30);
    second.vivling_id = "viv-existing-two".to_string();
    second.primary_vivling_id = second.vivling_id.clone();
    second.is_imported = true;
    vivling
        .save_state_record(&second, false, true)
        .expect("save second");
    let roster_path = vivling.roster_path().expect("roster path");
    fs::write(
        &roster_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "version": first.version,
            "active_vivling_id": "viv-missing",
            "vivling_ids": [
                "viv-missing",
                "viv-existing-one",
                "viv-existing-one",
                "viv-existing-two"
            ],
            "external_vivling_ids": [
                "viv-existing-two",
                "viv-missing",
                "viv-existing-two"
            ]
        }))
        .expect("roster json"),
    )
    .expect("write dirty roster");

    let reloaded = configured_vivling(temp.path());

    assert_eq!(
        reloaded.active_vivling_id.as_deref(),
        Some("viv-existing-one")
    );
    let healed = reloaded.load_roster().expect("healed roster");
    assert_eq!(
        healed.vivling_ids,
        vec![
            "viv-existing-one".to_string(),
            "viv-existing-two".to_string()
        ]
    );
    assert_eq!(
        healed.external_vivling_ids,
        vec!["viv-existing-two".to_string()]
    );
    assert_eq!(
        healed.active_vivling_id.as_deref(),
        Some("viv-existing-one")
    );
}

#[test]
fn roster_with_only_missing_entries_loads_empty_without_deleting_directory() {
    let temp = TempDir::new().expect("tempdir");
    let vivling = configured_vivling(temp.path());
    let roster_dir = vivling.roster_dir().expect("roster dir");
    fs::create_dir_all(&roster_dir).expect("roster dir");
    let roster_path = vivling.roster_path().expect("roster path");
    fs::write(
        &roster_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "version": 6,
            "active_vivling_id": "viv-missing",
            "vivling_ids": ["viv-missing", "viv-missing"],
            "external_vivling_ids": ["viv-missing"]
        }))
        .expect("roster json"),
    )
    .expect("write dirty roster");

    let reloaded = configured_vivling(temp.path());

    assert!(reloaded.state.is_none());
    assert!(roster_dir.exists());
    let healed = reloaded.load_roster().expect("healed roster");
    assert!(healed.vivling_ids.is_empty());
    assert!(healed.external_vivling_ids.is_empty());
    assert!(healed.active_vivling_id.is_none());
}
