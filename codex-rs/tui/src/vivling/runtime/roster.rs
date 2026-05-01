use super::*;

#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub(crate) struct VivlingRoster {
    #[serde(default)]
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) active_vivling_id: Option<String>,
    #[serde(default)]
    pub(crate) vivling_ids: Vec<String>,
    #[serde(default)]
    pub(crate) external_vivling_ids: Vec<String>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct VivlingPackageManifest {
    pub(crate) package_version: u32,
    pub(crate) exported_at: DateTime<Utc>,
    pub(crate) vivling_id: String,
    pub(crate) primary_vivling_id: String,
    pub(crate) species: String,
    pub(crate) rarity: String,
    pub(crate) level: u64,
    pub(crate) is_primary: bool,
    pub(crate) is_imported: bool,
    pub(crate) spawn_generation: u64,
}

impl Vivling {
    pub(crate) fn top_level_slot_usage(&self) -> io::Result<usize> {
        let roster = self.load_roster()?;
        let mut used = 0usize;
        for vivling_id in &roster.vivling_ids {
            if let Some(state) = self.load_state_for_id(vivling_id)?
                && (state.is_primary || state.is_imported)
            {
                used += 1;
            }
        }
        Ok(used)
    }

    pub(crate) fn config_path(&self) -> Option<PathBuf> {
        self.codex_home
            .as_ref()
            .map(|home| home.join(CONFIG_TOML_FILE))
    }

    fn load_config_text(&self) -> Result<String, String> {
        let path = self
            .config_path()
            .ok_or_else(|| "Vivling cannot find CODEX_HOME yet.".to_string())?;
        match fs::read_to_string(&path) {
            Ok(text) => Ok(text),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(err) => Err(format!("Failed to read {}: {err}", path.display())),
        }
    }

    pub(crate) fn model_summary(&mut self) -> Result<String, String> {
        self.ensure_hatched()?;
        let summary = self.state.as_ref().expect("state checked").brain_summary();
        if self
            .state
            .as_ref()
            .and_then(|state| state.brain_profile.as_ref())
            .is_some()
        {
            return Ok(summary);
        }

        let mut lines = vec![summary, "No Vivling brain profile is selected.".to_string()];
        let profiles = self.model_list()?;
        if profiles.contains("Vivling brain profiles:") {
            lines.push("Select one with `/vivling model <profile>`.".to_string());
        } else {
            lines.push("Create one with `/vivling model <model> [provider] [effort]`.".to_string());
        }
        lines.push(profiles);
        Ok(lines.join("\n"))
    }

    pub(crate) fn model_list(&mut self) -> Result<String, String> {
        self.ensure_hatched()?;
        let config_text = self.load_config_text()?;
        let config: ConfigToml = if config_text.trim().is_empty() {
            ConfigToml::default()
        } else {
            toml::from_str(&config_text).map_err(|err| {
                let path = self
                    .config_path()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "~/.codex/config.toml".to_string());
                format!("Failed to parse {path}: {err}")
            })?
        };
        let raw_config = if config_text.trim().is_empty() {
            toml::Value::Table(toml::map::Map::new())
        } else {
            toml::from_str::<toml::Value>(&config_text)
                .map_err(|err| format!("Failed to inspect model provider models: {err}"))?
        };
        let assigned = self
            .state
            .as_ref()
            .and_then(|state| state.brain_profile.as_deref())
            .map(str::to_string);

        let mut lines = Vec::new();
        if config.model.is_some()
            || config.model_provider.is_some()
            || config.model_reasoning_effort.is_some()
        {
            lines.push("Current config model:".to_string());
            let model = config.model.as_deref().unwrap_or("default");
            let provider = config.model_provider.as_deref().unwrap_or("default");
            let effort = config
                .model_reasoning_effort
                .map(|value| value.to_string())
                .unwrap_or_else(|| "default".to_string());
            lines.push(format!(
                "- model {model} · provider {provider} · effort {effort}"
            ));
            if config.model.is_some() {
                lines.push(format!(
                    "  use `/vivling model {model} {provider} {effort}` to assign it"
                ));
            }
        }

