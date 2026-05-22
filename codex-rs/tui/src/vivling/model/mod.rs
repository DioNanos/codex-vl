pub(crate) mod constants;
pub(crate) mod gene;
pub(crate) mod lineage;
pub(crate) mod state_chat;
pub(crate) mod state_init;
pub(crate) mod state_llm;
pub(crate) mod state_memory;
pub(crate) mod state_unlock;
pub(crate) mod state_xp;
pub(crate) mod text_utils;
pub(crate) mod types;

pub(crate) use constants::*;
pub(crate) use gene::*;
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
    pub(crate) gene_vector: VivlingGeneVector,
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
    #[serde(default)]
    pub(crate) bond: crate::vivling::VivlingBond,
    /// codex-vl lineage passive learning: dedup keys for parent distilled
    /// summaries already absorbed by this child. FIFO eviction at 64 entries.
    #[serde(default)]
    pub(crate) lineage_seen_parent_summary_keys: Vec<String>,
    /// codex-vl lineage rarity pressure: +2 per successful offspring spawn,
    /// cap 10. Applied **inside species** as a quality roll bias on
    /// `gene_vector` and `brain_potential` of the next offspring; never
    /// swaps species (DAG design directive 2026-05-15).
    #[serde(default)]
    pub(crate) lineage_rarity_pressure_pct: u8,
    /// codex-vl cultural parent: the `vivling_id` of the active primary at
    /// the time this Vivling was hatched/spawned. Drives lineage passive
    /// learning regardless of the biological parent. `None` for legacy
    /// states from before the multi-origin spawn; propagation falls back
    /// to `parent_vivling_id` for those entries.
    #[serde(default)]
    pub(crate) cultural_parent_vivling_id: Option<String>,
    /// codex-vl lineage blessed flag: set on a successful quality roll
    /// when `lineage_rarity_pressure_pct` met the blessed threshold.
    /// Cosmetic / audit signal — never affects active state, brain or
    /// loop ownership.
    #[serde(default)]
    pub(crate) lineage_blessed: bool,

    // --- Memory V2 Step 2.A schema fields (storage only, no runtime
    // logic yet — wiring lands in later steps). All carry serde defaults
    // so V8 JSON keeps loading unchanged.
    /// Axis A: self-authored voice paragraph written by the memory
    /// agent. None until the agent runs the first time.
    #[serde(default)]
    pub(crate) self_voice: Option<codex_vivling_core::model::VivlingVoice>,
    /// Axis G: detected/override language plus a small rolling sample
    /// window. Default `MirrorUser` matches the design (§8.2).
    #[serde(default)]
    pub(crate) language_state: codex_vivling_core::model::VivlingLanguageState,
    /// Axis D extended: cultural inheritance seed populated at spawn
    /// time. None for legacy/V8 Vivlings.
    #[serde(default)]
    pub(crate) lineage_inheritance: Option<codex_vivling_core::model::LineageInheritance>,
    /// Fix §10.2: monotonic-from-hatch counters. Coexist with the
    /// existing `identity_profile.{caution,verification,question}_bias`
    /// until later steps migrate the rebuild logic.
    #[serde(default)]
    pub(crate) accumulated_bias: codex_vivling_core::model::BiasCounters,
    /// Fix §10.2: sliding-window counters refreshed by the memory
    /// agent. Same coexistence note as `accumulated_bias`.
    #[serde(default)]
    pub(crate) recent_bias: codex_vivling_core::model::BiasCounters,
    /// Axis F: cached CRT footer phrase. Volatile, regenerated by the
    /// lightweight LLM; `#[serde(skip)]` so it never lands in
    /// `<id>.json`. Reads land in a later step.
    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) cached_crt_phrase: Option<codex_vivling_core::model::CachedCrtPhrase>,
    /// Axis F: cached proactive message. Same volatility contract as
    /// `cached_crt_phrase`. Reads land in a later step.
    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) cached_proactive: Option<codex_vivling_core::model::CachedProactive>,

    // --- Memory V2 Step 12.B.A: V10 scaffolding ---
    //
    // Tri-state opt-in for the CRT/proactive expression channel.
    // Decoupled from `brain_enabled`: `brain_enabled` gates assist /
    // loop-tick LLM, this gates the always-on expression surface.
    // Default = stage-driven (Adult/Juvenile run, Baby on rare events).
    /// Memory V2 Step 12.B.A — expression LLM opt-in mode.
    #[serde(default)]
    pub(crate) crt_brain_mode: codex_vivling_core::model::VivlingExpressionMode,

    /// Memory V2 Step 12.B.O — per-Vivling budget cap override.
    /// `Default` falls back to `stage_llm_budget(stage)`; DAG can
    /// switch to `Unlimited` or `Custom(n)` via `/vivling crt-brain
    /// budget`. Additive on V10 with `#[serde(default)]`.
    #[serde(default)]
    pub(crate) budget_override: codex_vivling_core::model::VivlingBudgetCap,

    /// Memory V2 Step 12.B.P — one-shot Ctrl+J discoverability flag.
    /// Set to `true` the first time the user has accumulated enough
    /// `/vl` chat turns without ever opening the dedicated panel —
    /// the chatwidget emits a single inline hint and never repeats.
    /// Additive on V10 with `#[serde(default)]`.
    #[serde(default)]
    pub(crate) chat_hint_shown: bool,

    // Step 12.B.A — daily LLM call budget counters. Reservation
    // happens in main-thread `try_reserve_llm_call` (Step 12.B.B); the
    // increments persist here so a crash/restart cannot let the
    // Vivling spend past its daily cap. `day_key` is `YYYY-MM-DD` in
    // UTC; the counters reset the first time a reservation is
    // attempted on a different day.
    /// Step 12.B.A — total LLM calls reserved today (Chat + Assist +
    /// LoopTick + Expression). Increment is unconditional once the
    /// reservation passes guards; failure counts via
    /// `daily_llm_failure_count`.
    #[serde(default)]
    pub(crate) daily_llm_call_count: u32,
    /// Step 12.B.A — `Chat` kind breakdown.
    #[serde(default)]
    pub(crate) daily_llm_chat_calls: u32,
    /// Step 12.B.A — `Assist` kind breakdown.
    #[serde(default)]
    pub(crate) daily_llm_assist_calls: u32,
    /// Step 12.B.A — `LoopTick` kind breakdown. Tracked separately so
    /// loop owners' LLM consumption stays observable.
    #[serde(default)]
    pub(crate) daily_llm_loop_tick_calls: u32,
    /// Step 12.B.A — `Expression` kind breakdown.
    #[serde(default)]
    pub(crate) daily_llm_expression_calls: u32,
    /// Step 12.B.A — count of reservations where the post-dispatch
    /// result returned an error / invalid JSON / empty payload.
    /// Does not subtract from `daily_llm_call_count` (reservation
    /// already paid for the slot).
    #[serde(default)]
    pub(crate) daily_llm_failure_count: u32,
    /// Step 12.B.A — reservation rejected because we are within the
    /// 60s throttle window from `last_llm_dispatch_at`.
    #[serde(default)]
    pub(crate) daily_llm_throttle_skips: u32,
    /// Step 12.B.A — reservation rejected because the planner's
    /// prompt hash matches the cached entry's hash (no new signal).
    #[serde(default)]
    pub(crate) daily_llm_dedup_skips: u32,
    /// Step 12.B.A — reservation rejected because the daily budget
    /// for this Vivling's stage was already reached.
    #[serde(default)]
    pub(crate) daily_llm_budget_skips: u32,
    /// Step 12.B.A — reservation rejected because the Vivling's
    /// `crt_brain_mode` is `Off` (only counted for `Expression` kind;
    /// `/vl` chat is governed by stage policy, not by `crt_brain_mode`).
    #[serde(default)]
    pub(crate) daily_llm_optout_skips: u32,
    /// Step 12.B.A — UTC day boundary key (`YYYY-MM-DD`). Counters
    /// reset the first time `try_reserve_llm_call` runs on a fresh
    /// day. Empty string at V9→V10 migration; gets populated on the
    /// next reservation attempt.
    #[serde(default)]
    pub(crate) daily_llm_day_key: String,
    /// Step 12.B.A — timestamp of the last successful reservation,
    /// used by the 60s throttle. None at migration time.
    #[serde(default)]
    pub(crate) last_llm_dispatch_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Memory V2 Step 12.B.D.4 — timestamp of the last successful
    /// loop-triggered Expression dispatch. Independent from
    /// `last_llm_dispatch_at` so the dedicated 5-minute anti-burn
    /// floor only applies to loop-event hooks; turn-driven refresh
    /// keeps the standard 60s window. Additive `#[serde(default)]`,
    /// NO schema bump (still V10).
    #[serde(default)]
    pub(crate) last_loop_expression_dispatch_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone)]
pub(crate) struct SeedIdentity {
    pub(crate) value: String,
    pub(crate) install_id: Option<String>,
}
