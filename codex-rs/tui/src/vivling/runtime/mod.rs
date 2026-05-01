pub(crate) mod action;
pub(crate) mod brain;
pub(crate) mod brain_context;
pub(crate) mod command;
pub(crate) mod crt_insight;
pub(crate) mod live_context;
pub(crate) mod panel;
pub(crate) mod path_utils;
pub(crate) mod proactive;
pub(crate) mod render;
pub(crate) mod request;
pub(crate) mod roster;
#[cfg(test)]
mod tests;

pub(crate) use action::VivlingAction;
pub(crate) use live_context::VivlingLiveContext;
pub(crate) use live_context::VivlingLiveStatusItem;
pub(crate) use panel::VivlingPanelData;
pub(crate) use panel::render_upgrade_card;
pub(crate) use panel::render_vivling_card;
pub(crate) use path_utils::ensure_extension;
pub(crate) use path_utils::read_zip_json;
pub(crate) use path_utils::resolve_input_path;
pub(crate) use path_utils::roman_numeral;
pub(crate) use request::*;
pub(crate) use roster::VivlingPackageManifest;

// Re-export model types and registry functions for sub-modules
pub(crate) use super::model::JUVENILE_LEVEL;
pub(crate) use super::model::MAX_CARD_REPLY_LEN;
pub(crate) use super::model::SeedIdentity;
pub(crate) use super::model::Stage;
pub(crate) use super::model::VivlingAiMode;
pub(crate) use super::model::VivlingLoopEvent;
pub(crate) use super::model::VivlingState;
pub(crate) use super::model::hatch_species_from_unlocked;
pub(crate) use super::model::truncate_summary;
pub(crate) use super::registry::active_footer_sprites_for_species;
pub(crate) use super::registry::card_art_for_species;
pub(crate) use super::registry::species_for_id;
pub(crate) use super::zed::ZedTopic;
pub(crate) use super::zed::zed_panel_data;
pub(crate) use super::zed::zed_summary_for_topic;
pub(crate) use crate::render::renderable::Renderable;
pub(crate) use crate::tui::FrameRequester;
pub(crate) use chrono::DateTime;
pub(crate) use chrono::Utc;
pub(crate) use codex_config::CONFIG_TOML_FILE;
pub(crate) use codex_config::config_toml::ConfigToml;
pub(crate) use codex_config::types::AuthCredentialsStoreMode;
pub(crate) use codex_login::load_auth_dot_json;
pub(crate) use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
pub(crate) use ratatui::buffer::Buffer;
pub(crate) use ratatui::layout::Rect;
pub(crate) use std::fs;
pub(crate) use std::io;
pub(crate) use std::io::Write;
pub(crate) use std::path::Path;
pub(crate) use std::path::PathBuf;
pub(crate) use std::time::Duration;
pub(crate) use std::time::Instant;
pub(crate) use std::{cell::Cell, cell::RefCell};
pub(crate) use uuid::Uuid;
pub(crate) use zip::CompressionMethod;
pub(crate) use zip::ZipArchive;
pub(crate) use zip::ZipWriter;
pub(crate) use zip::write::SimpleFileOptions;

pub(crate) const STATE_FILE: &str = "vivling.json";
pub(crate) const ROSTER_DIR: &str = "vivlings";
pub(crate) const ROSTER_FILE: &str = "roster.json";
pub(crate) const EXPORT_DIR: &str = "exports";
pub(crate) const VIVPKG_VERSION: u32 = 1;
pub(crate) const VIVEGG_EXT: &str = "vivegg";
pub(crate) const EXTERNAL_SLOT_LIMIT: usize = 3;
pub(crate) const ACTIVE_FOOTER_FRAME_INTERVAL: Duration = Duration::from_millis(140);
pub(crate) const ACTIVE_FOOTER_TAIL: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub(crate) struct Vivling {
    pub(crate) codex_home: Option<PathBuf>,
    pub(crate) auth_mode: AuthCredentialsStoreMode,
    pub(crate) state: Option<VivlingState>,
    pub(crate) active_vivling_id: Option<String>,
    pub(crate) frame_requester: Option<FrameRequester>,
    pub(crate) animations_enabled: bool,
    pub(crate) task_running: Cell<bool>,
    pub(crate) active_until: Cell<Option<Instant>>,
    pub(crate) active_started_at: Cell<Option<Instant>>,
    pub(crate) next_scheduled_frame_at: RefCell<Option<Instant>>,
    /// Short lifecycle text set by lifecycle tick. Baby CRT scripts prefer visual scenes.
    pub(crate) animation_text: RefCell<Option<String>>,
    pub(crate) activity: RefCell<Option<crate::vl::VivlingActivity>>,
    pub(crate) live_context: RefCell<Option<VivlingLiveContext>>,
}
