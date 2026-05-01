use super::*;

impl Vivling {
    pub(crate) fn unavailable() -> Self {
        Self {
            codex_home: None,
            auth_mode: AuthCredentialsStoreMode::default(),
            state: None,
            active_vivling_id: None,
            frame_requester: None,
            animations_enabled: false,
            task_running: Cell::new(false),
            active_until: Cell::new(None),
            active_started_at: Cell::new(None),
            next_scheduled_frame_at: RefCell::new(None),
            animation_text: RefCell::new(None),
            activity: RefCell::new(None),
            live_context: RefCell::new(None),
        }
    }

    pub(crate) fn configure_runtime(
        &mut self,
        frame_requester: FrameRequester,
        animations_enabled: bool,
    ) {
        self.frame_requester = Some(frame_requester);
        self.animations_enabled = animations_enabled;
    }

    pub(crate) fn configure(&mut self, codex_home: &Path, auth_mode: AuthCredentialsStoreMode) {
        let codex_home = codex_home.to_path_buf();
        let needs_reload = self.codex_home.as_ref() != Some(&codex_home);
        self.codex_home = Some(codex_home);
        self.auth_mode = auth_mode;
        if needs_reload {
            let migrated = self.migrate_legacy_state_if_needed().ok().flatten();
            self.state = if migrated.is_some() {
                migrated
            } else {
                self.load_state().ok().flatten()
            };
            self.active_vivling_id = self.state.as_ref().map(|state| state.vivling_id.clone());
        }
    }

    pub(crate) fn should_render(&self) -> bool {
        self.visible_state().is_some()
    }

    pub(crate) fn set_task_running(&self, running: bool) {
        self.task_running.set(running);
        if running {
            self.mark_recent_activity(ACTIVE_FOOTER_TAIL);
        } else {
            self.request_frame();
        }
    }

    pub(crate) fn set_live_context(&self, context: Option<VivlingLiveContext>) {
        if *self.live_context.borrow() == context {
            return;
        }
        *self.live_context.borrow_mut() = context;
        self.request_frame();
    }

    pub(crate) fn command(
        &mut self,
        action: VivlingAction,
        cwd: &Path,
    ) -> Result<VivlingCommandOutcome, String> {
        match action {
            VivlingAction::Hatch => self.hatch().map(VivlingCommandOutcome::Message),
            VivlingAction::Dashboard => {
                self.dashboard_message().map(VivlingCommandOutcome::Message)
            }
            VivlingAction::Help => Ok(VivlingCommandOutcome::Message(self.help_message())),
            VivlingAction::Status => self.status().map(VivlingCommandOutcome::Message),
            VivlingAction::Roster => self.roster_summary().map(VivlingCommandOutcome::Message),
            VivlingAction::Focus(target) => self.focus(&target).map(VivlingCommandOutcome::Message),
            VivlingAction::Spawn => self.spawn_vivling().map(VivlingCommandOutcome::Message),
            VivlingAction::Export(path) => self
                .export_active(cwd, path.as_deref())
                .map(VivlingCommandOutcome::Message),
            VivlingAction::Import(path) => self
                .import_package(cwd, &path)
                .map(VivlingCommandOutcome::Message),
            VivlingAction::Remove(target) => self
                .remove_vivling(&target)
                .map(VivlingCommandOutcome::Message),
            VivlingAction::Memory => self
                .update_existing(|state| state.memory_digest())
                .map(VivlingCommandOutcome::Message),
            VivlingAction::Card => self
                .update_existing_value(render_vivling_card)
                .map(VivlingCommandOutcome::OpenCard),
            VivlingAction::Upgrade => self
                .update_existing_value(render_upgrade_card)
                .map(VivlingCommandOutcome::OpenUpgrade),
            VivlingAction::Assist(task) => self
                .prepare_assist_request(&task)
                .map(VivlingCommandOutcome::DispatchAssist),
            VivlingAction::Chat(text) => self.chat(&text),
            VivlingAction::Brain(enabled) => self
                .set_brain_enabled_with_guidance(enabled)
                .map(VivlingCommandOutcome::Message),
            VivlingAction::ModelShow => self.model_summary().map(VivlingCommandOutcome::Message),
            VivlingAction::ModelList => self.model_list().map(VivlingCommandOutcome::Message),
            VivlingAction::ModelProfile(profile) => self
                .prepare_existing_profile_request(profile)
                .map(VivlingCommandOutcome::PersistBrainProfile),
            VivlingAction::ModelCustom {
                model,
                provider,
                effort,
            } => self
                .prepare_custom_profile_request(model, provider, effort)
                .map(VivlingCommandOutcome::PersistBrainProfile),
            VivlingAction::Recap => self
                .update_existing(|state| state.memory_recap())
                .map(VivlingCommandOutcome::Message),
            VivlingAction::PromoteEarly => self
                .update_existing(|state| state.promote_to_level_10_seed())
                .map(VivlingCommandOutcome::Message),
            VivlingAction::PromoteAdult => self
                .update_existing(|state| state.promote_to_adult_seed())
                .map(VivlingCommandOutcome::Message),
            VivlingAction::Mode(mode) => self
                .update_existing_result(|state| state.set_ai_mode(mode))
                .map(VivlingCommandOutcome::Message),
            VivlingAction::DirectMessage(text) => self
                .update_existing_result(|state| state.direct_chat_reply(&text))
                .map(VivlingCommandOutcome::Message),
            VivlingAction::Reset => {
                self.state = None;
                let removed_id = self.active_vivling_id.take();
                if let Some(path) = self.active_state_path() {
                    let _ = fs::remove_file(path);
                }
                if let Some(id) = removed_id {
                    let _ = self.remove_from_roster(&id);
                }
                Ok(VivlingCommandOutcome::Message(
                    "Vivling reset. Use /vivling hatch when you want a new one.".to_string(),
                ))
            }
        }
    }

