use super::common::*;

#[test]
fn export_and_import_roundtrip_uses_external_slots_without_auto_focus() {
    let source = TempDir::new().expect("source tempdir");
    let target = TempDir::new().expect("target tempdir");

    let mut source_vivling = configured_vivling(source.path());
    let _ = source_vivling
        .command(VivlingAction::Hatch, source.path())
        .expect("hatch");
    let source_state = leveled_state(30, 30);
    source_vivling.active_vivling_id = Some(source_state.vivling_id.clone());
    source_vivling.state = Some(source_state.clone());
    source_vivling.save_state().expect("save source");

    let export_path = source.path().join("demo.vivegg");
    let export_message = match source_vivling
        .command(
            VivlingAction::Export(Some(export_path.display().to_string())),
            source.path(),
        )
        .expect("export")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(export_message.contains("Exported"));
    assert!(export_path.exists());
    assert_eq!(
        source_vivling
            .state
            .as_ref()
            .map(|state| state.export_count),
        Some(source_state.export_count + 1)
    );
    assert_eq!(
        source_vivling.active_vivling_id.as_deref(),
        Some(source_state.vivling_id.as_str())
    );

    let mut target_vivling = configured_vivling(target.path());
    let _ = target_vivling
        .command(VivlingAction::Hatch, target.path())
        .expect("target hatch");
    let active_before = target_vivling.active_vivling_id.clone();

    let import_message = match target_vivling
        .command(
            VivlingAction::Import(export_path.display().to_string()),
            target.path(),
        )
        .expect("import")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(import_message.contains("Imported"));
    assert_eq!(target_vivling.active_vivling_id, active_before);
    let roster = target_vivling.load_roster().expect("target roster");
    assert_eq!(roster.external_vivling_ids.len(), 1);
    let imported = target_vivling
        .load_state_for_id(&roster.external_vivling_ids[0])
        .expect("load imported")
        .expect("imported state");
    assert_eq!(imported.vivling_id, source_state.vivling_id);
    assert_eq!(imported.primary_vivling_id, source_state.primary_vivling_id);
    assert!(imported.is_imported);
    assert!(
        imported
            .import_source
            .as_deref()
            .is_some_and(|path| path.ends_with("demo.vivegg"))
    );
    assert_eq!(imported.export_count, source_state.export_count);
}

#[test]
fn import_rejects_duplicate_vivling_id_without_roster_duplication() {
    let source = TempDir::new().expect("source tempdir");
    let target = TempDir::new().expect("target tempdir");

    let mut source_vivling = hatched_vivling(source.path());
    let _ = set_active_level(&mut source_vivling, 30);
    let export_path = source.path().join("duplicate.vivegg");
    let _ = source_vivling
        .command(
            VivlingAction::Export(Some(export_path.display().to_string())),
            source.path(),
        )
        .expect("export");

    let mut target_vivling = hatched_vivling(target.path());
    let _ = target_vivling
        .command(
            VivlingAction::Import(export_path.display().to_string()),
            target.path(),
        )
        .expect("first import");
    let err = target_vivling
        .command(
            VivlingAction::Import(export_path.display().to_string()),
            target.path(),
        )
        .expect_err("duplicate import should fail");
    assert!(err.contains("already exists"), "{err}");

    let roster = target_vivling.load_roster().expect("target roster");
    assert_eq!(roster.external_vivling_ids.len(), 1);
    assert_eq!(
        roster
            .vivling_ids
            .iter()
            .filter(|id| *id == &roster.external_vivling_ids[0])
            .count(),
        1
    );
}

#[test]
fn import_preserves_spawned_lineage_metadata_as_external_entry() {
    let target = TempDir::new().expect("target");
    let package_dir = TempDir::new().expect("package");
    let package_path = package_dir.path().join("spawned-lineage.vivegg");
    let mut state = exportable_state(30);
    state.vivling_id = "viv-spawned-import".to_string();
    state.primary_vivling_id = "viv-primary-origin".to_string();
    state.parent_vivling_id = Some("viv-primary-origin".to_string());
    state.spawn_generation = 2;
    state.is_primary = false;
    state.is_imported = false;
    state.instance_label = Some("spawn-2".to_string());
    let manifest = VivlingPackageManifest {
        package_version: VIVPKG_VERSION,
        exported_at: Utc::now(),
        vivling_id: state.vivling_id.clone(),
        primary_vivling_id: state.primary_vivling_id.clone(),
        species: state.species.clone(),
        rarity: state.rarity.clone(),
        level: state.level,
        is_primary: state.is_primary,
        is_imported: false,
        spawn_generation: state.spawn_generation,
    };
    make_package(&package_path, &manifest, &state);

    let mut vivling = hatched_vivling(target.path());
    let _ = vivling
        .command(
            VivlingAction::Import(package_path.display().to_string()),
            target.path(),
        )
        .expect("import spawned package");

    let roster = vivling.load_roster().expect("roster");
    assert!(roster.external_vivling_ids.contains(&state.vivling_id));
    let imported = vivling
        .load_state_for_id(&state.vivling_id)
        .expect("load imported")
        .expect("imported state");
    assert_eq!(imported.primary_vivling_id, "viv-primary-origin");
    assert_eq!(
        imported.parent_vivling_id.as_deref(),
        Some("viv-primary-origin")
    );
    assert_eq!(imported.spawn_generation, 2);
    assert!(!imported.is_primary);
    assert!(imported.is_imported);
    assert_eq!(imported.instance_label.as_deref(), Some("spawn-2"));
    assert!(
        imported
            .import_source
            .as_deref()
            .is_some_and(|path| path.ends_with("spawned-lineage.vivegg"))
    );
}