        if !config.profiles.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            let mut profile_names = config.profiles.keys().cloned().collect::<Vec<_>>();
            profile_names.sort();
            lines.push("Vivling brain profiles:".to_string());
            for profile_name in profile_names {
                let profile = config.profiles.get(&profile_name).expect("profile exists");
                let mark = if assigned.as_deref() == Some(profile_name.as_str()) {
                    "*"
                } else {
                    "-"
                };
                let model = profile.model.as_deref().unwrap_or("inherit");
                let provider = profile.model_provider.as_deref().unwrap_or("inherit");
                let effort = profile
                    .model_reasoning_effort
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "default".to_string());
                lines.push(format!(
                    "{mark} {profile_name} -> model {model} · provider {provider} · effort {effort}"
                ));
            }
        }

        let provider_models = configured_provider_models(&raw_config);
        if !provider_models.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push("Configured provider models:".to_string());
            for (provider, models) in provider_models {
                for model in models {
                    lines.push(format!(
                        "- {model} · provider {provider} · use `/vivling model {model} {provider}`"
                    ));
                }
            }
        }

        let catalog_models = configured_catalog_models(&config);
        if !catalog_models.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push("Configured OpenAI catalog models:".to_string());
            for (model, effort) in catalog_models {
                lines.push(format!(
                    "- {model} · provider openai · effort {effort} · use `/vivling model {model} openai {effort}`"
                ));
            }
        }

        if lines.is_empty() {
            lines.push("No models are configured in ~/.codex/config.toml yet.".to_string());
            lines.push(
                "Add `model`, `[profiles.*]`, `[model_providers.*].models`, or `model_catalog_json`."
                    .to_string(),
            );
        }
        Ok(lines.join("\n"))
    }

    pub(crate) fn remove_vivling(&mut self, target: &str) -> Result<String, String> {
        let target_id = self
            .resolve_vivling_target(target)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("No Vivling matches `{target}`."))?;
        if self.active_vivling_id.as_deref() == Some(target_id.as_str()) {
            return Err("Cannot remove the active Vivling. Focus another one first.".to_string());
        }
        let state = self
            .load_state_for_id(&target_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("Vivling `{target_id}` is missing on disk."))?;
        if state.is_primary {
            let lineage_states = self
                .load_lineage_states(&state.primary_vivling_id)
                .map_err(|err| err.to_string())?;
            let has_spawned_children = lineage_states.iter().any(|entry| {
                entry.vivling_id != state.vivling_id && !entry.is_imported && !entry.is_primary
            });
            if has_spawned_children {
                return Err(
                    "Cannot remove a primary Vivling while spawned lineage children still exist."
                        .to_string(),
                );
            }
        }
        if let Some(path) = self.state_path_for_id(&target_id) {
            match fs::remove_file(&path) {
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.to_string()),
            }
        }
        self.remove_from_roster(&target_id)
            .map_err(|err| err.to_string())?;
        Ok(format!(
            "Removed {} [{}] {} from the roster.",
            state.vivling_id,
            state.lineage_role_label(),
            state.name
        ))
    }

    pub(crate) fn seed_identity(&self) -> Option<SeedIdentity> {
        let codex_home = self.codex_home.as_ref()?;
        let auth = load_auth_dot_json(codex_home, self.auth_mode)
            .ok()
            .flatten();
        if let Some(tokens) = auth.as_ref().and_then(|auth| auth.tokens.as_ref()) {
            if let Some(account_id) = tokens.account_id.as_ref().filter(|value| !value.is_empty()) {
                return Some(SeedIdentity {
                    value: format!("account:{account_id}"),
                    install_id: self
                        .state
                        .as_ref()
                        .and_then(|state| state.install_id.clone()),
                });
            }
            if let Some(user_id) = tokens
                .id_token
                .chatgpt_user_id
                .as_ref()
                .filter(|value| !value.is_empty())
            {
                return Some(SeedIdentity {
                    value: format!("user:{user_id}"),
                    install_id: self
                        .state
                        .as_ref()
                        .and_then(|state| state.install_id.clone()),
                });
            }
        }
        let install_id = self
            .state
            .as_ref()
            .and_then(|state| state.install_id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        Some(SeedIdentity {
            value: format!("install:{install_id}"),
            install_id: Some(install_id),
        })
    }

    pub(crate) fn roster_dir(&self) -> Option<PathBuf> {
        self.codex_home.as_ref().map(|home| home.join(ROSTER_DIR))
    }

    pub(crate) fn export_dir(&self) -> Option<PathBuf> {
        self.roster_dir().map(|dir| dir.join(EXPORT_DIR))
    }

    pub(crate) fn legacy_state_path(&self) -> Option<PathBuf> {
        self.codex_home.as_ref().map(|home| home.join(STATE_FILE))
    }

    pub(crate) fn roster_path(&self) -> Option<PathBuf> {
        self.roster_dir().map(|dir| dir.join(ROSTER_FILE))
    }

    pub(crate) fn active_state_path(&self) -> Option<PathBuf> {
        let dir = self.roster_dir()?;
        let vivling_id = self.active_vivling_id.as_ref()?;
        Some(dir.join(format!("{vivling_id}.json")))
    }

    pub(crate) fn state_path_for_id(&self, vivling_id: &str) -> Option<PathBuf> {
        self.roster_dir()
            .map(|dir| dir.join(format!("{vivling_id}.json")))
    }

    pub(crate) fn load_roster(&self) -> io::Result<VivlingRoster> {
        let Some(path) = self.roster_path() else {
            return Ok(VivlingRoster::default());
        };
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Ok(VivlingRoster::default());
            }
            Err(err) => return Err(err),
        };
        serde_json::from_str(&text).map_err(io::Error::other)
    }

    pub(crate) fn save_roster(&self, roster: &VivlingRoster) -> io::Result<()> {
        let Some(path) = self.roster_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(roster).map_err(io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &text)?;
        fs::rename(&tmp, &path)
    }

    pub(crate) fn remove_from_roster(&self, vivling_id: &str) -> io::Result<()> {
        let mut roster = self.load_roster()?;
        roster.vivling_ids.retain(|entry| entry != vivling_id);
        roster
            .external_vivling_ids
            .retain(|entry| entry != vivling_id);
        if roster.active_vivling_id.as_deref() == Some(vivling_id) {
            roster.active_vivling_id = roster.vivling_ids.first().cloned();
        }
        self.save_roster(&roster)
    }

    pub(crate) fn migrate_legacy_state_if_needed(&mut self) -> io::Result<Option<VivlingState>> {
        let roster_path = self.roster_path();
        let legacy_path = self.legacy_state_path();
        if roster_path.as_ref().is_some_and(|path| path.exists())
            || legacy_path.as_ref().is_none_or(|path| !path.exists())
        {
            return Ok(None);
        }
        let path = legacy_path.expect("checked legacy path");
        let text = fs::read_to_string(&path)?;
        let mut state: VivlingState = serde_json::from_str(&text).map_err(io::Error::other)?;
        if state.vivling_id.trim().is_empty() {
            state.vivling_id = state
                .install_id
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string());
        }
        state.normalize_loaded_state();
        self.active_vivling_id = Some(state.vivling_id.clone());
        self.state = Some(state.clone());
        self.save_state()?;
        let _ = fs::remove_file(path);
        Ok(Some(state))
    }

    pub(crate) fn load_state(&self) -> io::Result<Option<VivlingState>> {
        let mut roster = self.load_roster()?;
        let active_id = roster
            .active_vivling_id
            .clone()
            .or_else(|| roster.vivling_ids.first().cloned());
        let Some(path) = self
            .roster_dir()
            .and_then(|dir| active_id.as_ref().map(|id| dir.join(format!("{id}.json"))))
        else {
            return Ok(None);
        };
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let mut state: VivlingState = serde_json::from_str(&text).map_err(io::Error::other)?;
        if let Some(active_id) = active_id {
            roster.active_vivling_id = Some(active_id.clone());
            let _ = self.save_roster(&roster);
            state.vivling_id = active_id;
        }
        if state.hatched {
            state.apply_decay(Utc::now());
        }
        state.normalize_loaded_state();
        Ok(Some(state))
    }

    pub(crate) fn load_state_for_id(&self, vivling_id: &str) -> io::Result<Option<VivlingState>> {
        let Some(path) = self.state_path_for_id(vivling_id) else {
            return Ok(None);
        };
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let mut state: VivlingState = serde_json::from_str(&text).map_err(io::Error::other)?;
        if state.hatched {
            state.apply_decay(Utc::now());
        }
        state.normalize_loaded_state();
        Ok(Some(state))
    }

    pub(crate) fn load_lineage_states(
        &self,
        primary_vivling_id: &str,
    ) -> io::Result<Vec<VivlingState>> {
        let roster = self.load_roster()?;
        let mut states = Vec::new();
        for vivling_id in roster.vivling_ids {
            if let Some(state) = self.load_state_for_id(&vivling_id)?
                && state.primary_vivling_id == primary_vivling_id
            {
                states.push(state);
            }
        }
        Ok(states)
    }

    pub(crate) fn save_state(&self) -> io::Result<()> {
        let Some(path) = self.active_state_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let Some(state) = &self.state else {
            return Ok(());
        };
        let mut roster = self.load_roster()?;
        if !roster
            .vivling_ids
            .iter()
            .any(|entry| entry == &state.vivling_id)
        {
            roster.vivling_ids.push(state.vivling_id.clone());
        }
        if state.is_imported
            && !roster
                .external_vivling_ids
                .iter()
                .any(|entry| entry == &state.vivling_id)
        {
            roster.external_vivling_ids.push(state.vivling_id.clone());
        }
        roster.version = state.version;
        roster.active_vivling_id = Some(state.vivling_id.clone());
        self.save_roster(&roster)?;
        let text = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &text)?;
        fs::rename(&tmp, &path)
    }

    pub(crate) fn save_state_record(
        &self,
        state: &VivlingState,
        set_active: bool,
        imported: bool,
    ) -> io::Result<()> {
        let Some(path) = self.state_path_for_id(&state.vivling_id) else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut roster = self.load_roster()?;
        if !roster
            .vivling_ids
            .iter()
            .any(|entry| entry == &state.vivling_id)
        {
            roster.vivling_ids.push(state.vivling_id.clone());
        }
        if imported
            && !roster
                .external_vivling_ids
                .iter()
                .any(|entry| entry == &state.vivling_id)
        {
            roster.external_vivling_ids.push(state.vivling_id.clone());
        }
        roster.version = state.version;
        if set_active {
            roster.active_vivling_id = Some(state.vivling_id.clone());
        }
        self.save_roster(&roster)?;
        let text = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &text)?;
        fs::rename(&tmp, &path)
    }

    pub(crate) fn resolve_vivling_target(&self, target: &str) -> io::Result<Option<String>> {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let roster = self.load_roster()?;
        let lower = trimmed.to_ascii_lowercase();
        if let Some(id) = roster
            .vivling_ids
            .iter()
            .find(|entry| entry.eq_ignore_ascii_case(trimmed))
        {
            return Ok(Some(id.clone()));
        }
        for vivling_id in roster.vivling_ids {
            if let Some(state) = self.load_state_for_id(&vivling_id)? {
                if state.name.eq_ignore_ascii_case(trimmed)
                    || state
                        .instance_label
                        .as_deref()
                        .is_some_and(|value| value.eq_ignore_ascii_case(trimmed))
                    || format!("{} {}", state.name, state.vivling_id)
                        .to_ascii_lowercase()
                        .contains(&lower)
                {
                    return Ok(Some(state.vivling_id));
                }
            }
        }
        Ok(None)
    }

    pub(crate) fn resolve_export_path(
        &self,
        cwd: &Path,
        maybe_path: Option<&str>,
        vivling_id: &str,
    ) -> Result<PathBuf, String> {
        let path = match maybe_path {
            Some(raw) => resolve_input_path(cwd, raw),
            None => self
                .export_dir()
                .ok_or_else(|| "Vivling cannot find CODEX_HOME yet.".to_string())?
                .join(format!("{vivling_id}.{VIVEGG_EXT}")),
        };
        Ok(ensure_extension(path, VIVEGG_EXT))
    }

    pub(crate) fn visible_state(&self) -> Option<&VivlingState> {
        self.state
            .as_ref()
            .filter(|state| state.hatched && state.visible)
    }
}