    pub(crate) fn chat(&mut self, text: &str) -> Result<VivlingCommandOutcome, String> {
        self.ensure_hatched()?;
        let should_use_brain = {
            let state = self.state.as_ref().expect("state checked");
            state.stage() == Stage::Adult && state.brain_enabled && state.brain_profile.is_some()
        };
        if should_use_brain {
            self.prepare_chat_request(text)
                .map(VivlingCommandOutcome::DispatchAssist)
        } else {
            self.update_existing_result(|state| state.direct_chat_reply(text))
                .map(VivlingCommandOutcome::Message)
        }
    }

    pub(crate) fn set_brain_enabled_with_guidance(
        &mut self,
        enabled: bool,
    ) -> Result<String, String> {
        if !enabled {
            return self.update_existing_result(|state| state.set_brain_enabled(false));
        }
        self.ensure_hatched()?;
        {
            let state = self.state.as_ref().expect("state checked");
            if state.stage() != Stage::Adult {
                return Err("Vivling brain unlocks only at level 60.".to_string());
            }
            if state.brain_profile.is_some() {
                return self.update_existing_result(|state| state.set_brain_enabled(true));
            }
        }

        let profiles = self.model_list()?;
        let mut lines =
            vec!["Select a Vivling brain profile before enabling the brain.".to_string()];
        if profiles.contains("Vivling brain profiles:") {
            lines.push("Use `/vivling model <profile>` with one of these profiles:".to_string());
        } else {
            lines.push("Create one with `/vivling model <model> [provider] [effort]`.".to_string());
        }
        lines.push(profiles);
        Ok(lines.join("\n"))
    }

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