#[test]
fn export_supports_default_and_relative_custom_paths() {
    let temp = TempDir::new().expect("tempdir");
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).expect("cwd dir");
    let mut vivling = hatched_vivling(temp.path());
    let state = set_active_level(&mut vivling, 30);

    let default_message = match vivling
        .command(VivlingAction::Export(None), &cwd)
        .expect("default export")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    let default_path = temp
        .path()
        .join("vivlings")
        .join("exports")
        .join(format!("{}.vivegg", state.vivling_id));
    assert!(default_message.contains(default_path.to_string_lossy().as_ref()));
    assert!(default_path.exists());

    let relative = match vivling
        .command(
            VivlingAction::Export(Some("nested/demo-export".to_string())),
            &cwd,
        )
        .expect("relative export")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    let relative_path = cwd.join("nested").join("demo-export.vivegg");
    assert!(relative.contains(relative_path.to_string_lossy().as_ref()));
    assert!(relative_path.exists());
}

#[test]
fn export_package_contains_manifest_and_state_and_updates_export_count() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let state = set_active_level(&mut vivling, 30);
    let export_path = temp.path().join("inspect.vivegg");

    let _ = vivling
        .command(
            VivlingAction::Export(Some(export_path.display().to_string())),
            temp.path(),
        )
        .expect("export");

    let file = File::open(&export_path).expect("open vivegg");
    let mut zip = ZipArchive::new(file).expect("zip archive");
    let manifest: VivlingPackageManifest =
        read_zip_json(&mut zip, "manifest.json").expect("manifest");
    let exported_state: VivlingState = read_zip_json(&mut zip, "state.json").expect("state");
    assert_eq!(manifest.vivling_id, state.vivling_id);
    assert_eq!(manifest.primary_vivling_id, state.primary_vivling_id);
    assert_eq!(exported_state.vivling_id, state.vivling_id);
    assert_eq!(exported_state.primary_vivling_id, state.primary_vivling_id);
    assert_eq!(
        vivling.state.as_ref().map(|entry| entry.export_count),
        Some(1)
    );
}

#[test]
fn import_rejects_non_vivegg_files() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let invalid = temp.path().join("bad.txt");
    fs::write(&invalid, "not a package").expect("write invalid");
    let err = vivling
        .command(
            VivlingAction::Import(invalid.display().to_string()),
            temp.path(),
        )
        .expect_err("should reject extension");
    assert!(err.contains(".vivegg"));
}

#[test]
fn import_rejects_malformed_zip_file() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let invalid = temp.path().join("broken.vivegg");
    fs::write(&invalid, "definitely not zip").expect("write invalid");
    let err = vivling
        .command(
            VivlingAction::Import(invalid.display().to_string()),
            temp.path(),
        )
        .expect_err("should reject malformed zip");
    assert!(!err.is_empty());
}

#[test]
fn import_rejects_missing_manifest_or_state_entries() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    let missing_manifest = temp.path().join("missing-manifest.vivegg");
    {
        let file = File::create(&missing_manifest).expect("create package");
        let mut zip = ZipWriter::new(file);
        zip.start_file("state.json", options).expect("state entry");
        zip.write_all(b"{}").expect("state body");
        zip.finish().expect("finish");
    }
    let err = vivling
        .command(
            VivlingAction::Import(missing_manifest.display().to_string()),
            temp.path(),
        )
        .expect_err("missing manifest");
    assert!(err.contains("specified file not found") || err.contains("manifest"));

    let missing_state = temp.path().join("missing-state.vivegg");
    {
        let file = File::create(&missing_state).expect("create package");
        let mut zip = ZipWriter::new(file);
        zip.start_file("manifest.json", options)
            .expect("manifest entry");
        zip.write_all(b"{}").expect("manifest body");
        zip.finish().expect("finish");
    }
    let err = vivling
        .command(
            VivlingAction::Import(missing_state.display().to_string()),
            temp.path(),
        )
        .expect_err("missing state");
    assert!(!err.is_empty());
}

