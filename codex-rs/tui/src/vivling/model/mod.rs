pub(crate) mod constants;
pub(crate) mod state_chat;
pub(crate) mod state_init;
pub(crate) mod state_memory;
pub(crate) mod state_unlock;
pub(crate) mod state_xp;
pub(crate) mod text_utils;
pub(crate) mod types;

pub(crate) use constants::*;
pub(crate) use text_utils::classify_work_archetype;
pub(crate) use text_utils::contains_any;
pub(crate) use text_utils::truncate_summary;
pub(crate) use types::*;

#[cfg(test)]
pub(crate) use super::registry::hatch_species;
pub(crate) use super::registry::hatch_species_from_unlocked;
pub(crate) use super::registry::species_for_id;
pub(crate) use super::zed::zed_summary_for_stage;
pub(crate) use super::zed::zed_summary_for_upgrade;

use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct VivlingState {
    #[serde(default)]
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) hatched: bool,
    #[serde(default)]
    pub(crate) visible: bool,
    #[serde(default)]
    pub(crate) seed_hash: String,
    #[serde(default)]
    pub(crate) vivling_id: String,
    #[serde(default)]
    pub(crate) install_id: Option<String>,
    #[serde(default)]
    pub(crate) origin_install_id: Option<String>,
    #[serde(default)]
    pub(crate) species: String,
    #[serde(default)]
    pub(crate) rarity: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) primary_vivling_id: String,
    #[serde(default)]
    pub(crate) parent_vivling_id: Option<String>,
    #[serde(default)]
    pub(crate) spawn_generation: u64,
    #[serde(default)]
    pub(crate) is_primary: bool,
    #[serde(default)]
    pub(crate) is_imported: bool,
    #[serde(default)]
    pub(crate) imported_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub(crate) import_source: Option<String>,
    #[serde(default)]
    pub(crate) export_count: u64,
    #[serde(default)]
    pub(crate) instance_label: Option<String>,
    #[serde(default)]
    pub(crate) created_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub(crate) last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub(crate) last_fed_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub(crate) xp: u64,
    #[serde(default)]
    pub(crate) level: u64,
    #[serde(default)]
    pub(crate) hunger: i64,
    #[serde(default)]
    pub(crate) energy: i64,
    #[serde(default)]
    pub(crate) happiness: i64,
    #[serde(default)]
    pub(crate) social: i64,
    #[serde(default)]
    pub(crate) meals: u64,
    #[serde(default)]
    pub(crate) pets: u64,
    #[serde(default)]
    pub(crate) plays: u64,
    #[serde(default)]
    pub(crate) sleeps: u64,
    #[serde(default)]
    pub(crate) observations: u64,
    #[serde(default)]
    pub(crate) ai_mode: VivlingAiMode,
    #[serde(default)]
    pub(crate) brain_enabled: bool,
    #[serde(default)]
    pub(crate) brain_profile: Option<String>,
    #[serde(default)]
    pub(crate) brain_last_error: Option<String>,
    #[serde(default)]
    pub(crate) brain_last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub(crate) seed_origin: Option<String>,
    #[serde(default)]
    pub(crate) adult_bootstrap: bool,
    #[serde(default)]
    pub(crate) work_xp: u64,
    #[serde(default)]
    pub(crate) loop_exposure: u64,
    #[serde(default)]
    pub(crate) loop_runtime_submissions: u64,
    #[serde(default)]
    pub(crate) loop_runtime_blocks: u64,
    #[serde(default)]
    pub(crate) loop_admin_churn: u64,
    #[serde(default)]
    pub(crate) loop_blocked_review: u64,
    #[serde(default)]
    pub(crate) loop_blocked_side: u64,
    #[serde(default)]
    pub(crate) loop_blocked_busy: u64,
    #[serde(default)]
    pub(crate) turns_observed: u64,
    #[serde(default)]
    pub(crate) suggestions_made: u64,
    #[serde(default)]
    pub(crate) active_work_days: u64,
    #[serde(default)]
    pub(crate) last_active_work_day: Option<String>,
    #[serde(default)]
    pub(crate) last_work_xp_day: Option<String>,
    #[serde(default)]
    pub(crate) daily_work_xp: u64,
    #[serde(default)]
    pub(crate) chat_unlocked_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub(crate) active_mode_unlocked_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub(crate) last_work_summary: Option<String>,
    #[serde(default)]
    pub(crate) last_live_context_summary: Option<String>,
    #[serde(default)]
    pub(crate) work_affinities: WorkAffinitySet,
    #[serde(default)]
    pub(crate) work_memory: Vec<VivlingWorkMemoryEntry>,
    #[serde(default)]
    pub(crate) distilled_summaries: Vec<VivlingDistilledSummary>,
    #[serde(default)]
    pub(crate) mental_paths: Vec<VivlingMentalPath>,
    #[serde(default)]
    pub(crate) identity_profile: VivlingIdentityProfile,
    #[serde(default)]
    pub(crate) loop_profile: VivlingLoopProfile,
    #[serde(default)]
    pub(crate) capsules_since_distill: u64,
    #[serde(default)]
    pub(crate) last_message: Option<String>,
    #[serde(default)]
    pub(crate) pending_upgrade: Option<VivlingUpgrade>,
    #[serde(default)]
    pub(crate) last_seen_upgrade: Option<VivlingUpgrade>,
    #[serde(default)]
    pub(crate) last_zed_topic: Option<String>,
    #[serde(default)]
    pub(crate) unlocked_species: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct SeedIdentity {
    pub(crate) value: String,
    pub(crate) install_id: Option<String>,
}