    pub(crate) fn help_message(&self) -> String {
        let mut lines = vec![
            "Vivling commands:".to_string(),
            "Ctrl+J - open or close the Vivling chat panel".to_string(),
            "/vivling hatch - hatch a new top-level Vivling while slots are free".to_string(),
            "/vivling status - show active Vivling status and slot usage".to_string(),
            "/vivling roster - list known Vivlings".to_string(),
            "/vivling list - alias for roster".to_string(),
            "/vivling focus <vivling_id_or_name> - switch active Vivling".to_string(),
            "/vivling switch <vivling_id_or_name> - alias for focus".to_string(),
            "/vivling card - open the current Vivling card".to_string(),
            "/vivling upgrade - open the ZED upgrade card".to_string(),
            "/vivling assist <task> - ask the Vivling brain for adult help".to_string(),
            "/vivling brain <on|off> - enable or disable the Vivling brain".to_string(),
            "/vivling model - show the current Vivling brain profile".to_string(),
            "/vivling model list - show assignable Vivling brain models".to_string(),
            "/vivling model <profile> - assign an existing config profile".to_string(),
            "/vivling model <model> [provider] [effort] - create or update the per-Vivling profile"
                .to_string(),
            "/vivling recap - summarize learned memory and current focus".to_string(),
            "/vivling promote 10 - apply the early growth baseline".to_string(),
            "/vivling promote 60 - apply the adult seed baseline".to_string(),
            "/vivling mode <on|off> - toggle active help once adult".to_string(),
            "/vivling spawn - create a local lineage copy once unlocked".to_string(),
            "/vivling export [path.vivegg] - export the active Vivling from level 30".to_string(),
            "/vivling import <path.vivegg> - import a packaged Vivling".to_string(),
            "/vivling remove <vivling_id_or_name> - remove a non-active Vivling".to_string(),
            "/vivling reset - remove the current Vivling state".to_string(),
            "/vivling <message> - talk directly to the active Vivling".to_string(),
            "/vl <message> - short alias for direct Vivling chat".to_string(),
        ];

        if let Some(state) = self.state.as_ref().filter(|state| state.hatched) {
            lines.push(String::new());
            lines.push(format!(
                "Current: {} {} Lv {} [{}]",
                state.name,
                species_for_id(&state.species).name,
                state.level,
                state.lineage_role_label()
            ));
            lines.push(state.brain_summary());
            let loop_owner_ready = if state.stage() == Stage::Adult
                && state.brain_enabled
                && state.brain_profile.is_some()
            {
                "loop-owner eligible: yes"
            } else {
                "loop-owner eligible: no"
            };
            lines.push(loop_owner_ready.to_string());
        }

        lines.join("\n")
    }

    pub(crate) fn dashboard_message(&mut self) -> Result<String, String> {
        let mut lines = Vec::new();
        lines.push("Vivling control".to_string());
        lines.push("Ctrl+J opens the Vivling chat panel.".to_string());

        match self.status() {
            Ok(status) => lines.push(format!("Active: {status}")),
            Err(_) => lines.push("No active Vivling. Use /vivling hatch.".to_string()),
        }

        match self.roster_summary() {
            Ok(roster) => {
                lines.push(String::new());
                lines.push(roster);
            }
            Err(_) => {
                lines.push(String::new());
                lines.push("Roster: empty".to_string());
            }
        }

        lines.push(String::new());
        lines.push("Quick commands:".to_string());
        lines.push("/vivling hatch".to_string());
        lines.push("/vivling roster or /vivling list".to_string());
        lines.push("/vivling focus <id|name|alias> or /vivling switch <id|name|alias>".to_string());
        lines.push("/vivling card".to_string());
        lines.push("/vivling help".to_string());

        Ok(lines.join("\n"))
    }

    pub(crate) fn status(&mut self) -> Result<String, String> {
        self.ensure_hatched()?;
        let snapshot = {
            let state = self.state.as_mut().expect("state checked");
            state.apply_decay(Utc::now());
            state.clone()
        };
        let lineage_states = self
            .load_lineage_states(&snapshot.primary_vivling_id)
            .map_err(|err| err.to_string())?;
        let local_spawn_used = lineage_states
            .iter()
            .filter(|entry| !entry.is_primary && !entry.is_imported)
            .count();
        let local_spawn_unlocked = if snapshot.is_primary {
            snapshot.local_spawn_slots_unlocked()
        } else {
            lineage_states
                .iter()
                .find(|entry| entry.vivling_id == snapshot.primary_vivling_id)
                .map(|entry| entry.local_spawn_slots_unlocked())
                .unwrap_or(0)
        };
        let mut status = format!(
            "{} - local spawn slots {}/{} - top-level slots {}/{}",
            snapshot.status_summary(),
            local_spawn_used,
            local_spawn_unlocked,
            self.top_level_slot_usage().map_err(|err| err.to_string())?,
            EXTERNAL_SLOT_LIMIT
        );
        let loop_owner_ready = snapshot.stage() == Stage::Adult
            && snapshot.brain_enabled
            && snapshot.brain_profile.is_some();
        status.push_str(if loop_owner_ready {
            " - loop owner ready"
        } else {
            " - loop owner not ready"
        });
        self.save_state().map_err(|err| err.to_string())?;
        Ok(status)
    }

