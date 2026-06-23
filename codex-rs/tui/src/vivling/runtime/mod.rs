pub(crate) mod action;
pub(crate) mod brain;
pub(crate) mod brain_context;
pub(crate) mod command;
pub(crate) mod crt_insight;
pub(crate) mod expression;
pub(crate) mod lifecycle;
pub(crate) mod lineage_echo;
pub(crate) mod live_context;
pub(crate) mod msa;
pub(crate) mod panel;
pub(crate) mod path_utils;
pub(crate) mod proactive;
pub(crate) mod render;
pub(crate) mod request;
pub(crate) mod roster;
pub(crate) mod spawn_origin;
#[cfg(test)]
mod tests;

pub(crate) use action::VivlingAction;
pub(crate) use expression::VivlingExpressionRequest;
pub(crate) use expression::VivlingExpressionResult;
pub(crate) use lifecycle::{ExpressionKind, VivlingLifecyclePhase};
pub(crate) use live_context::VivlingLiveContext;
pub(crate) use msa::VivlingMsa;
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
pub(crate) use super::model::VERSION;
pub(crate) use super::model::VivlingAiMode;
pub(crate) use super::model::VivlingLoopEvent;
pub(crate) use super::model::VivlingState;
pub(crate) use super::model::hatch_species_from_unlocked;
pub(crate) use super::model::modulated_totals;
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
pub(crate) use codex_vivling_core::paths::last_write_backup_path;
pub(crate) use codex_vivling_core::paths::lock_file_path;
pub(crate) use codex_vivling_core::paths::pre_migration_backup_path;
pub(crate) use codex_vivling_core::safety::SafetyError;
pub(crate) use codex_vivling_core::safety::VivlingLockGuard;
pub(crate) use codex_vivling_core::safety::acquire_lock;
pub(crate) use codex_vivling_core::safety::backup_last_write;
pub(crate) use codex_vivling_core::safety::backup_pre_migration;
pub(crate) use codex_vivling_core::safety::write_atomic;
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
pub(crate) const ANIMATION_TEXT_TTL: Duration = Duration::from_secs(4);

#[derive(Debug)]
pub(crate) struct Vivling {
    pub(crate) codex_home: Option<PathBuf>,
    pub(crate) auth_mode: AuthCredentialsStoreMode,
    pub(crate) state: Option<VivlingState>,
    pub(crate) active_vivling_id: Option<String>,
    pub(crate) frame_requester: Option<FrameRequester>,
    pub(crate) animations_enabled: bool,
    /// Step 12.C — fase di dispatch. Sorgente di verità per il task-running
    /// (il flag legacy `task_running` è stato rimosso in questo step).
    pub(crate) lifecycle: RefCell<VivlingLifecyclePhase>,
    /// Step 12.C — gate ortogonale: un solo dispatch di espressione in volo
    /// (race-safety per 12.D). NON è una fase: può coesistere con TaskRunning.
    pub(crate) expression_in_flight: Cell<Option<ExpressionKind>>,
    pub(crate) active_until: Cell<Option<Instant>>,
    pub(crate) active_started_at: Cell<Option<Instant>>,
    pub(crate) next_scheduled_frame_at: RefCell<Option<Instant>>,
    /// Short lifecycle text set by lifecycle tick. Baby CRT scripts prefer visual scenes.
    pub(crate) animation_text: RefCell<Option<String>>,
    pub(crate) animation_text_expires_at: Cell<Option<Instant>>,
    pub(crate) activity: RefCell<Option<crate::vl::VivlingActivity>>,
    pub(crate) live_context: RefCell<Option<VivlingLiveContext>>,
    pub(crate) msa: Option<std::sync::Arc<VivlingMsa>>,
    /// Resolved CRT effect toggles. Re-read from `<codex_home>/config.toml`
    /// when `configure()` is called with a new home.
    pub(crate) crt_config: crate::vl::crt::VivlingCrtConfig,
    /// Per-render transition snapshot generator. Mutated inside `render()`.
    pub(crate) crt_animation_ledger: crate::vl::crt::CrtAnimationLedger,
    /// Frame pacing target detected from the runtime environment.
    pub(crate) crt_frame_target: Cell<crate::vl::crt::FrameTarget>,
    /// Memory V2 Step 12.B.L — runtime-only flag set the first time a
    /// boot/load completes for this Vivling instance. Prevents `ensure_
    /// startup_dispatched()` from re-firing on subsequent `configure()`
    /// calls within the same session (e.g. `codex_home` toggles). Reset
    /// implicitly on process restart because the wrapper is rebuilt
    /// (see `unavailable()` and `Clone`).
    pub(crate) startup_dispatched: Cell<bool>,
    /// codex-vl Step 14 Bug 1 fix — runtime gate that hides
    /// state-persistent CRT fallbacks (`proactive_next_phrase_at`,
    /// `recent_memory_phrase`, `last_work_summary_phrase`) until the
    /// first Expression dispatch of this TUI session has resolved.
    /// Without it, the CRT footer would render the *previous* session's
    /// `last_work_summary` text for the few seconds it takes the
    /// bootstrap LLM dispatch to populate `cached_crt_phrase`, which
    /// reads as "the Vivling is showing yesterday's chat". Flipped to
    /// `true` from `record_expression_result_for` (success) and
    /// `record_expression_failure_for` (so a stalled / failed dispatch
    /// does not freeze the CRT into safety-template-only mode forever).
    /// Reset implicitly on process restart because the wrapper is
    /// rebuilt (see `unavailable()` and `Clone`).
    pub(crate) crt_first_dispatch_completed: Cell<bool>,
    /// Memory V2 Step 12.B.P — runtime-only counter of `/vl` chat
    /// turns observed in this session. Drives the one-shot Ctrl+J
    /// hint surfaced via `chat_widget.add_info_message` after a few
    /// turns when the user has never opened the dedicated panel.
    /// Reset implicitly on process restart.
    pub(crate) session_chat_turns: Cell<u32>,
}

impl Clone for Vivling {
    /// Custom clone: `crt_animation_ledger` is intentionally reset (not
    /// shared) so that derived clones do not carry transition state from
    /// another renderer. All other fields clone through their defaults.
    fn clone(&self) -> Self {
        Self {
            codex_home: self.codex_home.clone(),
            auth_mode: self.auth_mode,
            state: self.state.clone(),
            active_vivling_id: self.active_vivling_id.clone(),
            frame_requester: self.frame_requester.clone(),
            animations_enabled: self.animations_enabled,
            lifecycle: RefCell::new(self.lifecycle.borrow().clone()),
            expression_in_flight: self.expression_in_flight.clone(),
            active_until: self.active_until.clone(),
            active_started_at: self.active_started_at.clone(),
            next_scheduled_frame_at: self.next_scheduled_frame_at.clone(),
            animation_text: self.animation_text.clone(),
            animation_text_expires_at: self.animation_text_expires_at.clone(),
            activity: self.activity.clone(),
            live_context: self.live_context.clone(),
            msa: self.msa.clone(),
            crt_config: self.crt_config.clone(),
            crt_animation_ledger: crate::vl::crt::CrtAnimationLedger::new(),
            crt_frame_target: self.crt_frame_target.clone(),
            startup_dispatched: self.startup_dispatched.clone(),
            crt_first_dispatch_completed: self.crt_first_dispatch_completed.clone(),
            session_chat_turns: self.session_chat_turns.clone(),
        }
    }
}
