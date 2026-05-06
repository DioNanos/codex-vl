use super::common::*;

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