#[test]
fn import_rejects_manifest_state_id_mismatch() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let path = temp.path().join("mismatch.vivegg");
    let state = exportable_state(30);
    let manifest = VivlingPackageManifest {
        package_version: VIVPKG_VERSION,
        exported_at: Utc::now(),
        vivling_id: "other-id".to_string(),
        primary_vivling_id: state.primary_vivling_id.clone(),
        species: state.species.clone(),
        rarity: state.rarity.clone(),
        level: state.level,
        is_primary: true,
        is_imported: false,
        spawn_generation: 0,
    };
    make_package(&path, &manifest, &state);

    let err = vivling
        .command(
            VivlingAction::Import(path.display().to_string()),
            temp.path(),
        )
        .expect_err("mismatch should fail");
    assert!(err.contains("manifest/state id mismatch"));
}

#[test]
fn import_rejects_duplicate_ids_and_full_external_slots() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let make_distinct_pkg = |idx: usize| {
        let path = temp.path().join(format!("import-{idx}.vivegg"));
        let mut state = exportable_state(30);
        state.vivling_id = format!("viv-import-{idx}");
        state.name = format!("Import {idx}");
        state.primary_vivling_id = String::new();
        state.origin_install_id = None;
        state.is_primary = false;
        let manifest = VivlingPackageManifest {
            package_version: VIVPKG_VERSION,
            exported_at: Utc::now(),
            vivling_id: state.vivling_id.clone(),
            primary_vivling_id: state.vivling_id.clone(),
            species: state.species.clone(),
            rarity: state.rarity.clone(),
            level: state.level,
            is_primary: true,
            is_imported: false,
            spawn_generation: 0,
        };
        make_package(&path, &manifest, &state);
        path
    };

    let duplicate_path = make_distinct_pkg(1);
    let _ = vivling
        .command(
            VivlingAction::Import(duplicate_path.display().to_string()),
            temp.path(),
        )
        .expect("first import");
    let err = vivling
        .command(
            VivlingAction::Import(duplicate_path.display().to_string()),
            temp.path(),
        )
        .expect_err("duplicate should fail");
    assert!(err.contains("already exists"));

    let path = make_distinct_pkg(2);
    let _ = vivling
        .command(
            VivlingAction::Import(path.display().to_string()),
            temp.path(),
        )
        .expect("fill top-level slot");
    let fourth_path = make_distinct_pkg(3);
    let err = vivling
        .command(
            VivlingAction::Import(fourth_path.display().to_string()),
            temp.path(),
        )
        .expect_err("next import should fail");
    assert!(err.contains("All top-level Vivling slots are full"));

    let imported_id = vivling
        .load_roster()
        .expect("roster")
        .external_vivling_ids
        .first()
        .cloned()
        .expect("imported id");
    let imported = vivling
        .load_state_for_id(&imported_id)
        .expect("load imported")
        .expect("imported state");
    assert_eq!(imported.primary_vivling_id, imported.vivling_id);
    assert!(imported.is_primary);
    assert!(imported.is_imported);
}

#[test]
fn imported_primary_can_use_local_spawn_capacity() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    let mut exporter = hatched_vivling(source.path());
    let state = set_active_level(&mut exporter, 60);
    let import_path = source.path().join("primary-import.vivegg");
    let _ = exporter
        .command(
            VivlingAction::Export(Some(import_path.display().to_string())),
            source.path(),
        )
        .expect("export source primary");

    let mut vivling = hatched_vivling(target.path());
    let _ = vivling
        .command(
            VivlingAction::Import(import_path.display().to_string()),
            target.path(),
        )
        .expect("import");
    let imported_id = vivling
        .load_roster()
        .expect("roster")
        .external_vivling_ids
        .into_iter()
        .next()
        .expect("imported id");
    let _ = vivling
        .command(VivlingAction::Focus(imported_id), target.path())
        .expect("focus imported");
    assert_eq!(
        vivling.state.as_ref().map(|entry| entry.level),
        Some(state.level)
    );

    let _ = vivling
        .command(VivlingAction::Spawn, target.path())
        .expect("spawn one");
    let _ = vivling
        .command(VivlingAction::Spawn, target.path())
        .expect("spawn two");
    let err = vivling
        .command(VivlingAction::Spawn, target.path())
        .expect_err("third spawn should fail at level 60");
    assert!(err.contains("No free local spawn slots"));
}