    pub(crate) fn update_existing<F>(&mut self, f: F) -> Result<String, String>
    where
        F: FnOnce(&mut VivlingState) -> String,
    {
        self.ensure_hatched()?;
        let message = {
            let state = self.state.as_mut().expect("state checked");
            state.apply_decay(Utc::now());
            f(state)
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(message)
    }

    pub(crate) fn update_existing_value<F, T>(&mut self, f: F) -> Result<T, String>
    where
        F: FnOnce(&mut VivlingState) -> T,
    {
        self.ensure_hatched()?;
        let value = {
            let state = self.state.as_mut().expect("state checked");
            state.apply_decay(Utc::now());
            f(state)
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(value)
    }

    pub(crate) fn update_existing_result<F>(&mut self, f: F) -> Result<String, String>
    where
        F: FnOnce(&mut VivlingState) -> Result<String, String>,
    {
        self.ensure_hatched()?;
        let message = {
            let state = self.state.as_mut().expect("state checked");
            state.apply_decay(Utc::now());
            f(state)?
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(message)
    }

    pub(crate) fn focus(&mut self, target: &str) -> Result<String, String> {
        let target_id = self
            .resolve_vivling_target(target)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("No Vivling matches `{target}`."))?;
        let mut state = self
            .load_state_for_id(&target_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("Vivling `{target_id}` is missing on disk."))?;
        let active_message = format!("{} active", state.name);
        state.last_message = Some(active_message.clone());
        state.last_work_summary = Some(active_message);
        let mut roster = self.load_roster().map_err(|err| err.to_string())?;
        roster.active_vivling_id = Some(target_id.clone());
        self.save_roster(&roster).map_err(|err| err.to_string())?;
        self.active_vivling_id = Some(target_id.clone());
        self.state = Some(state.clone());
        self.save_state().map_err(|err| err.to_string())?;
        Ok(format!(
            "Focused {} [{}] {} Lv {}.",
            state.vivling_id,
            state.lineage_role_label(),
            state.name,
            state.level
        ))
    }

    pub(crate) fn spawn_vivling(&mut self) -> Result<String, String> {
        self.ensure_hatched()?;
        let primary = self.state.as_ref().expect("state checked").clone();
        if !primary.is_primary {
            return Err("Only a primary Vivling can spawn a local lineage copy.".to_string());
        }
        if primary.level < JUVENILE_LEVEL {
            return Err("`/vivling spawn` unlocks only at level 30.".to_string());
        }
        let lineage_states = self
            .load_lineage_states(&primary.primary_vivling_id)
            .map_err(|err| err.to_string())?;
        let local_spawn_used = lineage_states
            .iter()
            .filter(|entry| !entry.is_primary && !entry.is_imported)
            .count();
        let local_spawn_unlocked = primary.local_spawn_slots_unlocked();
        if local_spawn_used >= local_spawn_unlocked {
            return Err(format!(
                "No free local spawn slots. Used {local_spawn_used}/{local_spawn_unlocked}."
            ));
        }
        let new_id = format!("viv-{}", Uuid::new_v4().simple());
        let instance_label = format!("spawn-{}", local_spawn_used + 1);
        let mut spawned = primary.create_spawned_clone(new_id.clone(), instance_label.clone());
        let existing_name_count = lineage_states
            .iter()
            .filter(|entry| entry.name == primary.name)
            .count();
        if existing_name_count > 0 {
            spawned.name = format!(
                "{} {}",
                primary.name,
                roman_numeral(existing_name_count + 1)
            );
        }
        self.save_state_record(&spawned, false, false)
            .map_err(|err| err.to_string())?;
        Ok(format!(
            "Spawned {} [{}] {}. Local spawn slots now {}/{}.",
            spawned.vivling_id,
            instance_label,
            spawned.name,
            local_spawn_used + 1,
            local_spawn_unlocked
        ))
    }

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
        let file = fs::File::open(&import_path).map_err(|err| err.to_string())?;
        let mut archive = ZipArchive::new(file).map_err(|err| err.to_string())?;
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
