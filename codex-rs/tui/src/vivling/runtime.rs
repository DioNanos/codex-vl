use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;
use std::{cell::Cell, cell::RefCell};

use chrono::DateTime;
use chrono::Utc;
use codex_config::CONFIG_TOML_FILE;
use codex_config::config_toml::ConfigToml;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::load_auth_dot_json;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize as _;
use ratatui::text::Line;
use ratatui::widgets::Widget;
use uuid::Uuid;

use crate::render::renderable::Renderable;
use crate::tui::FrameRequester;

use super::model::JUVENILE_LEVEL;
use super::model::MAX_CARD_REPLY_LEN;
use super::model::SeedIdentity;
use super::model::Stage;
use super::model::VivlingAiMode;
use super::model::VivlingLoopEvent;
use super::model::VivlingState;
use super::model::truncate_summary;
use super::registry::active_footer_sprites_for_species;
use super::registry::card_art_for_species;
use super::registry::species_for_id;
use super::zed::ZedTopic;
use super::zed::zed_panel_data;
use super::zed::zed_summary_for_topic;
use zip::CompressionMethod;
use zip::ZipArchive;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

const STATE_FILE: &str = "vivling.json";
const ROSTER_DIR: &str = "vivlings";
const ROSTER_FILE: &str = "roster.json";
const EXPORT_DIR: &str = "exports";
const VIVPKG_VERSION: u32 = 1;
const VIVEGG_EXT: &str = "vivegg";
const EXTERNAL_SLOT_LIMIT: usize = 3;
const ACTIVE_FOOTER_FRAME_INTERVAL: Duration = Duration::from_millis(140);
const ACTIVE_FOOTER_TAIL: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub(crate) struct Vivling {
    codex_home: Option<PathBuf>,
    auth_mode: AuthCredentialsStoreMode,
    state: Option<VivlingState>,
    active_vivling_id: Option<String>,
    frame_requester: Option<FrameRequester>,
    animations_enabled: bool,
    task_running: Cell<bool>,
    active_until: Cell<Option<Instant>>,
    active_started_at: Cell<Option<Instant>>,
    next_scheduled_frame_at: RefCell<Option<Instant>>,
}