fn configured_provider_models(raw_config: &toml::Value) -> Vec<(String, Vec<String>)> {
    let Some(providers) = raw_config
        .as_table()
        .and_then(|table| table.get("model_providers"))
        .and_then(toml::Value::as_table)
    else {
        return Vec::new();
    };

    let mut entries = providers
        .iter()
        .filter_map(|(provider, value)| {
            let models = value
                .get("models")
                .and_then(toml::Value::as_array)?
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if models.is_empty() {
                None
            } else {
                Some((provider.to_string(), models))
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, models) in &mut entries {
        models.sort();
        models.dedup();
    }
    entries
}

fn configured_catalog_models(config: &ConfigToml) -> Vec<(String, String)> {
    let Some(path) = config.model_catalog_json.as_ref() else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(path.as_path()) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(models) = value.get("models").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };

    let mut entries = models
        .iter()
        .filter(|model| {
            model
                .get("visibility")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|visibility| visibility == "list")
        })
        .filter_map(|model| {
            let slug = model
                .get("slug")
                .and_then(serde_json::Value::as_str)
                .or_else(|| model.get("model").and_then(serde_json::Value::as_str))?
                .trim();
            if slug.is_empty() {
                return None;
            }
            let effort = model
                .get("default_reasoning_level")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("default")
                .to_string();
            Some((slug.to_string(), effort))
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries.dedup_by(|a, b| a.0 == b.0);
    entries
}
