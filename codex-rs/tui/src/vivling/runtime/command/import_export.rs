use super::super::*;

impl Vivling {
    pub(crate) fn export_active(
        &mut self,
        cwd: &Path,
        maybe_path: Option<&str>,
    ) -> Result<String, String> {
        self.ensure_hatched()?;
        let mut state = self.state.as_mut().expect("state checked").clone();
        if !state.export_unlocked() {
            return Err("`/vivling export` unlocks only at level 30.".to_string());
        }
        let export_path = self.resolve_export_path(cwd, maybe_path, &state.vivling_id)?;
        if let Some(parent) = export_path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let manifest = VivlingPackageManifest {
            package_version: VIVPKG_VERSION,
            exported_at: Utc::now(),
            vivling_id: state.vivling_id.clone(),
            primary_vivling_id: state.primary_vivling_id.clone(),
            species: state.species.clone(),
            rarity: state.rarity.clone(),
            level: state.level,
            is_primary: state.is_primary,
            is_imported: state.is_imported,
            spawn_generation: state.spawn_generation,
        };
        let file = fs::File::create(&export_path).map_err(|err| err.to_string())?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("manifest.json", options)
            .map_err(|err| err.to_string())?;
        zip.write_all(
            serde_json::to_string_pretty(&manifest)
                .map_err(|err| err.to_string())?
                .as_bytes(),
        )
        .map_err(|err| err.to_string())?;
        zip.start_file("state.json", options)
            .map_err(|err| err.to_string())?;
        zip.write_all(
            serde_json::to_string_pretty(&state)
                .map_err(|err| err.to_string())?
                .as_bytes(),
        )
        .map_err(|err| err.to_string())?;
        zip.finish().map_err(|err| err.to_string())?;

        state.export_count = state.export_count.saturating_add(1);
        state.last_message = Some("was packaged for export".to_string());
        self.state = Some(state);
        self.save_state().map_err(|err| err.to_string())?;
        Ok(format!(
            "Exported Vivling package to {}.",
            export_path.display()
        ))
    }

    pub(crate) fn import_package(&mut self, cwd: &Path, raw_path: &str) -> Result<String, String> {
        let import_path = resolve_input_path(cwd, raw_path);
        if import_path.extension().and_then(|ext| ext.to_str()) != Some(VIVEGG_EXT) {
            return Err("Vivling import expects a `.vivegg` file.".to_string());
        }
        let roster = self.load_roster().map_err(|err| err.to_string())?;
        if self.top_level_slot_usage().map_err(|err| err.to_string())? >= EXTERNAL_SLOT_LIMIT {
            return Err(format!(
                "All top-level Vivling slots are full ({EXTERNAL_SLOT_LIMIT}/{EXTERNAL_SLOT_LIMIT})."
            ));
        }
        let file = fs::File::open(&import_path).map_err(|err| {
            format!(
                "Failed to open Vivling package {}: {err}. Use /vivling import <path.vivegg>.",
                import_path.display()
            )
        })?;
        let mut archive = ZipArchive::new(file).map_err(|err| {
            format!(
                "Invalid Vivling package {}: {err}. Use a .vivegg file exported with /vivling export.",
                import_path.display()
            )
        })?;
        let manifest: VivlingPackageManifest =
            read_zip_json(&mut archive, "manifest.json").map_err(|err| err.to_string())?;
        let mut state: VivlingState =
            read_zip_json(&mut archive, "state.json").map_err(|err| err.to_string())?;
        if manifest.vivling_id != state.vivling_id {
            return Err("Vivling package manifest/state id mismatch.".to_string());
        }
        if roster
            .vivling_ids
            .iter()
            .any(|entry| entry == &state.vivling_id)
        {
            return Err(format!(
                "Vivling `{}` already exists in the local roster.",
                state.vivling_id
            ));
        }
        state.is_imported = true;
        state.imported_at = Some(Utc::now());
        state.import_source = Some(import_path.display().to_string());
        state.last_message = Some("was imported into the local roster".to_string());
        state.normalize_loaded_state();
        self.save_state_record(&state, false, true)
            .map_err(|err| err.to_string())?;
        Ok(format!(
            "Imported {} [{}] {}. Top-level slots now {}/{}.",
            state.vivling_id,
            state.lineage_role_label(),
            state.name,
            self.top_level_slot_usage().map_err(|err| err.to_string())?,
            EXTERNAL_SLOT_LIMIT
        ))
    }
}