#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
struct VivlingRoster {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    active_vivling_id: Option<String>,
    #[serde(default)]
    vivling_ids: Vec<String>,
    #[serde(default)]
    external_vivling_ids: Vec<String>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct VivlingPackageManifest {
    package_version: u32,
    exported_at: DateTime<Utc>,
    vivling_id: String,
    primary_vivling_id: String,
    species: String,
    rarity: String,
    level: u64,
    is_primary: bool,
    is_imported: bool,
    spawn_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VivlingPanelData {
    pub(crate) title: String,
    pub(crate) narrow_lines: Vec<String>,
    pub(crate) wide_lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VivlingBrainRequestKind {
    Assist,
    Chat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VivlingAssistRequest {
    pub(crate) vivling_id: String,
    pub(crate) vivling_name: String,
    pub(crate) brain_profile: String,
    pub(crate) kind: VivlingBrainRequestKind,
    pub(crate) task: String,
    pub(crate) prompt_context: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VivlingLoopTickRequest {
    pub(crate) vivling_id: String,
    pub(crate) vivling_name: String,
    pub(crate) brain_profile: String,
    pub(crate) loop_label: String,
    pub(crate) loop_goal: String,
    pub(crate) prompt_text: String,
    pub(crate) auto_remove_on_completion: bool,
    pub(crate) prompt_context: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub(crate) struct VivlingLoopTickResult {
    pub(crate) status: String,
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) loop_action: Option<VivlingLoopTickAction>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub(crate) struct VivlingLoopTickAction {
    pub(crate) action: String,
    #[serde(default)]
    pub(crate) interval: Option<String>,
    #[serde(default)]
    pub(crate) goal: Option<String>,
    #[serde(default)]
    pub(crate) prompt: Option<String>,
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VivlingBrainProfileRequestKind {
    AssignExisting {
        profile: String,
    },
    CreateOrUpdate {
        profile: String,
        model: String,
        provider: Option<String>,
        effort: Option<ReasoningEffortConfig>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VivlingBrainProfileRequest {
    pub(crate) vivling_id: String,
    pub(crate) vivling_name: String,
    pub(crate) kind: VivlingBrainProfileRequestKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VivlingCommandOutcome {
    Message(String),
    OpenCard(VivlingPanelData),
    OpenUpgrade(VivlingPanelData),
    DispatchAssist(VivlingAssistRequest),
    PersistBrainProfile(VivlingBrainProfileRequest),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum VivlingAction {
    Hatch,
    Help,
    Status,
    Roster,
    Focus(String),
    Spawn,
    Export(Option<String>),
    Import(String),
    Remove(String),
    Memory,
    Card,
    Upgrade,
    Assist(String),
    Brain(bool),
    ModelShow,
    ModelList,
    ModelProfile(String),
    ModelCustom {
        model: String,
        provider: Option<String>,
        effort: Option<ReasoningEffortConfig>,
    },
    Recap,
    PromoteEarly,
    PromoteAdult,
    Mode(VivlingAiMode),
    Chat(String),
    DirectMessage(String),
    Reset,
}

impl VivlingAction {
    pub(crate) fn parse(args: &str) -> Result<Self, String> {
        let trimmed = args.trim();
        if trimmed.is_empty() || trimmed == "status" {
            return Ok(Self::Status);
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or_default();
        let rest = parts.next().unwrap_or_default().trim();
        match cmd {
            "hatch" => Ok(Self::Hatch),
            "help" => Ok(Self::Help),
            "roster" => Ok(Self::Roster),
            "focus" => {
                if rest.is_empty() {
                    Err("Usage: /vivling focus <vivling_id_or_name>".to_string())
                } else {
                    Ok(Self::Focus(rest.to_string()))
                }
            }
            "spawn" => Ok(Self::Spawn),
            "export" => {
                if rest.is_empty() {
                    Ok(Self::Export(None))
                } else {
                    Ok(Self::Export(Some(rest.to_string())))
                }
            }
            "import" => {
                if rest.is_empty() {
                    Err("Usage: /vivling import <path.vivegg>".to_string())
                } else {
                    Ok(Self::Import(rest.to_string()))
                }
            }
            "remove" => {
                if rest.is_empty() {
                    Err("Usage: /vivling remove <vivling_id_or_name>".to_string())
                } else {
                    Ok(Self::Remove(rest.to_string()))
                }
            }
            "memory" => Ok(Self::Memory),
            "recap" => Ok(Self::Recap),
            "card" => Ok(Self::Card),
            "upgrade" => Ok(Self::Upgrade),
            "assist" => {
                if rest.is_empty() {
                    Err("Usage: /vivling assist <task>".to_string())
                } else {
                    Ok(Self::Assist(rest.to_string()))
                }
            }
            "brain" => match rest {
                "on" => Ok(Self::Brain(true)),
                "off" => Ok(Self::Brain(false)),
                _ => Err("Usage: /vivling brain <on|off>".to_string()),
            },
            "model" => Self::parse_model_action(rest),
            "promote" => match rest {
                "10" => Ok(Self::PromoteEarly),
                "60" => Ok(Self::PromoteAdult),
                _ => Err("Usage: /vivling promote <10|60>".to_string()),
            },
            "mode" => VivlingAiMode::parse(rest)
                .map(Self::Mode)
                .ok_or_else(|| "Usage: /vivling mode <on|off>".to_string()),
            "reset" => Ok(Self::Reset),
            _ => Ok(Self::DirectMessage(trimmed.to_string())),
        }
    }

    fn parse_model_action(rest: &str) -> Result<Self, String> {
        let trimmed = rest.trim();
        if trimmed.is_empty() {
            return Ok(Self::ModelShow);
        }
        if trimmed.eq_ignore_ascii_case("list") {
            return Ok(Self::ModelList);
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() == 1 {
            return Ok(Self::ModelProfile(parts[0].to_string()));
        }

        let model = parts[0].to_string();
        let mut provider = None;
        let mut effort = None;

        for token in parts.iter().skip(1) {
            if effort.is_none()
                && let Ok(parsed_effort) = token.parse::<ReasoningEffortConfig>()
            {
                effort = Some(parsed_effort);
                continue;
            }
            if provider.is_none() {
                provider = Some((*token).to_string());
                continue;
            }
            return Err(
                "Usage: /vivling model <profile> | /vivling model <model> [provider] [effort]"
                    .to_string(),
            );
        }

        Ok(Self::ModelCustom {
            model,
            provider,
            effort,
        })
    }
}

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

    pub(crate) fn command(
        &mut self,
        action: VivlingAction,
        cwd: &Path,
    ) -> Result<VivlingCommandOutcome, String> {
        match action {
            VivlingAction::Hatch => self.hatch().map(VivlingCommandOutcome::Message),
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
                .update_existing_result(|state| state.set_brain_enabled(enabled))
                .map(VivlingCommandOutcome::Message),
            VivlingAction::ModelShow => self
                .update_existing(|state| state.brain_summary())
                .map(VivlingCommandOutcome::Message),
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

    fn chat(&mut self, text: &str) -> Result<VivlingCommandOutcome, String> {
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

    fn hatch(&mut self) -> Result<String, String> {
        let top_level_used = self.top_level_slot_usage().map_err(|err| err.to_string())?;
        if top_level_used >= EXTERNAL_SLOT_LIMIT {
            return Err(format!(
                "All top-level Vivling slots are full ({EXTERNAL_SLOT_LIMIT}/{EXTERNAL_SLOT_LIMIT})."
            ));
        }
        let Some(seed) = self.seed_identity() else {
            return Err("Vivling cannot find CODEX_HOME yet.".to_string());
        };
        let mut state = VivlingState::new(seed);
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

    fn help_message(&self) -> String {
        let mut lines = vec![
            "Vivling commands:".to_string(),
            "/vivling hatch - hatch a new top-level Vivling while slots are free".to_string(),
            "/vivling status - show active Vivling status and slot usage".to_string(),
            "/vivling roster - list known Vivlings".to_string(),
            "/vivling focus <vivling_id_or_name> - switch active Vivling".to_string(),
            "/vivling card - open the current Vivling card".to_string(),
            "/vivling upgrade - open the ZED upgrade card".to_string(),
            "/vivling assist <task> - ask the Vivling brain for adult help".to_string(),
            "/vivling brain <on|off> - enable or disable the Vivling brain".to_string(),
            "/vivling model - show the current Vivling brain profile".to_string(),
            "/vivling model list - show assignable Vivling brain profiles".to_string(),
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

    fn status(&mut self) -> Result<String, String> {
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

    fn update_existing<F>(&mut self, f: F) -> Result<String, String>
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

    fn update_existing_value<F, T>(&mut self, f: F) -> Result<T, String>
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

    fn update_existing_result<F>(&mut self, f: F) -> Result<String, String>
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

    fn prepare_assist_request(&mut self, task: &str) -> Result<VivlingAssistRequest, String> {
        self.ensure_hatched()?;
        let (vivling_id, vivling_name, brain_profile, prompt_context, task) = {
            let state = self.state.as_mut().expect("state checked");
            state.apply_decay(Utc::now());
            let prompt_context = state.assist_prompt_context(task)?;
            let brain_profile = state.brain_profile.clone().ok_or_else(|| {
                "Set a Vivling brain profile first with `/vivling model ...`.".to_string()
            })?;
            (
                state.vivling_id.clone(),
                state.name.clone(),
                brain_profile,
                prompt_context,
                task.trim().to_string(),
            )
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(VivlingAssistRequest {
            vivling_id,
            vivling_name,
            brain_profile,
            kind: VivlingBrainRequestKind::Assist,
            task,
            prompt_context,
        })
    }

    fn prepare_chat_request(&mut self, text: &str) -> Result<VivlingAssistRequest, String> {
        self.ensure_hatched()?;
        let (vivling_id, vivling_name, brain_profile, prompt_context, task) = {
            let state = self.state.as_mut().expect("state checked");
            state.apply_decay(Utc::now());
            let prompt_context = state.chat_prompt_context(text)?;
            let brain_profile = state.brain_profile.clone().ok_or_else(|| {
                "Set a Vivling brain profile first with `/vivling model ...`.".to_string()
            })?;
            (
                state.vivling_id.clone(),
                state.name.clone(),
                brain_profile,
                prompt_context,
                text.trim().to_string(),
            )
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(VivlingAssistRequest {
            vivling_id,
            vivling_name,
            brain_profile,
            kind: VivlingBrainRequestKind::Chat,
            task,
            prompt_context,
        })
    }

    pub(crate) fn active_loop_owner_identity(&mut self) -> Result<(String, String), String> {
        self.ensure_hatched()?;
        let state = self.state.as_mut().expect("state checked");
        state.apply_decay(Utc::now());
        if state.stage() != Stage::Adult {
            return Err("Vivling loop ownership unlocks only at level 60.".to_string());
        }
        if !state.brain_enabled {
            return Err("Enable the Vivling brain first with `/vivling brain on`.".to_string());
        }
        if state.brain_profile.is_none() {
            return Err("Set a Vivling brain profile first with `/vivling model ...`.".to_string());
        }
        Ok((state.vivling_id.clone(), state.name.clone()))
    }

    pub(crate) fn prepare_loop_tick_request(
        &mut self,
        owner_vivling_id: &str,
        job: &codex_state::ThreadLoopJob,
    ) -> Result<VivlingLoopTickRequest, String> {
        let state = self
            .load_state_for_id(owner_vivling_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("Vivling owner `{owner_vivling_id}` is missing on disk."))?;
        if state.stage() != Stage::Adult {
            return Err(format!("Vivling owner `{}` is not adult yet.", state.name));
        }
        if !state.brain_enabled {
            return Err(format!(
                "Vivling owner `{}` has brain disabled.",
                state.name
            ));
        }
        let brain_profile = state.brain_profile.clone().ok_or_else(|| {
            format!(
                "Vivling owner `{}` has no brain profile configured.",
                state.name
            )
        })?;
        let goal = job
            .goal_text
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&job.prompt_text)
            .to_string();
        let prompt_context = format!(
            "Vivling id: {}\nVivling name: {}\nProfile: {}\nStage: {}\nDNA: {}\nTone: {}\nVerification bias: {}\nMemory digest:\n{}\n\nLoop label: {}\nLoop goal: {}\nLoop prompt: {}\nAuto remove on completion: {}\n\nReturn strict JSON with fields status, message, and optional loop_action. status must be one of progress, blocked, done. loop_action.action may be none, disable, remove, trigger, or update. For update you may optionally provide interval, goal, prompt, enabled.",
            state.vivling_id,
            state.name,
            brain_profile,
            state.stage().label(),
            state.dominant_archetype().label(),
            state.identity_profile.tone,
            state.identity_profile.verification_bias,
            state.memory_digest(),
            job.label,
            goal,
            job.prompt_text,
            job.auto_remove_on_completion,
        );
        Ok(VivlingLoopTickRequest {
            vivling_id: state.vivling_id.clone(),
            vivling_name: state.name.clone(),
            brain_profile,
            loop_label: job.label.clone(),
            loop_goal: goal,
            prompt_text: job.prompt_text.clone(),
            auto_remove_on_completion: job.auto_remove_on_completion,
            prompt_context,
        })
    }

    fn prepare_existing_profile_request(
        &mut self,
        profile: String,
    ) -> Result<VivlingBrainProfileRequest, String> {
        self.ensure_hatched()?;
        let state = self.state.as_ref().expect("state checked");
        Ok(VivlingBrainProfileRequest {
            vivling_id: state.vivling_id.clone(),
            vivling_name: state.name.clone(),
            kind: VivlingBrainProfileRequestKind::AssignExisting { profile },
        })
    }

    fn prepare_custom_profile_request(
        &mut self,
        model: String,
        provider: Option<String>,
        effort: Option<ReasoningEffortConfig>,
    ) -> Result<VivlingBrainProfileRequest, String> {
        self.ensure_hatched()?;
        let state = self.state.as_ref().expect("state checked");
        Ok(VivlingBrainProfileRequest {
            vivling_id: state.vivling_id.clone(),
            vivling_name: state.name.clone(),
            kind: VivlingBrainProfileRequestKind::CreateOrUpdate {
                profile: format!("vivling-{}", state.vivling_id),
                model,
                provider,
                effort,
            },
        })
    }

    fn ensure_hatched(&self) -> Result<(), String> {
        if self.state.as_ref().is_some_and(|state| state.hatched) {
            Ok(())
        } else {
            Err("No Vivling yet. Use /vivling hatch.".to_string())
        }
    }

    fn roster_summary(&mut self) -> Result<String, String> {
        let roster = self.load_roster().map_err(|err| err.to_string())?;
        if roster.vivling_ids.is_empty() {
            return Err("No Vivling yet. Use /vivling hatch.".to_string());
        }
        let mut lines = Vec::new();
        lines.push(format!(
            "Vivling roster · active {} · top-level slots {}/{}",
            roster
                .active_vivling_id
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            self.top_level_slot_usage().map_err(|err| err.to_string())?,
            EXTERNAL_SLOT_LIMIT
        ));
        for vivling_id in &roster.vivling_ids {
            if let Some(state) = self
                .load_state_for_id(vivling_id)
                .map_err(|err| err.to_string())?
            {
                let active_mark = if roster.active_vivling_id.as_deref() == Some(vivling_id) {
                    "*"
                } else {
                    "-"
                };
                let label = state
                    .instance_label
                    .as_deref()
                    .map(|value| format!(" · {value}"))
                    .unwrap_or_default();
                lines.push(format!(
                    "{active_mark} {} [{}] {} {} Lv {}{}",
                    state.vivling_id,
                    state.lineage_role_label(),
                    state.name,
                    species_for_id(&state.species).name,
                    state.level,
                    label
                ));
            }
        }
        Ok(lines.join("\n"))
    }

    fn focus(&mut self, target: &str) -> Result<String, String> {
        let target_id = self
            .resolve_vivling_target(target)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("No Vivling matches `{target}`."))?;
        let state = self
            .load_state_for_id(&target_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("Vivling `{target_id}` is missing on disk."))?;
        let mut roster = self.load_roster().map_err(|err| err.to_string())?;
        roster.active_vivling_id = Some(target_id.clone());
        self.save_roster(&roster).map_err(|err| err.to_string())?;
        self.active_vivling_id = Some(target_id.clone());
        self.state = Some(state.clone());
        Ok(format!(
            "Focused {} [{}] {} Lv {}.",
            state.vivling_id,
            state.lineage_role_label(),
            state.name,
            state.level
        ))
    }

    fn spawn_vivling(&mut self) -> Result<String, String> {
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

    fn export_active(&mut self, cwd: &Path, maybe_path: Option<&str>) -> Result<String, String> {
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

    fn import_package(&mut self, cwd: &Path, raw_path: &str) -> Result<String, String> {
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

    fn top_level_slot_usage(&self) -> io::Result<usize> {
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

    fn config_path(&self) -> Option<PathBuf> {
        self.codex_home
            .as_ref()
            .map(|home| home.join(CONFIG_TOML_FILE))
    }

    fn load_config_toml(&self) -> Result<ConfigToml, String> {
        let path = self
            .config_path()
            .ok_or_else(|| "Vivling cannot find CODEX_HOME yet.".to_string())?;
        let text = fs::read_to_string(&path)
            .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
        toml::from_str(&text).map_err(|err| format!("Failed to parse {}: {err}", path.display()))
    }

    fn model_list(&mut self) -> Result<String, String> {
        self.ensure_hatched()?;
        let config = self.load_config_toml()?;
        if config.profiles.is_empty() {
            return Ok("No config profiles are defined in ~/.codex/config.toml yet.".to_string());
        }
        let assigned = self
            .state
            .as_ref()
            .and_then(|state| state.brain_profile.as_deref())
            .map(str::to_string);
        let mut profile_names = config.profiles.keys().cloned().collect::<Vec<_>>();
        profile_names.sort();
        let mut lines = vec!["Vivling brain profiles:".to_string()];
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
        Ok(lines.join("\n"))
    }

    fn remove_vivling(&mut self, target: &str) -> Result<String, String> {
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

    fn seed_identity(&self) -> Option<SeedIdentity> {
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

    fn roster_dir(&self) -> Option<PathBuf> {
        self.codex_home.as_ref().map(|home| home.join(ROSTER_DIR))
    }

    fn export_dir(&self) -> Option<PathBuf> {
        self.roster_dir().map(|dir| dir.join(EXPORT_DIR))
    }

    fn legacy_state_path(&self) -> Option<PathBuf> {
        self.codex_home.as_ref().map(|home| home.join(STATE_FILE))
    }

    fn roster_path(&self) -> Option<PathBuf> {
        self.roster_dir().map(|dir| dir.join(ROSTER_FILE))
    }

    fn active_state_path(&self) -> Option<PathBuf> {
        let dir = self.roster_dir()?;
        let vivling_id = self.active_vivling_id.as_ref()?;
        Some(dir.join(format!("{vivling_id}.json")))
    }

    fn state_path_for_id(&self, vivling_id: &str) -> Option<PathBuf> {
        self.roster_dir()
            .map(|dir| dir.join(format!("{vivling_id}.json")))
    }

    fn load_roster(&self) -> io::Result<VivlingRoster> {
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

    fn save_roster(&self, roster: &VivlingRoster) -> io::Result<()> {
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

    fn remove_from_roster(&self, vivling_id: &str) -> io::Result<()> {
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

    fn migrate_legacy_state_if_needed(&mut self) -> io::Result<Option<VivlingState>> {
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

    fn load_state(&self) -> io::Result<Option<VivlingState>> {
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

    fn load_state_for_id(&self, vivling_id: &str) -> io::Result<Option<VivlingState>> {
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

    fn load_lineage_states(&self, primary_vivling_id: &str) -> io::Result<Vec<VivlingState>> {
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

    fn save_state(&self) -> io::Result<()> {
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

    fn save_state_record(
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

    fn resolve_vivling_target(&self, target: &str) -> io::Result<Option<String>> {
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

    fn resolve_export_path(
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

    fn visible_state(&self) -> Option<&VivlingState> {
        self.state
            .as_ref()
            .filter(|state| state.hatched && state.visible)
    }

    pub(crate) fn record_loop_event(&mut self, event: VivlingLoopEvent) -> Result<(), String> {
        self.update_existing(|state| {
            state.record_loop_event(&event);
            state
                .last_message
                .clone()
                .unwrap_or_else(|| format!("noticed loop {} `{}`", event.action, event.label))
        })
        .map(|_| {
            self.mark_recent_activity(ACTIVE_FOOTER_TAIL);
        })
    }

    pub(crate) fn record_turn_completed(&mut self, summary: Option<&str>) -> Result<(), String> {
        self.update_existing(|state| {
            state.record_turn_completed(summary);
            state
                .last_message
                .clone()
                .unwrap_or_else(|| "is learning from completed work".to_string())
        })
        .map(|_| {
            self.mark_recent_activity(ACTIVE_FOOTER_TAIL);
        })
    }

    pub(crate) fn assign_brain_profile(&mut self, profile: String) -> Result<String, String> {
        self.update_existing(|state| state.assign_brain_profile(profile))
    }

    pub(crate) fn mark_brain_runtime_error(&mut self, error: &str) -> Result<(), String> {
        self.update_existing(|state| {
            state.mark_brain_runtime_error(error);
            state
                .brain_last_error
                .clone()
                .unwrap_or_else(|| "Vivling brain failed.".to_string())
        })
        .map(|_| ())
    }

    pub(crate) fn mark_brain_runtime_error_for(
        &mut self,
        vivling_id: &str,
        error: &str,
    ) -> Result<(), String> {
        let mut state = self
            .load_state_for_id(vivling_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("Vivling `{vivling_id}` is missing on disk."))?;
        state.mark_brain_runtime_error(error);
        self.save_state_record(&state, /*set_active*/ false, state.is_imported)
            .map_err(|err| err.to_string())?;
        if self.active_vivling_id.as_deref() == Some(vivling_id) {
            self.state = Some(state);
        }
        Ok(())
    }

    pub(crate) fn mark_brain_reply(&mut self, reply: &str) -> Result<(), String> {
        self.update_existing(|state| {
            state.mark_brain_reply(reply);
            truncate_summary(reply, MAX_CARD_REPLY_LEN)
        })
        .map(|_| {
            self.mark_recent_activity(ACTIVE_FOOTER_TAIL);
        })
    }

    pub(crate) fn mark_brain_reply_for(
        &mut self,
        vivling_id: &str,
        reply: &str,
    ) -> Result<(), String> {
        let mut state = self
            .load_state_for_id(vivling_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("Vivling `{vivling_id}` is missing on disk."))?;
        state.mark_brain_reply(reply);
        self.save_state_record(&state, /*set_active*/ false, state.is_imported)
            .map_err(|err| err.to_string())?;
        if self.active_vivling_id.as_deref() == Some(vivling_id) {
            self.state = Some(state);
        }
        self.mark_recent_activity(ACTIVE_FOOTER_TAIL);
        Ok(())
    }

    fn mark_recent_activity(&self, tail: Duration) {
        let now = Instant::now();
        if !self.is_active_at(now) {
            self.active_started_at.set(Some(now));
        }
        let deadline = now + tail;
        let current = self.active_until.get();
        if current.is_none_or(|existing| existing < deadline) {
            self.active_until.set(Some(deadline));
        }
        self.request_frame();
    }

    fn request_frame(&self) {
        if let Some(frame_requester) = &self.frame_requester {
            frame_requester.schedule_frame();
        }
    }

    fn is_active_at(&self, now: Instant) -> bool {
        self.task_running.get()
            || self
                .active_until
                .get()
                .is_some_and(|deadline| deadline > now)
    }

    fn current_sprite(&self, state: &VivlingState, now: Instant) -> String {
        let species = species_for_id(&state.species);
        if !self.animations_enabled {
            *self.next_scheduled_frame_at.borrow_mut() = None;
            return match state.stage() {
                Stage::Baby => species.ascii_baby.clone(),
                Stage::Juvenile => species.ascii_juvenile.clone(),
                Stage::Adult => species.ascii_adult.clone(),
            };
        }

        let frames = active_footer_sprites_for_species(species, state.stage());
        let started = self.active_started_at.get().unwrap_or_else(|| {
            self.active_started_at.set(Some(now));
            now
        });
        let elapsed = now.saturating_duration_since(started);
        let frame_idx =
            (((elapsed.as_millis() / ACTIVE_FOOTER_FRAME_INTERVAL.as_millis()) as usize) + 1)
                % frames.len();
        let next_deadline = now + ACTIVE_FOOTER_FRAME_INTERVAL;
        let should_schedule = self
            .next_scheduled_frame_at
            .borrow()
            .is_none_or(|deadline| deadline <= now);
        if should_schedule {
            if let Some(frame_requester) = &self.frame_requester {
                frame_requester.schedule_frame_in(ACTIVE_FOOTER_FRAME_INTERVAL);
            }
            *self.next_scheduled_frame_at.borrow_mut() = Some(next_deadline);
        }
        frames[frame_idx].clone()
    }
}

impl Renderable for Vivling {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let Some(state) = self.visible_state() else {
            return;
        };
        if area.height == 0 || area.width < 18 {
            return;
        }
        let species = species_for_id(&state.species);
        let now = Instant::now();
        let sprite = self.current_sprite(state, now);
        let msg = state
            .last_message
            .as_deref()
            .unwrap_or("is watching the session");
        let compact = format!(
            "{} {} {} Lv {}: {}",
            state.name,
            sprite.as_str(),
            state.dominant_archetype().label(),
            state.level,
            truncate_summary(msg, 56)
        );
        let line = if area.width < 72 || area.height == 1 {
            Line::from(compact).dim()
        } else {
            Line::from(vec![
                state.name.clone().bold(),
                " ".into(),
                sprite.cyan(),
                format!(
                    " {} Lv {}: {} · {} {} {} · days {}",
                    state.dominant_archetype().label(),
                    state.level,
                    truncate_summary(msg, 72),
                    state.stage().label(),
                    state.rarity,
                    species.name,
                    state.active_work_days,
                )
                .dim(),
            ])
        };
        line.render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        if self.visible_state().is_some() && width >= 18 {
            1
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vivling::model::ADULT_LEVEL;
    use crate::vivling::model::WORK_XP_PER_LEVEL;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::ZipArchive;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    fn seeded_state() -> VivlingState {
        VivlingState::new(SeedIdentity {
            value: "install:test-seed".to_string(),
            install_id: Some("test-seed".to_string()),
        })
    }

    fn leveled_state(level: u64, active_days: u64) -> VivlingState {
        let mut state = seeded_state();
        state.active_work_days = active_days;
        state.work_xp = WORK_XP_PER_LEVEL.saturating_mul(level.saturating_sub(1));
        state.recompute_level();
        state
    }

    fn configured_vivling(home: &Path) -> Vivling {
        let mut vivling = Vivling::unavailable();
        vivling.configure(home, AuthCredentialsStoreMode::default());
        vivling.configure_runtime(FrameRequester::test_dummy(), false);
        vivling
    }

    fn hatched_vivling(home: &Path) -> Vivling {
        let mut vivling = configured_vivling(home);
        let _ = vivling
            .command(VivlingAction::Hatch, home)
            .expect("hatch vivling");
        vivling
    }

    fn set_active_level(vivling: &mut Vivling, level: u64) -> VivlingState {
        let mut state = vivling.state.clone().expect("active state");
        state.active_work_days = if level >= ADULT_LEVEL {
            90
        } else if level >= JUVENILE_LEVEL {
            30
        } else {
            level.max(1)
        };
        state.work_xp = WORK_XP_PER_LEVEL.saturating_mul(level.saturating_sub(1));
        state.xp = state.work_xp;
        state.recompute_level();
        vivling.active_vivling_id = Some(state.vivling_id.clone());
        vivling.state = Some(state.clone());
        vivling.save_state().expect("save leveled state");
        state
    }

    fn spawn_ids(vivling: &Vivling, primary_id: &str) -> Vec<String> {
        vivling
            .load_roster()
            .expect("roster")
            .vivling_ids
            .into_iter()
            .filter(|id| id != primary_id)
            .collect()
    }

    fn make_package(path: &Path, manifest: &VivlingPackageManifest, state: &VivlingState) {
        let file = File::create(path).expect("create vivegg");
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("manifest.json", options)
            .expect("manifest entry");
        zip.write_all(
            serde_json::to_string_pretty(manifest)
                .expect("manifest json")
                .as_bytes(),
        )
        .expect("write manifest");
        zip.start_file("state.json", options).expect("state entry");
        zip.write_all(
            serde_json::to_string_pretty(state)
                .expect("state json")
                .as_bytes(),
        )
        .expect("write state");
        zip.finish().expect("finish package");
    }

    fn exportable_state(level: u64) -> VivlingState {
        let mut state = leveled_state(
            level,
            if level >= ADULT_LEVEL {
                90
            } else if level >= JUVENILE_LEVEL {
                30
            } else {
                level.max(1)
            },
        );
        state.primary_vivling_id = state.vivling_id.clone();
        state.is_primary = true;
        state
    }

    #[test]
    fn active_footer_pose_changes_while_task_running() {
        let mut vivling = Vivling::unavailable();
        vivling.state = Some(seeded_state());
        vivling.configure_runtime(FrameRequester::test_dummy(), true);
        vivling.set_task_running(true);

        let state = vivling.visible_state().expect("hatched state");
        let sprite = vivling.current_sprite(state, Instant::now());
        assert_ne!(sprite, species_for_id(&state.species).ascii_baby);
    }

    #[test]
    fn footer_pose_animates_while_visible_and_idle() {
        let mut vivling = Vivling::unavailable();
        vivling.state = Some(seeded_state());
        vivling.configure_runtime(FrameRequester::test_dummy(), true);

        let state = vivling.visible_state().expect("hatched state");
        let start = Instant::now();
        let first = vivling.current_sprite(state, start);
        let second = vivling.current_sprite(state, start + ACTIVE_FOOTER_FRAME_INTERVAL);
        assert_ne!(first, second);
    }

    #[test]
    fn static_footer_pose_used_when_animations_disabled() {
        let mut vivling = Vivling::unavailable();
        vivling.state = Some(seeded_state());
        vivling.configure_runtime(FrameRequester::test_dummy(), false);
        vivling.set_task_running(true);

        let state = vivling.visible_state().expect("hatched state");
        let sprite = vivling.current_sprite(state, Instant::now());
        assert_eq!(sprite, species_for_id(&state.species).ascii_baby);
    }

    #[test]
    fn render_keeps_vivling_line_shape() {
        let mut vivling = Vivling::unavailable();
        vivling.state = Some(seeded_state());
        vivling.configure_runtime(FrameRequester::test_dummy(), true);
        vivling.set_task_running(true);

        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        vivling.render(area, &mut buf);
        let rendered = buf
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Lv 1:"));
    }

    #[test]
    fn action_parse_supports_spawn_transfer_and_roster_commands() {
        assert_eq!(VivlingAction::parse("help"), Ok(VivlingAction::Help));
        assert_eq!(VivlingAction::parse("roster"), Ok(VivlingAction::Roster));
        assert_eq!(VivlingAction::parse("spawn"), Ok(VivlingAction::Spawn));
        assert_eq!(
            VivlingAction::parse("assist review the blocker"),
            Ok(VivlingAction::Assist("review the blocker".to_string()))
        );
        assert_eq!(
            VivlingAction::parse("brain on"),
            Ok(VivlingAction::Brain(true))
        );
        assert_eq!(
            VivlingAction::parse("model list"),
            Ok(VivlingAction::ModelList)
        );
        assert_eq!(
            VivlingAction::parse("model spark-fast"),
            Ok(VivlingAction::ModelProfile("spark-fast".to_string()))
        );
        assert_eq!(
            VivlingAction::parse("model gpt-5.3-codex-spark zai-a high"),
            Ok(VivlingAction::ModelCustom {
                model: "gpt-5.3-codex-spark".to_string(),
                provider: Some("zai-a".to_string()),
                effort: Some(ReasoningEffortConfig::High),
            })
        );
        assert_eq!(VivlingAction::parse("recap"), Ok(VivlingAction::Recap));
        assert_eq!(
            VivlingAction::parse("promote 10"),
            Ok(VivlingAction::PromoteEarly)
        );
        assert_eq!(
            VivlingAction::parse("promote 60"),
            Ok(VivlingAction::PromoteAdult)
        );
        assert_eq!(
            VivlingAction::parse("focus viv-123"),
            Ok(VivlingAction::Focus("viv-123".to_string()))
        );
        assert_eq!(
            VivlingAction::parse("export out.vivegg"),
            Ok(VivlingAction::Export(Some("out.vivegg".to_string())))
        );
        assert_eq!(
            VivlingAction::parse("import in.vivegg"),
            Ok(VivlingAction::Import("in.vivegg".to_string()))
        );
        assert_eq!(
            VivlingAction::parse("remove viv-123"),
            Ok(VivlingAction::Remove("viv-123".to_string()))
        );
    }

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
            VivlingCommandOutcome::Message(message) => message,
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
    }

    #[test]
    fn help_lists_supported_commands_instead_of_falling_back_to_chat() {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = configured_vivling(temp.path());

        let message = match vivling
            .command(VivlingAction::Help, temp.path())
            .expect("help should work")
        {
            VivlingCommandOutcome::Message(message) => message,
            other => panic!("unexpected outcome: {other:?}"),
        };

        assert!(message.contains("Vivling commands:"));
        assert!(message.contains("/vivling hatch"));
        assert!(message.contains("/vivling status"));
        assert!(message.contains("/vivling roster"));
        assert!(message.contains("/vivling spawn"));
        assert!(message.contains("/vivling assist <task>"));
        assert!(message.contains("/vivling brain <on|off>"));
        assert!(message.contains("/vivling model"));
        assert!(message.contains("/vivling model list"));
        assert!(message.contains("/vivling recap"));
        assert!(message.contains("/vivling promote 10"));
        assert!(message.contains("/vivling promote 60"));
        assert!(message.contains("/vivling export [path.vivegg]"));
        assert!(message.contains("/vivling import <path.vivegg>"));
        assert!(message.contains("/vivling <message>"));
    }

    #[test]
    fn hatch_fills_top_level_slots_before_failing() {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = configured_vivling(temp.path());

        for expected in 1..=EXTERNAL_SLOT_LIMIT {
            let message = match vivling
                .command(VivlingAction::Hatch, temp.path())
                .expect("hatch should work")
            {
                VivlingCommandOutcome::Message(message) => message,
                other => panic!("unexpected outcome: {other:?}"),
            };
            assert!(message.contains(&format!(
                "Top-level slots now {expected}/{EXTERNAL_SLOT_LIMIT}"
            )));
        }

        let err = vivling
            .command(VivlingAction::Hatch, temp.path())
            .expect_err("slots should be full");
        assert!(err.contains("All top-level Vivling slots are full"));

        let roster = vivling.load_roster().expect("roster");
        assert_eq!(roster.vivling_ids.len(), EXTERNAL_SLOT_LIMIT);
    }

    #[test]
    fn promote_10_applies_early_seed_baseline() {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = hatched_vivling(temp.path());

        let message = match vivling
            .command(VivlingAction::PromoteEarly, temp.path())
            .expect("promote")
        {
            VivlingCommandOutcome::Message(message) => message,
            other => panic!("unexpected outcome: {other:?}"),
        };

        assert!(message.contains("level 10"));
        let state = vivling.state.as_ref().expect("state");
        assert_eq!(state.level, 10);
        assert!(!state.adult_bootstrap);
        assert_eq!(state.seed_origin.as_deref(), Some("early_seed_v1"));
        assert!(!state.work_memory.is_empty());
        assert_eq!(state.stage(), Stage::Baby);
    }

    #[test]
    fn promote_60_applies_adult_seed_baseline() {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = hatched_vivling(temp.path());

        let message = match vivling
            .command(VivlingAction::PromoteAdult, temp.path())
            .expect("promote")
        {
            VivlingCommandOutcome::Message(message) => message,
            other => panic!("unexpected outcome: {other:?}"),
        };

        assert!(message.contains("adult baseline"));
        let state = vivling.state.as_ref().expect("state");
        assert_eq!(state.level, ADULT_LEVEL);
        assert!(state.adult_bootstrap);
        assert_eq!(state.seed_origin.as_deref(), Some("adult_seed_v1"));
        assert!(!state.work_memory.is_empty());
        assert!(!state.distilled_summaries.is_empty());
    }

    #[test]
    fn promote_60_persists_across_new_instance_reload() {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = hatched_vivling(temp.path());

        let _ = vivling
            .command(VivlingAction::PromoteAdult, temp.path())
            .expect("promote adult");

        let reloaded = configured_vivling(temp.path());
        let state = reloaded.state.as_ref().expect("reloaded state");
        assert_eq!(state.level, ADULT_LEVEL);
        assert!(state.adult_bootstrap);
        assert_eq!(state.seed_origin.as_deref(), Some("adult_seed_v1"));
    }

    #[test]
    fn recap_reads_synthesized_memory_view() {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = hatched_vivling(temp.path());
        let _ = vivling
            .command(VivlingAction::PromoteEarly, temp.path())
            .expect("promote early");

        let message = match vivling
            .command(VivlingAction::Recap, temp.path())
            .expect("recap")
        {
            VivlingCommandOutcome::Message(message) => message,
            other => panic!("unexpected outcome: {other:?}"),
        };

        assert!(message.contains("stage baby"));
        assert!(message.contains("distilled:"));
        assert!(message.contains("paths:"));
    }

    #[test]
    fn model_list_reads_profiles_from_config() {
        let temp = TempDir::new().expect("tempdir");
        fs::write(
            temp.path().join(CONFIG_TOML_FILE),
            r#"
[profiles.vivling-spark]
model = "gpt-5.3-codex-spark"
model_provider = "openai"
model_reasoning_effort = "high"

[profiles.local-ollama]
model = "glm-5.1:cloud"
model_provider = "lm-studio"
"#,
        )
        .expect("write config");
        let mut vivling = hatched_vivling(temp.path());
        let _ = vivling.assign_brain_profile("vivling-spark".to_string());

        let message = match vivling
            .command(VivlingAction::ModelList, temp.path())
            .expect("model list")
        {
            VivlingCommandOutcome::Message(message) => message,
            other => panic!("unexpected outcome: {other:?}"),
        };

        assert!(message.contains("* vivling-spark -> model gpt-5.3-codex-spark"));
        assert!(message.contains("- local-ollama -> model glm-5.1:cloud"));
    }

    #[test]
    fn assigning_brain_profile_after_adult_promotion_auto_enables_brain() {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = hatched_vivling(temp.path());
        let _ = vivling
            .command(VivlingAction::PromoteAdult, temp.path())
            .expect("promote adult");

        let message = vivling
            .assign_brain_profile("vivling-spark".to_string())
            .expect("assign profile");
        assert!(message.contains("brain enabled"));

        let summary = match vivling
            .command(VivlingAction::ModelShow, temp.path())
            .expect("model show")
        {
            VivlingCommandOutcome::Message(message) => message,
            other => panic!("unexpected outcome: {other:?}"),
        };

        assert!(summary.contains("brain on"));
        assert!(summary.contains("vivling-spark"));
    }

    #[test]
    fn adult_direct_chat_is_role_focused_instead_of_generic() {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = hatched_vivling(temp.path());
        let _ = vivling
            .command(VivlingAction::PromoteAdult, temp.path())
            .expect("promote adult");

        let message = match vivling
            .command(
                VivlingAction::DirectMessage("Dimmi cosa faresti".to_string()),
                temp.path(),
            )
            .expect("chat")
        {
            VivlingCommandOutcome::Message(message) => message,
            other => panic!("unexpected outcome: {other:?}"),
        };

        assert!(message.contains("As reviewer"));
        assert!(!message.contains("I remember this pattern. Lately:"));
    }

    #[test]
    fn chat_falls_back_to_direct_reply_without_ready_brain() {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = hatched_vivling(temp.path());

        let message = match vivling
            .command(VivlingAction::Chat("ciao bello".to_string()), temp.path())
            .expect("chat")
        {
            VivlingCommandOutcome::Message(message) => message,
            other => panic!("unexpected outcome: {other:?}"),
        };

        assert!(message.contains("I'm ") || message.contains("As "));
    }

    #[test]
    fn adult_chat_with_ready_brain_dispatches_chat_request() {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = hatched_vivling(temp.path());
        let _ = vivling
            .command(VivlingAction::PromoteAdult, temp.path())
            .expect("promote adult");
        let _ = vivling
            .assign_brain_profile("vivling-spark".to_string())
            .expect("assign profile");

        let request = match vivling
            .command(VivlingAction::Chat("ciao bello".to_string()), temp.path())
            .expect("chat")
        {
            VivlingCommandOutcome::DispatchAssist(request) => request,
            other => panic!("unexpected outcome: {other:?}"),
        };

        assert_eq!(request.kind, VivlingBrainRequestKind::Chat);
        assert_eq!(request.task, "ciao bello");
        assert_eq!(request.brain_profile, "vivling-spark");
        assert!(request.prompt_context.contains("User message:\nciao bello"));
        assert!(request.prompt_context.contains("Live state contract:"));
    }

    #[test]
    fn assist_request_keeps_assist_kind() {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = hatched_vivling(temp.path());
        let _ = vivling
            .command(VivlingAction::PromoteAdult, temp.path())
            .expect("promote adult");
        let _ = vivling
            .assign_brain_profile("vivling-spark".to_string())
            .expect("assign profile");

        let request = match vivling
            .command(
                VivlingAction::Assist("review this blocker".to_string()),
                temp.path(),
            )
            .expect("assist")
        {
            VivlingCommandOutcome::DispatchAssist(request) => request,
            other => panic!("unexpected outcome: {other:?}"),
        };

        assert_eq!(request.kind, VivlingBrainRequestKind::Assist);
        assert_eq!(request.task, "review this blocker");
    }

    #[test]
    fn assist_prompt_context_declares_memory_and_live_state_boundary() {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = hatched_vivling(temp.path());
        let _ = vivling
            .command(VivlingAction::PromoteAdult, temp.path())
            .expect("promote adult");
        let _ = vivling
            .assign_brain_profile("vivling-spark".to_string())
            .expect("assign profile");

        let prompt = vivling
            .state
            .as_ref()
            .expect("state")
            .assist_prompt_context("review this blocker")
            .expect("prompt");

        assert!(prompt.contains("Learned memory:"));
        assert!(prompt.contains("Live state contract:"));
        assert!(prompt.contains("Live state is unknown unless the task explicitly provides it."));
        assert!(prompt.contains("Task:\nreview this blocker"));
    }

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
                    VivlingCommandOutcome::Message(message) => message,
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
            VivlingCommandOutcome::Message(message) => assert!(message.contains("Spawned")),
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
        let mut raw =
            serde_json::to_value(&legacy_state).expect("serialize legacy state for rewrite");
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
}

fn render_vivling_card(state: &mut VivlingState) -> VivlingPanelData {
    let species = species_for_id(&state.species);
    let displayed = state.work_affinities.totals_with_bias(state.species_bias());
    let memory = state
        .last_work_summary
        .as_deref()
        .map(|summary| truncate_summary(summary, MAX_CARD_REPLY_LEN))
        .unwrap_or_else(|| "No work memory yet.".to_string());

    let build_lines = |width_hint: usize| {
        let art = card_art_for_species(species, state.stage(), width_hint);
        let mut lines = Vec::new();
        lines.push(format!(
            "{} · {} {} {} · Lv {}",
            state.name,
            state.stage().label(),
            state.rarity,
            species.name,
            state.level
        ));
        lines.push(format!(
            "DNA {} · mood {} · mode {} · active days {}",
            state.dominant_archetype().label(),
            state.mood(),
            state.ai_mode.label(),
            state.active_work_days
        ));
        lines.push(format!(
            "Tone {} · recent {} · distilled {} · paths {}",
            state.identity_profile.tone,
            state.work_memory.len(),
            state.distilled_summaries.len(),
            state.mental_paths.len()
        ));
        if let Some(upgrade) = state.pending_upgrade {
            lines.push(format!("Upgrade ready: {}", upgrade.prompt()));
        }
        lines.push(String::new());
        lines.extend(art.lines);
        lines.push(String::new());
        lines.push(format!(
            "Stats  B:{}  R:{}  D:{}  O:{}",
            displayed[0].1, displayed[1].1, displayed[2].1, displayed[3].1
        ));
        lines.push(format!(
            "Loops {} · blocks {} · churn {} · turns {}",
            state.loop_exposure,
            state.loop_runtime_blocks,
            state.loop_profile.noisy_churn,
            state.turns_observed
        ));
        lines.push(format!("Last: {}", memory));
        lines
    };

    VivlingPanelData {
        title: format!("{} · Card", state.name),
        narrow_lines: build_lines(64),
        wide_lines: build_lines(120),
    }
}

fn render_upgrade_card(state: &mut VivlingState) -> VivlingPanelData {
    let pending_or_seen_topic = state
        .pending_upgrade
        .map(|upgrade| ZedTopic::from_slug(upgrade.slug()))
        .or_else(|| {
            state
                .last_seen_upgrade
                .map(|upgrade| ZedTopic::from_slug(upgrade.slug()))
        })
        .or_else(|| state.last_zed_topic.as_deref().map(ZedTopic::from_slug));
    let (topic, summary) = if let Some(topic) = pending_or_seen_topic {
        (topic, state.upgrade_summary())
    } else {
        let topic = if state.stage() == Stage::Juvenile
            && state.loop_runtime_submissions == 0
            && state.turns_observed >= 3
            && state.loop_admin_churn == 0
        {
            ZedTopic::LoopOnboarding
        } else if state.loop_runtime_blocks >= 2
            || (state.loop_admin_churn >= 3 && state.loop_runtime_submissions == 0)
        {
            ZedTopic::LoopRhythm
        } else if state.stage() == Stage::Adult
            && (state.loop_runtime_submissions > 0 || state.loop_exposure > 0)
        {
            ZedTopic::LoopAssistReady
        } else {
            ZedTopic::Growth
        };
        (topic, zed_summary_for_topic(topic))
    };
    let panel = zed_panel_data(topic, &summary);
    VivlingPanelData {
        title: panel.title,
        narrow_lines: panel.narrow_lines,
        wide_lines: panel.wide_lines,
    }
}

fn resolve_input_path(cwd: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn ensure_extension(path: PathBuf, ext: &str) -> PathBuf {
    if path.extension().and_then(|value| value.to_str()) == Some(ext) {
        path
    } else {
        path.with_extension(ext)
    }
}

fn read_zip_json<T: serde::de::DeserializeOwned>(
    archive: &mut ZipArchive<fs::File>,
    name: &str,
) -> io::Result<T> {
    let mut file = archive.by_name(name).map_err(io::Error::other)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    serde_json::from_str(&buf).map_err(io::Error::other)
}

fn roman_numeral(n: usize) -> String {
    match n {
        1 => "I".to_string(),
        2 => "II".to_string(),
        3 => "III".to_string(),
        4 => "IV".to_string(),
        5 => "V".to_string(),
        6 => "VI".to_string(),
        7 => "VII".to_string(),
        8 => "VIII".to_string(),
        9 => "IX".to_string(),
        10 => "X".to_string(),
        _ => n.to_string(),
    }
}
