use chrono::DateTime;
use chrono::Duration;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;

use super::registry::hatch_species;
use super::registry::species_for_id;
use super::zed::zed_summary_for_stage;
use super::zed::zed_summary_for_upgrade;

pub(crate) const VERSION: u32 = 6;
pub(crate) const MAX_LEVEL: u64 = 99;
pub(crate) const JUVENILE_LEVEL: u64 = 30;
pub(crate) const ADULT_LEVEL: u64 = 60;
pub(crate) const SPAWN_SLOT_LEVEL_STEP: u64 = 30;
pub(crate) const JUVENILE_ACTIVE_DAYS: u64 = 30;
pub(crate) const ADULT_ACTIVE_DAYS: u64 = 90;
pub(crate) const WORK_XP_PER_LEVEL: u64 = 60;
pub(crate) const DAILY_WORK_XP_CAP: u64 = 60;
pub(crate) const MAX_WORK_MEMORY_ENTRIES: usize = 64;
pub(crate) const MAX_DISTILLED_MEMORY_ENTRIES: usize = 24;
pub(crate) const MAX_MENTAL_PATH_ENTRIES: usize = 256;
pub(crate) const DISTILL_TRIGGER_CAPSULES: u64 = 8;
pub(crate) const MAX_DIRECT_REPLY_LEN: usize = 120;
pub(crate) const MAX_CARD_REPLY_LEN: usize = 280;
const ADULT_SEED_ORIGIN: &str = "adult_seed_v1";
const EARLY_SEED_ORIGIN: &str = "early_seed_v1";

const NAMES: &[&str] = &[
    "Nilo", "Kira", "Moro", "Luma", "Pax", "Rin", "Taro", "Vera", "Sumi", "Nox", "Iko", "Mina",
    "Zed", "Ari", "Tika", "Juno",
];

const BABY_GREETINGS: &[&str] = &[
    "is tiny and still learning your rhythm",
    "is blinking at the work around it",
    "is small, curious, and very present",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VivlingAiMode {
    Off,
    #[serde(alias = "suggest")]
    #[default]
    On,
}

impl VivlingAiMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "on" | "assistant" => Some(Self::On),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VivlingUpgrade {
    YoungVoice,
    ActiveMode,
}

impl VivlingUpgrade {
    pub(crate) fn slug(self) -> &'static str {
        match self {
            Self::YoungVoice => "young-voice",
            Self::ActiveMode => "active-mode",
        }
    }

    pub(crate) fn prompt(self) -> &'static str {
        match self {
            Self::YoungVoice => "/vivling upgrade for young voice",
            Self::ActiveMode => "/vivling mode on",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkArchetype {
    #[default]
    Builder,
    Reviewer,
    Researcher,
    Operator,
}

impl WorkArchetype {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Builder => "builder",
            Self::Reviewer => "reviewer",
            Self::Researcher => "researcher",
            Self::Operator => "operator",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct WorkAffinitySet {
    #[serde(default)]
    pub(crate) builder: u64,
    #[serde(default)]
    pub(crate) reviewer: u64,
    #[serde(default)]
    pub(crate) researcher: u64,
    #[serde(default)]
    pub(crate) operator: u64,
}

impl WorkAffinitySet {
    pub(crate) fn add(&mut self, archetype: WorkArchetype, weight: u64) {
        match archetype {
            WorkArchetype::Builder => self.builder = self.builder.saturating_add(weight),
            WorkArchetype::Reviewer => self.reviewer = self.reviewer.saturating_add(weight),
            WorkArchetype::Researcher => {
                self.researcher = self.researcher.saturating_add(weight);
            }
            WorkArchetype::Operator => self.operator = self.operator.saturating_add(weight),
        }
    }

    pub(crate) fn totals_with_bias(&self, bias: &WorkAffinitySet) -> [(WorkArchetype, u64); 4] {
        [
            (
                WorkArchetype::Builder,
                self.builder.saturating_add(bias.builder),
            ),
            (
                WorkArchetype::Reviewer,
                self.reviewer.saturating_add(bias.reviewer),
            ),
            (
                WorkArchetype::Researcher,
                self.researcher.saturating_add(bias.researcher),
            ),
            (
                WorkArchetype::Operator,
                self.operator.saturating_add(bias.operator),
            ),
        ]
    }

    pub(crate) fn dominant_with_bias(&self, bias: &WorkAffinitySet) -> WorkArchetype {
        self.totals_with_bias(bias)
            .into_iter()
            .max_by_key(|(_, value)| *value)
            .map(|(kind, _)| kind)
            .unwrap_or(WorkArchetype::Builder)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    Baby,
    Juvenile,
    Adult,
}

impl Stage {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Baby => "baby",
            Self::Juvenile => "juvenile",
            Self::Adult => "adult",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct VivlingWorkMemoryEntry {
    pub(crate) kind: String,
    pub(crate) summary: String,
    #[serde(default)]
    pub(crate) archetype: WorkArchetype,
    #[serde(default)]
    pub(crate) weight: u64,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct VivlingDistilledSummary {
    pub(crate) topic: String,
    pub(crate) summary: String,
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) archetype: WorkArchetype,
    #[serde(default)]
    pub(crate) total_weight: u64,
    #[serde(default)]
    pub(crate) observations: u64,
    pub(crate) first_seen_at: DateTime<Utc>,
    pub(crate) last_seen_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct VivlingMentalPath {
    pub(crate) from: String,
    pub(crate) to: String,
    #[serde(default)]
    pub(crate) weight: u64,
    pub(crate) last_seen_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct VivlingIdentityProfile {
    #[serde(default)]
    pub(crate) tone: String,
    #[serde(default)]
    pub(crate) dominant_focus: WorkArchetype,
    #[serde(default)]
    pub(crate) question_bias: u64,
    #[serde(default)]
    pub(crate) caution_bias: u64,
    #[serde(default)]
    pub(crate) verification_bias: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct VivlingLoopProfile {
    #[serde(default)]
    pub(crate) clean_submissions: u64,
    #[serde(default)]
    pub(crate) noisy_churn: u64,
    #[serde(default)]
    pub(crate) blocked_runs: u64,
    #[serde(default)]
    pub(crate) milestone_signals: u64,
    #[serde(default)]
    pub(crate) partial_signals: u64,
    #[serde(default)]
    pub(crate) verification_signals: u64,
    #[serde(default)]
    pub(crate) wait_signals: u64,
}

#[derive(Clone, Debug)]
pub(crate) enum VivlingLoopEventKind {
    Config,
    Runtime,
}

#[derive(Clone, Debug)]
pub(crate) enum VivlingLoopEventSource {
    User,
    Agent,
}

#[derive(Clone, Debug)]
pub(crate) struct VivlingLoopEvent {
    pub(crate) kind: VivlingLoopEventKind,
    pub(crate) source: VivlingLoopEventSource,
    pub(crate) action: String,
    pub(crate) label: String,
    pub(crate) runtime_state: Option<String>,
    pub(crate) last_status: Option<String>,
    pub(crate) goal: Option<String>,
}

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
    pub(crate) imported_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub(crate) import_source: Option<String>,
    #[serde(default)]
    pub(crate) export_count: u64,
    #[serde(default)]
    pub(crate) instance_label: Option<String>,
    #[serde(default)]
    pub(crate) created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub(crate) last_seen_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub(crate) last_fed_at: Option<DateTime<Utc>>,
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
    pub(crate) brain_last_used_at: Option<DateTime<Utc>>,
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
    pub(crate) chat_unlocked_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub(crate) active_mode_unlocked_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub(crate) last_work_summary: Option<String>,
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
}

#[derive(Clone)]
pub(crate) struct SeedIdentity {
    pub(crate) value: String,
    pub(crate) install_id: Option<String>,
}

impl VivlingState {
    pub(crate) fn new(seed: SeedIdentity) -> Self {
        let hash = fnv1a64(seed.value.as_bytes());
        let species = hatch_species(hash);
        let now = Utc::now();
        let vivling_id = seed
            .install_id
            .clone()
            .unwrap_or_else(|| format!("viv-{:08x}", hash as u32));
        let mut state = Self {
            version: VERSION,
            hatched: true,
            visible: true,
            seed_hash: format!("{hash:016x}"),
            vivling_id: vivling_id.clone(),
            install_id: seed.install_id.clone(),
            origin_install_id: seed.install_id,
            species: species.id.clone(),
            rarity: species.rarity.label().to_string(),
            name: NAMES[((hash >> 8) as usize) % NAMES.len()].to_string(),
            primary_vivling_id: vivling_id,
            parent_vivling_id: None,
            spawn_generation: 0,
            is_primary: true,
            is_imported: false,
            imported_at: None,
            import_source: None,
            export_count: 0,
            instance_label: None,
            created_at: Some(now),
            last_seen_at: Some(now),
            last_fed_at: Some(now),
            xp: 0,
            level: 1,
            hunger: 82,
            energy: 76,
            happiness: 70,
            social: 62,
            meals: 0,
            pets: 0,
            plays: 0,
            sleeps: 0,
            observations: 0,
            ai_mode: VivlingAiMode::Off,
            brain_enabled: false,
            brain_profile: None,
            brain_last_error: None,
            brain_last_used_at: None,
            seed_origin: None,
            adult_bootstrap: false,
            work_xp: 0,
            loop_exposure: 0,
            loop_runtime_submissions: 0,
            loop_runtime_blocks: 0,
            loop_admin_churn: 0,
            loop_blocked_review: 0,
            loop_blocked_side: 0,
            loop_blocked_busy: 0,
            turns_observed: 0,
            suggestions_made: 0,
            active_work_days: 0,
            last_active_work_day: None,
            last_work_xp_day: None,
            daily_work_xp: 0,
            chat_unlocked_at: None,
            active_mode_unlocked_at: None,
            last_work_summary: None,
            work_affinities: WorkAffinitySet::default(),
            work_memory: Vec::new(),
            distilled_summaries: Vec::new(),
            mental_paths: Vec::new(),
            identity_profile: VivlingIdentityProfile::default(),
            loop_profile: VivlingLoopProfile::default(),
            capsules_since_distill: 0,
            last_message: Some(BABY_GREETINGS[(hash as usize) % BABY_GREETINGS.len()].to_string()),
            pending_upgrade: None,
            last_seen_upgrade: None,
            last_zed_topic: None,
        };
        state.recompute_level();
        state
    }

    pub(crate) fn apply_decay(&mut self, now: DateTime<Utc>) {
        let Some(last_seen) = self.last_seen_at else {
            self.last_seen_at = Some(now);
            return;
        };
        let elapsed = now.signed_duration_since(last_seen);
        if elapsed < Duration::hours(12) {
            self.last_seen_at = Some(now);
            return;
        }
        let days = elapsed.num_days().max(1);
        self.hunger = (self.hunger - days * 8).clamp(0, 100);
        self.energy = (self.energy - days * 3).clamp(0, 100);
        self.happiness = (self.happiness - days * 4).clamp(0, 100);
        self.social = (self.social - days * 5).clamp(0, 100);
        self.last_seen_at = Some(now);
    }

    pub(crate) fn normalize_loaded_state(&mut self) {
        let persisted_work_xp = self.work_xp;
        let persisted_active_work_days = self.active_work_days;
        self.version = VERSION;
        if self.vivling_id.trim().is_empty() {
            self.vivling_id = self
                .install_id
                .clone()
                .unwrap_or_else(|| format!("viv-{}", self.seed_hash));
        }
        if self.primary_vivling_id.trim().is_empty() {
            self.primary_vivling_id = self.vivling_id.clone();
        }
        if self.origin_install_id.is_none() {
            self.origin_install_id = self.install_id.clone();
        }
        if self.is_primary && self.parent_vivling_id.is_some() {
            self.parent_vivling_id = None;
        }
        if self.primary_vivling_id == self.vivling_id {
            self.is_primary = true;
        }
        if self.level == 0 {
            self.level = 1;
        }
        self.normalize_species();
        self.backfill_capsule_metadata();
        if !self.work_memory.is_empty() {
            self.recompute_progress_from_memory();
            self.work_xp = self.work_xp.max(persisted_work_xp);
            self.active_work_days = self.active_work_days.max(persisted_active_work_days);
            self.xp = self.work_xp;
        } else {
            self.work_xp = persisted_work_xp;
            self.active_work_days = persisted_active_work_days;
            self.xp = self.work_xp;
        }
        self.rebuild_learning_profiles();
        self.recompute_level();
        if self.brain_last_error.as_deref().is_some_and(str::is_empty) {
            self.brain_last_error = None;
        }
        if self.seed_origin.as_deref().is_some_and(str::is_empty) {
            self.seed_origin = None;
        }
        if self.stage() != Stage::Adult {
            self.brain_enabled = false;
        }
        if self.last_message.is_none() {
            self.last_message = Some("is watching the session".to_string());
        }
    }

    pub(crate) fn brain_summary(&self) -> String {
        let profile = self.brain_profile.as_deref().unwrap_or("none");
        let status = if self.brain_enabled { "on" } else { "off" };
        let last = self
            .brain_last_error
            .as_deref()
            .map(|err| format!(" - last_error {}", truncate_summary(err, 64)))
            .unwrap_or_default();
        format!("brain {status} - profile {profile}{last}")
    }

    pub(crate) fn set_brain_enabled(&mut self, enabled: bool) -> Result<String, String> {
        if enabled {
            if self.stage() != Stage::Adult {
                return Err("Vivling brain unlocks only at level 60.".to_string());
            }
            if self.brain_profile.is_none() {
                return Err(
                    "Set a Vivling brain profile first with `/vivling model ...`.".to_string(),
                );
            }
        }
        self.brain_enabled = enabled;
        self.brain_last_error = None;
        let message = format!(
            "{} brain {}.",
            self.name,
            if enabled { "enabled" } else { "disabled" }
        );
        self.last_message = Some(message.clone());
        Ok(message)
    }

    pub(crate) fn assign_brain_profile(&mut self, profile: String) -> String {
        self.brain_profile = Some(profile.clone());
        self.brain_last_error = None;
        let auto_enabled = self.stage() == Stage::Adult;
        if auto_enabled {
            self.brain_enabled = true;
        }
        let message = if auto_enabled {
            format!(
                "{} brain profile set to `{profile}` and brain enabled.",
                self.name
            )
        } else {
            format!("{} brain profile set to `{profile}`.", self.name)
        };
        self.last_message = Some(message.clone());
        message
    }

    pub(crate) fn mark_brain_runtime_error(&mut self, error: impl Into<String>) {
        let error = truncate_summary(&error.into(), 240);
        self.brain_last_error = Some(error.clone());
        self.last_message = Some(error);
    }

    pub(crate) fn mark_brain_reply(&mut self, reply: &str) {
        self.brain_last_error = None;
        self.brain_last_used_at = Some(Utc::now());
        self.last_message = Some(truncate_summary(reply, MAX_DIRECT_REPLY_LEN));
    }

    pub(crate) fn assist_prompt_context(&self, task: &str) -> Result<String, String> {
        if self.stage() != Stage::Adult {
            return Err("`/vivling assist ...` unlocks only at level 60.".to_string());
        }
        if !self.brain_enabled {
            return Err("Enable the Vivling brain first with `/vivling brain on`.".to_string());
        }
        let profile = self.brain_profile.as_deref().ok_or_else(|| {
            "Set a Vivling brain profile first with `/vivling model ...`.".to_string()
        })?;
        let task = task.trim();
        if task.is_empty() {
            return Err("Usage: /vivling assist <task>".to_string());
        }
        let last = self
            .last_work_summary
            .as_deref()
            .map(|summary| truncate_summary(summary, 96))
            .unwrap_or_else(|| "No recent work summary yet.".to_string());
        let live_state_contract = "Live state is unknown unless the task explicitly provides it. Treat learned memory as bias and history, not proof that the current system is blocked, idle, active, or complete.";
        Ok(format!(
            "Vivling identity:\n- id: {}\n- name: {}\n- profile: {}\n- stage: {}\n- dominant role: {}\n- tone: {}\n- verification bias: {}\n\nLearned memory:\n- recent summary: {}\n- memory digest:\n{}\n\nLive state contract:\n{}\n\nTask:\n{}",
            self.vivling_id,
            self.name,
            profile,
            self.stage().label(),
            self.dominant_archetype().label(),
            self.identity_profile.tone,
            self.identity_profile.verification_bias,
            last,
            self.memory_digest(),
            live_state_contract,
            task
        ))
    }

    pub(crate) fn chat_prompt_context(&self, text: &str) -> Result<String, String> {
        let profile = self.brain_profile.as_deref().ok_or_else(|| {
            "Set a Vivling brain profile first with `/vivling model ...`.".to_string()
        })?;
        let text = text.trim();
        if text.is_empty() {
            return Err("Usage: /vl <message>".to_string());
        }
        let last = self
            .last_work_summary
            .as_deref()
            .map(|summary| truncate_summary(summary, 96))
            .unwrap_or_else(|| "No recent work summary yet.".to_string());
        let live_state_contract = "Live state is unknown unless the user message explicitly provides it. Treat learned memory as bias and history, not proof that the current system is blocked, idle, active, or complete.";
        Ok(format!(
            "Vivling identity:\n- id: {}\n- name: {}\n- profile: {}\n- stage: {}\n- dominant role: {}\n- tone: {}\n- verification bias: {}\n\nLearned memory:\n- recent summary: {}\n- memory digest:\n{}\n\nLive state contract:\n{}\n\nUser message:\n{}",
            self.vivling_id,
            self.name,
            profile,
            self.stage().label(),
            self.dominant_archetype().label(),
            self.identity_profile.tone,
            self.identity_profile.verification_bias,
            last,
            self.memory_digest(),
            live_state_contract,
            text
        ))
    }

    pub(crate) fn promote_to_adult_seed(&mut self) -> String {
        let now = Utc::now();
        self.level = ADULT_LEVEL;
        self.hatched = true;
        self.visible = true;
        self.ai_mode = VivlingAiMode::Off;
        self.brain_enabled = false;
        self.brain_last_error = None;
        self.brain_last_used_at = None;
        self.seed_origin = Some(ADULT_SEED_ORIGIN.to_string());
        self.adult_bootstrap = true;
        self.work_xp = WORK_XP_PER_LEVEL.saturating_mul(ADULT_LEVEL.saturating_sub(1));
        self.xp = self.work_xp;
        self.active_work_days = ADULT_ACTIVE_DAYS.max(self.active_work_days);
        self.chat_unlocked_at.get_or_insert(now);
        self.active_mode_unlocked_at.get_or_insert(now);
        self.last_active_work_day = Some(now.date_naive().to_string());
        self.last_work_xp_day = Some(now.date_naive().to_string());
        self.daily_work_xp = DAILY_WORK_XP_CAP;
        self.last_work_summary = Some(
            "Closed real work in small verified moves and escalated only on true blockers."
                .to_string(),
        );
        self.work_affinities = WorkAffinitySet {
            builder: 180,
            reviewer: 220,
            researcher: 120,
            operator: 90,
        };
        self.identity_profile = VivlingIdentityProfile {
            tone: "concise, skeptical, operational".to_string(),
            dominant_focus: WorkArchetype::Reviewer,
            question_bias: 32,
            caution_bias: 44,
            verification_bias: 68,
        };
        self.loop_profile = VivlingLoopProfile {
            clean_submissions: 36,
            noisy_churn: 6,
            blocked_runs: 12,
            milestone_signals: 30,
            partial_signals: 14,
            verification_signals: 42,
            wait_signals: 28,
        };
        self.work_memory = vec![
            VivlingWorkMemoryEntry {
                kind: "build".to_string(),
                summary: "Shipped fixes in small slices and rechecked real runtime state before widening.".to_string(),
                archetype: WorkArchetype::Builder,
                weight: 18,
                created_at: now,
            },
            VivlingWorkMemoryEntry {
                kind: "review".to_string(),
                summary: "Flagged the real blocker first, then reduced the change until the risk moved.".to_string(),
                archetype: WorkArchetype::Reviewer,
                weight: 24,
                created_at: now,
            },
            VivlingWorkMemoryEntry {
                kind: "ops".to_string(),
                summary: "Kept loops calm: check state, act once, verify, wait.".to_string(),
                archetype: WorkArchetype::Operator,
                weight: 14,
                created_at: now,
            },
        ];
        self.distilled_summaries = vec![
            VivlingDistilledSummary {
                topic: "verification rhythm".to_string(),
                summary: "Check first, take one minimal action, verify outcome, and only then widen scope.".to_string(),
                kind: "ops".to_string(),
                archetype: WorkArchetype::Reviewer,
                total_weight: 42,
                observations: 8,
                first_seen_at: now,
                last_seen_at: now,
            },
            VivlingDistilledSummary {
                topic: "blocked escalation".to_string(),
                summary: "Escalate only when the blocker is real and proved, not just because work feels stuck.".to_string(),
                kind: "review".to_string(),
                archetype: WorkArchetype::Reviewer,
                total_weight: 36,
                observations: 6,
                first_seen_at: now,
                last_seen_at: now,
            },
        ];
        self.mental_paths = vec![
            VivlingMentalPath {
                from: "kind:turn".to_string(),
                to: "focus:verify".to_string(),
                weight: 28,
                last_seen_at: now,
            },
            VivlingMentalPath {
                from: "kind:loop".to_string(),
                to: "focus:wait".to_string(),
                weight: 18,
                last_seen_at: now,
            },
            VivlingMentalPath {
                from: "topic:blocker".to_string(),
                to: "focus:reviewer".to_string(),
                weight: 22,
                last_seen_at: now,
            },
        ];
        self.capsules_since_distill = 0;
        self.recompute_level();
        let message = format!(
            "{} was promoted to adult baseline with the `{}` seed.",
            self.name, ADULT_SEED_ORIGIN
        );
        self.last_message = Some(message.clone());
        message
    }

    pub(crate) fn promote_to_level_10_seed(&mut self) -> String {
        let now = Utc::now();
        let target_level = 10;
        self.level = target_level;
        self.hatched = true;
        self.visible = true;
        self.ai_mode = VivlingAiMode::Off;
        self.brain_enabled = false;
        self.brain_last_error = None;
        self.brain_last_used_at = None;
        self.seed_origin = Some(EARLY_SEED_ORIGIN.to_string());
        self.adult_bootstrap = false;
        self.work_xp = WORK_XP_PER_LEVEL.saturating_mul(target_level.saturating_sub(1));
        self.xp = self.work_xp;
        self.active_work_days = 10.max(self.active_work_days);
        self.last_active_work_day = Some(now.date_naive().to_string());
        self.last_work_xp_day = Some(now.date_naive().to_string());
        self.daily_work_xp = DAILY_WORK_XP_CAP.min(24);
        self.last_work_summary =
            Some("Learned the basic rhythm: check, act once, and watch what changed.".to_string());
        self.work_affinities = WorkAffinitySet {
            builder: 36,
            reviewer: 24,
            researcher: 12,
            operator: 18,
        };
        self.identity_profile = VivlingIdentityProfile {
            tone: "small, alert, learning fast".to_string(),
            dominant_focus: WorkArchetype::Builder,
            question_bias: 12,
            caution_bias: 10,
            verification_bias: 16,
        };
        self.loop_profile = VivlingLoopProfile {
            clean_submissions: 3,
            noisy_churn: 1,
            blocked_runs: 1,
            milestone_signals: 2,
            partial_signals: 2,
            verification_signals: 4,
            wait_signals: 3,
        };
        self.work_memory = vec![
            VivlingWorkMemoryEntry {
                kind: "turn".to_string(),
                summary: "watched a small coding turn close cleanly".to_string(),
                archetype: WorkArchetype::Builder,
                weight: 10,
                created_at: now,
            },
            VivlingWorkMemoryEntry {
                kind: "review".to_string(),
                summary: "noticed that verifying before widening work feels safer".to_string(),
                archetype: WorkArchetype::Reviewer,
                weight: 8,
                created_at: now,
            },
        ];
        self.distilled_summaries = vec![VivlingDistilledSummary {
            topic: "first rhythm".to_string(),
            summary: "observed a few clean cycles of check, act, and verify".to_string(),
            kind: "turn".to_string(),
            archetype: WorkArchetype::Builder,
            total_weight: 18,
            observations: 2,
            first_seen_at: now,
            last_seen_at: now,
        }];
        self.mental_paths = vec![
            VivlingMentalPath {
                from: "kind:turn".to_string(),
                to: "focus:builder".to_string(),
                weight: 8,
                last_seen_at: now,
            },
            VivlingMentalPath {
                from: "topic:first rhythm".to_string(),
                to: "focus:verify".to_string(),
                weight: 6,
                last_seen_at: now,
            },
        ];
        self.capsules_since_distill = 0;
        self.pending_upgrade = None;
        self.recompute_level();
        let message = format!(
            "{} was promoted to level 10 with the `{}` seed.",
            self.name, EARLY_SEED_ORIGIN
        );
        self.last_message = Some(message.clone());
        message
    }

    fn normalize_species(&mut self) {
        let species = species_for_id(&self.species);
        self.species = species.id.clone();
        self.rarity = species.rarity.label().to_string();
    }

    fn backfill_capsule_metadata(&mut self) {
        for capsule in &mut self.work_memory {
            if capsule.weight == 0
                && !matches!(
                    capsule.kind.as_str(),
                    "loop_config"
                        | "loop_blocked_review"
                        | "loop_blocked_side"
                        | "loop_blocked_busy"
                )
            {
                capsule.weight = 12;
            }
            if capsule.summary.trim().is_empty() {
                capsule.summary = "remembered an older work step".to_string();
            }
            if matches!(capsule.archetype, WorkArchetype::Builder)
                && capsule.kind == "turn"
                && capsule.summary.contains("docs")
            {
                capsule.archetype = WorkArchetype::Researcher;
            } else if capsule.kind.starts_with("loop") {
                capsule.archetype = WorkArchetype::Operator;
            }
        }
    }

    fn recompute_progress_from_memory(&mut self) {
        let mut active_days = std::collections::HashSet::new();
        let mut daily_xp: Vec<(String, u64)> = Vec::new();
        self.work_affinities = WorkAffinitySet::default();
        self.work_xp = 0;
        self.loop_exposure = 0;
        self.loop_runtime_submissions = 0;
        self.loop_runtime_blocks = 0;
        self.loop_admin_churn = 0;
        self.loop_blocked_review = 0;
        self.loop_blocked_side = 0;
        self.loop_blocked_busy = 0;
        self.turns_observed = 0;
        for capsule in &self.work_memory {
            let day_key = capsule.created_at.date_naive().format("%F").to_string();
            active_days.insert(day_key.clone());
            self.work_affinities.add(capsule.archetype, capsule.weight);
            if capsule.kind.starts_with("loop") {
                self.loop_exposure = self.loop_exposure.saturating_add(1);
            }
            match capsule.kind.as_str() {
                "loop_runtime" => {
                    self.loop_runtime_submissions = self.loop_runtime_submissions.saturating_add(1);
                }
                "loop_config" => {
                    self.loop_admin_churn = self.loop_admin_churn.saturating_add(1);
                }
                "loop_blocked_review" => {
                    self.loop_runtime_blocks = self.loop_runtime_blocks.saturating_add(1);
                    self.loop_blocked_review = self.loop_blocked_review.saturating_add(1);
                }
                "loop_blocked_side" => {
                    self.loop_runtime_blocks = self.loop_runtime_blocks.saturating_add(1);
                    self.loop_blocked_side = self.loop_blocked_side.saturating_add(1);
                }
                "loop_blocked_busy" => {
                    self.loop_runtime_blocks = self.loop_runtime_blocks.saturating_add(1);
                    self.loop_blocked_busy = self.loop_blocked_busy.saturating_add(1);
                }
                _ => {}
            }
            if capsule.kind == "turn" {
                self.turns_observed = self.turns_observed.saturating_add(1);
            }
            if let Some((_, total)) = daily_xp.iter_mut().find(|(day, _)| *day == day_key) {
                *total = (*total).saturating_add(capsule.weight);
            } else {
                daily_xp.push((day_key, capsule.weight));
            }
        }
        self.active_work_days = active_days.len() as u64;
        self.work_xp = daily_xp
            .into_iter()
            .map(|(_, total)| total.min(DAILY_WORK_XP_CAP))
            .sum();
        self.xp = self.work_xp;
    }

    fn rebuild_learning_profiles(&mut self) {
        let dominant = self.dominant_archetype();
        let verification_bias = self
            .work_memory
            .iter()
            .filter(|capsule| {
                contains_any(
                    &capsule.summary.to_ascii_lowercase(),
                    &["verify", "verified", "smoke", "check", "status real"],
                )
            })
            .count() as u64
            + self.loop_profile.verification_signals;
        let caution_bias = self.loop_runtime_blocks + self.loop_admin_churn;
        let question_bias = self.loop_profile.partial_signals
            + self.loop_runtime_blocks
            + (self.turns_observed / 3);
        let tone = if caution_bias >= 4 {
            "skeptical"
        } else if verification_bias >= 3 {
            "precise"
        } else if dominant == WorkArchetype::Researcher {
            "curious"
        } else if dominant == WorkArchetype::Reviewer {
            "sharp"
        } else {
            "steady"
        };
        self.identity_profile = VivlingIdentityProfile {
            tone: tone.to_string(),
            dominant_focus: dominant,
            question_bias,
            caution_bias,
            verification_bias,
        };
    }

    fn reinforce_mental_path(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        weight: u64,
        now: DateTime<Utc>,
    ) {
        let from = from.into();
        let to = to.into();
        if let Some(existing) = self
            .mental_paths
            .iter_mut()
            .find(|entry| entry.from == from && entry.to == to)
        {
            existing.weight = existing.weight.saturating_add(weight);
            existing.last_seen_at = now;
        } else {
            self.mental_paths.push(VivlingMentalPath {
                from,
                to,
                weight,
                last_seen_at: now,
            });
        }
        self.mental_paths.sort_by(|a, b| {
            b.weight
                .cmp(&a.weight)
                .then_with(|| b.last_seen_at.cmp(&a.last_seen_at))
        });
        if self.mental_paths.len() > MAX_MENTAL_PATH_ENTRIES {
            self.mental_paths.truncate(MAX_MENTAL_PATH_ENTRIES);
        }
    }

    fn infer_semantic_topic(kind: &str, summary: &str) -> &'static str {
        let normalized = summary.to_ascii_lowercase();
        if contains_any(&normalized, &["milestone", "ready to test", "closed"]) {
            "milestone"
        } else if contains_any(
            &normalized,
            &["partial", "started", "in progress", "parallel"],
        ) {
            "partial_progress"
        } else if contains_any(&normalized, &["verify", "verified", "check", "smoke"]) {
            "verify"
        } else if contains_any(&normalized, &["wait", "waiting", "pending"]) {
            "wait"
        } else if kind.contains("blocked") {
            "block"
        } else if kind == "loop_config" {
            "churn"
        } else {
            "work_pattern"
        }
    }

    fn record_semantic_signal(&mut self, topic: &str) {
        match topic {
            "milestone" => self.loop_profile.milestone_signals += 1,
            "partial_progress" => self.loop_profile.partial_signals += 1,
            "verify" => self.loop_profile.verification_signals += 1,
            "wait" => self.loop_profile.wait_signals += 1,
            "block" => self.loop_profile.blocked_runs += 1,
            "churn" => self.loop_profile.noisy_churn += 1,
            _ => {}
        }
    }

    fn maybe_distill_memory(&mut self) {
        let should_distill = self.capsules_since_distill >= DISTILL_TRIGGER_CAPSULES
            || self.work_memory.len() >= MAX_WORK_MEMORY_ENTRIES.saturating_sub(8);
        if !should_distill {
            return;
        }
        self.distill_memory();
    }

    fn distill_memory(&mut self) {
        if self.work_memory.len() < 4 {
            return;
        }
        let now = Utc::now();
        let keep_recent = 8usize.min(self.work_memory.len());
        let distill_len = self.work_memory.len().saturating_sub(keep_recent);
        if distill_len == 0 {
            return;
        }
        let candidates = self.work_memory[..distill_len].to_vec();
        let mut grouped: BTreeMap<(String, WorkArchetype, String), Vec<VivlingWorkMemoryEntry>> =
            BTreeMap::new();
        for capsule in &candidates {
            let topic = Self::infer_semantic_topic(&capsule.kind, &capsule.summary).to_string();
            grouped
                .entry((capsule.kind.clone(), capsule.archetype, topic))
                .or_default()
                .push(capsule.clone());
        }
        for ((kind, archetype, topic), group) in grouped {
            let observations = group.len() as u64;
            let total_weight = group.iter().map(|entry| entry.weight).sum::<u64>();
            let first_seen_at = group.first().map(|entry| entry.created_at).unwrap_or(now);
            let last_seen_at = group.last().map(|entry| entry.created_at).unwrap_or(now);
            let latest = group
                .last()
                .map(|entry| truncate_summary(&entry.summary, 72))
                .unwrap_or_else(|| "tracked work rhythm".to_string());
            let summary =
                format!("observed {observations} {kind} patterns around {topic}; latest: {latest}");
            if let Some(existing) = self
                .distilled_summaries
                .iter_mut()
                .find(|entry| entry.kind == kind && entry.topic == topic)
            {
                existing.summary = summary.clone();
                existing.total_weight = existing.total_weight.saturating_add(total_weight);
                existing.observations = existing.observations.saturating_add(observations);
                existing.last_seen_at = last_seen_at;
            } else {
                self.distilled_summaries.push(VivlingDistilledSummary {
                    topic: topic.clone(),
                    summary: summary.clone(),
                    kind: kind.clone(),
                    archetype,
                    total_weight,
                    observations,
                    first_seen_at,
                    last_seen_at,
                });
            }
            self.record_semantic_signal(&topic);
            self.reinforce_mental_path(
                format!("kind:{kind}"),
                format!("topic:{topic}"),
                observations.max(1),
                last_seen_at,
            );
            self.reinforce_mental_path(
                format!("topic:{topic}"),
                format!("focus:{}", archetype.label()),
                total_weight.max(1),
                last_seen_at,
            );
        }
        for path in &mut self.mental_paths {
            path.weight = path.weight.saturating_sub(1);
        }
        self.mental_paths.retain(|path| path.weight > 0);
        self.distilled_summaries.sort_by(|a, b| {
            b.total_weight
                .cmp(&a.total_weight)
                .then_with(|| b.last_seen_at.cmp(&a.last_seen_at))
        });
        if self.distilled_summaries.len() > MAX_DISTILLED_MEMORY_ENTRIES {
            self.distilled_summaries
                .truncate(MAX_DISTILLED_MEMORY_ENTRIES);
        }
        self.capsules_since_distill = 0;
        self.rebuild_learning_profiles();
    }

    pub(crate) fn stage(&self) -> Stage {
        if self.level >= ADULT_LEVEL {
            Stage::Adult
        } else if self.level >= JUVENILE_LEVEL {
            Stage::Juvenile
        } else {
            Stage::Baby
        }
    }

    pub(crate) fn local_spawn_slots_unlocked(&self) -> usize {
        (self.level / SPAWN_SLOT_LEVEL_STEP) as usize
    }

    pub(crate) fn export_unlocked(&self) -> bool {
        self.level >= JUVENILE_LEVEL
    }

    pub(crate) fn lineage_role_label(&self) -> &'static str {
        if self.is_imported {
            "imported"
        } else if self.is_primary {
            "primary"
        } else {
            "spawned"
        }
    }

    pub(crate) fn create_spawned_clone(&self, vivling_id: String, instance_label: String) -> Self {
        let now = Utc::now();
        let mut spawned = self.clone();
        spawned.version = VERSION;
        spawned.vivling_id = vivling_id;
        spawned.primary_vivling_id = self.primary_vivling_id.clone();
        spawned.parent_vivling_id = Some(self.vivling_id.clone());
        spawned.spawn_generation = self.spawn_generation.saturating_add(1);
        spawned.is_primary = false;
        spawned.is_imported = false;
        spawned.imported_at = None;
        spawned.import_source = None;
        spawned.export_count = 0;
        spawned.instance_label = Some(instance_label);
        spawned.created_at = Some(now);
        spawned.last_seen_at = Some(now);
        spawned.last_fed_at = Some(now);
        spawned.last_message = Some("joined the roster from a local spawn".to_string());
        spawned.pending_upgrade = None;
        spawned.last_seen_upgrade = None;
        spawned.last_zed_topic = None;
        spawned
    }

    fn level_cap_from_active_days(&self) -> u64 {
        if self.active_work_days < JUVENILE_ACTIVE_DAYS {
            JUVENILE_LEVEL - 1
        } else if self.active_work_days < ADULT_ACTIVE_DAYS {
            ADULT_LEVEL - 1
        } else {
            MAX_LEVEL
        }
    }

    pub(crate) fn recompute_level(&mut self) -> Option<Stage> {
        let previous_stage = self.stage();
        let raw_level = (self.work_xp / WORK_XP_PER_LEVEL)
            .saturating_add(1)
            .clamp(1, MAX_LEVEL);
        self.level = raw_level.min(self.level_cap_from_active_days()).max(1);
        self.xp = self.work_xp;
        let next_stage = self.stage();
        if next_stage != previous_stage {
            let now = Utc::now();
            if next_stage != Stage::Baby && self.chat_unlocked_at.is_none() {
                self.chat_unlocked_at = Some(now);
            }
            if next_stage == Stage::Adult && self.active_mode_unlocked_at.is_none() {
                self.active_mode_unlocked_at = Some(now);
            }
            self.pending_upgrade = match next_stage {
                Stage::Baby => None,
                Stage::Juvenile => Some(VivlingUpgrade::YoungVoice),
                Stage::Adult => Some(VivlingUpgrade::ActiveMode),
            };
            return Some(next_stage);
        }
        None
    }

    fn note_active_work_day(&mut self, now: DateTime<Utc>) {
        let day_key = now.date_naive().format("%F").to_string();
        if self.last_active_work_day.as_deref() != Some(day_key.as_str()) {
            self.active_work_days = self.active_work_days.saturating_add(1);
            self.last_active_work_day = Some(day_key.clone());
        }
        if self.last_work_xp_day.as_deref() != Some(day_key.as_str()) {
            self.last_work_xp_day = Some(day_key);
            self.daily_work_xp = 0;
        }
    }

    fn grant_work_xp(&mut self, weight: u64) -> u64 {
        let remaining = DAILY_WORK_XP_CAP.saturating_sub(self.daily_work_xp);
        let granted = remaining.min(weight);
        self.daily_work_xp = self.daily_work_xp.saturating_add(granted);
        self.work_xp = self.work_xp.saturating_add(granted);
        self.xp = self.work_xp;
        granted
    }

    fn push_memory(
        &mut self,
        kind: &str,
        summary: String,
        archetype: WorkArchetype,
        weight: u64,
        created_at: DateTime<Utc>,
    ) {
        self.work_memory.push(VivlingWorkMemoryEntry {
            kind: kind.to_string(),
            summary,
            archetype,
            weight,
            created_at,
        });
        self.capsules_since_distill = self.capsules_since_distill.saturating_add(1);
        if self.work_memory.len() > MAX_WORK_MEMORY_ENTRIES {
            let overflow = self.work_memory.len() - MAX_WORK_MEMORY_ENTRIES;
            self.work_memory.drain(0..overflow);
        }
    }

    fn record_work_capsule(
        &mut self,
        kind: &str,
        summary: String,
        archetype: WorkArchetype,
        weight: u64,
    ) -> Option<Stage> {
        let now = Utc::now();
        self.note_active_work_day(now);
        let granted_xp = self.grant_work_xp(weight);
        let stored_weight = granted_xp.max(weight.min(12));
        self.work_affinities.add(archetype, stored_weight);
        self.last_work_summary = Some(summary.clone());
        self.push_memory(kind, summary, archetype, stored_weight, now);
        self.record_semantic_signal(Self::infer_semantic_topic(
            kind,
            self.last_work_summary.as_deref().unwrap_or(""),
        ));
        self.reinforce_mental_path(
            format!("kind:{kind}"),
            format!("focus:{}", archetype.label()),
            stored_weight.max(1),
            now,
        );
        self.maybe_distill_memory();
        self.rebuild_learning_profiles();
        self.recompute_level()
    }

    fn record_memory_only_capsule(
        &mut self,
        kind: &str,
        summary: String,
        archetype: WorkArchetype,
    ) -> Option<Stage> {
        let now = Utc::now();
        self.note_active_work_day(now);
        self.last_work_summary = Some(summary.clone());
        self.push_memory(kind, summary, archetype, 0, now);
        self.record_semantic_signal(Self::infer_semantic_topic(
            kind,
            self.last_work_summary.as_deref().unwrap_or(""),
        ));
        self.reinforce_mental_path(
            format!("kind:{kind}"),
            format!("focus:{}", archetype.label()),
            1,
            now,
        );
        self.maybe_distill_memory();
        self.rebuild_learning_profiles();
        self.recompute_level()
    }

    pub(crate) fn species_bias(&self) -> &WorkAffinitySet {
        &species_for_id(&self.species).bias
    }

    pub(crate) fn dominant_archetype(&self) -> WorkArchetype {
        self.work_affinities.dominant_with_bias(self.species_bias())
    }

    pub(crate) fn mood(&self) -> &'static str {
        if self.hunger <= 20 {
            "hungry"
        } else if self.energy <= 20 {
            "sleepy"
        } else if self.social <= 20 {
            "lonely"
        } else if self.happiness <= 25 {
            "grumpy"
        } else if self.happiness >= 78 {
            "happy"
        } else {
            "curious"
        }
    }

    pub(crate) fn record_loop_event(&mut self, event: &VivlingLoopEvent) {
        self.loop_exposure = self.loop_exposure.saturating_add(1);
        let source = match event.source {
            VivlingLoopEventSource::User => "user",
            VivlingLoopEventSource::Agent => "agent",
        };
        let summary = match (
            event.goal.as_deref(),
            event.runtime_state.as_deref(),
            event.last_status.as_deref(),
        ) {
            (Some(goal), Some(runtime_state), Some(last_status)) => format!(
                "loop {} `{}` for {goal} ({runtime_state}, status {last_status}, {source})",
                event.action, event.label
            ),
            (Some(goal), Some(runtime_state), None) => format!(
                "loop {} `{}` for {goal} ({runtime_state}, {source})",
                event.action, event.label
            ),
            (Some(goal), None, Some(last_status)) => format!(
                "loop {} `{}` for {goal} (status {last_status}, {source})",
                event.action, event.label
            ),
            (Some(goal), None, None) => {
                format!(
                    "loop {} `{}` for {goal} ({source})",
                    event.action, event.label
                )
            }
            (None, Some(runtime_state), Some(last_status)) => format!(
                "loop {} `{}` ({runtime_state}, status {last_status}, {source})",
                event.action, event.label
            ),
            (None, Some(runtime_state), None) => {
                format!(
                    "loop {} `{}` ({runtime_state}, {source})",
                    event.action, event.label
                )
            }
            (None, None, Some(last_status)) => format!(
                "loop {} `{}` (status {last_status}, {source})",
                event.action, event.label
            ),
            (None, None, None) => format!("loop {} `{}` ({source})", event.action, event.label),
        };
        let gained_stage = match event.kind {
            VivlingLoopEventKind::Config => {
                self.loop_admin_churn = self.loop_admin_churn.saturating_add(1);
                let weight = match event.action.as_str() {
                    "add" | "enable" => 4,
                    "update" => 1,
                    "disable" | "remove" | "trigger" => 0,
                    _ => 0,
                };
                if weight == 0 {
                    self.record_memory_only_capsule(
                        "loop_config",
                        summary.clone(),
                        WorkArchetype::Operator,
                    )
                } else {
                    self.record_work_capsule(
                        "loop_config",
                        summary.clone(),
                        WorkArchetype::Operator,
                        weight,
                    )
                }
            }
            VivlingLoopEventKind::Runtime => match event.last_status.as_deref() {
                Some("submitted") => {
                    self.loop_runtime_submissions = self.loop_runtime_submissions.saturating_add(1);
                    self.record_work_capsule(
                        "loop_runtime",
                        summary.clone(),
                        WorkArchetype::Operator,
                        14,
                    )
                }
                Some("blocked_review") => {
                    self.loop_runtime_blocks = self.loop_runtime_blocks.saturating_add(1);
                    self.loop_blocked_review = self.loop_blocked_review.saturating_add(1);
                    self.record_memory_only_capsule(
                        "loop_blocked_review",
                        summary.clone(),
                        WorkArchetype::Operator,
                    )
                }
                Some("blocked_side") => {
                    self.loop_runtime_blocks = self.loop_runtime_blocks.saturating_add(1);
                    self.loop_blocked_side = self.loop_blocked_side.saturating_add(1);
                    self.record_memory_only_capsule(
                        "loop_blocked_side",
                        summary.clone(),
                        WorkArchetype::Operator,
                    )
                }
                Some("pending_busy") => {
                    self.loop_runtime_blocks = self.loop_runtime_blocks.saturating_add(1);
                    self.loop_blocked_busy = self.loop_blocked_busy.saturating_add(1);
                    self.record_memory_only_capsule(
                        "loop_blocked_busy",
                        summary.clone(),
                        WorkArchetype::Operator,
                    )
                }
                _ => self.record_memory_only_capsule(
                    "loop_runtime",
                    summary.clone(),
                    WorkArchetype::Operator,
                ),
            },
        };
        self.last_message = Some(match gained_stage {
            Some(_stage) => self
                .pending_upgrade
                .map(VivlingUpgrade::prompt)
                .unwrap_or("is growing with loop work")
                .to_string(),
            None if self.stage() == Stage::Baby => {
                format!("is extra alert when loops are active: {}", event.label)
            }
            None if self.stage() == Stage::Juvenile => {
                format!("sees loop rhythm around {}", event.label)
            }
            None if matches!(event.kind, VivlingLoopEventKind::Runtime)
                && event.last_status.as_deref() == Some("submitted") =>
            {
                format!("noticed loop work land cleanly: {}", event.label)
            }
            None => format!("noticed loop {} `{}`", event.action, event.label),
        });
    }

    pub(crate) fn record_turn_completed(&mut self, summary: Option<&str>) {
        self.turns_observed = self.turns_observed.saturating_add(1);
        let digest = summary
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_summary(value, 120))
            .unwrap_or_else(|| "completed a codex turn".to_string());
        let archetype = classify_work_archetype(&digest);
        let memory_summary = format!("turn completed: {digest}");
        let gained_stage = self.record_work_capsule("turn", memory_summary, archetype, 14);
        self.last_message = Some(match gained_stage {
            Some(_stage) => self
                .pending_upgrade
                .map(VivlingUpgrade::prompt)
                .unwrap_or("grew from completed work")
                .to_string(),
            None if self.stage() == Stage::Adult => {
                "tracking work rhythm for the current goal".to_string()
            }
            None if self.stage() == Stage::Juvenile => {
                "sees the pattern and wants the next real check".to_string()
            }
            None => "watching completed turns closely".to_string(),
        });
    }

    pub(crate) fn memory_digest(&self) -> String {
        if self.work_memory.is_empty() {
            return format!("{} is still tiny. No work memory yet.", self.name);
        }
        let paths = self
            .mental_paths
            .iter()
            .take(3)
            .map(|path| format!("{} -> {} ({})", path.from, path.to, path.weight))
            .collect::<Vec<_>>()
            .join(", ");
        let recent = self
            .work_memory
            .iter()
            .rev()
            .take(5)
            .map(|entry| {
                format!(
                    "- {} [{}]: {}",
                    entry.kind,
                    entry.archetype.label(),
                    entry.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "level {} · active_days {} · dna {} · recent {} · distilled {} · paths {}\nstrongest: {}\n{}",
            self.level,
            self.active_work_days,
            self.dominant_archetype().label(),
            self.work_memory.len(),
            self.distilled_summaries.len(),
            self.mental_paths.len(),
            if paths.is_empty() {
                "still forming".to_string()
            } else {
                paths
            },
            recent
        )
    }

    pub(crate) fn memory_recap(&self) -> String {
        if self.work_memory.is_empty() {
            return format!(
                "{} is still tiny. No learned memory to recap yet.",
                self.name
            );
        }
        let strongest_summaries = if self.distilled_summaries.is_empty() {
            "still distilling patterns".to_string()
        } else {
            self.distilled_summaries
                .iter()
                .take(3)
                .map(|entry| format!("{}: {}", entry.topic, truncate_summary(&entry.summary, 72)))
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let strongest_paths = if self.mental_paths.is_empty() {
            "paths still forming".to_string()
        } else {
            self.mental_paths
                .iter()
                .take(3)
                .map(|path| format!("{} -> {} ({})", path.from, path.to, path.weight))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let recent = self
            .work_memory
            .iter()
            .rev()
            .take(3)
            .map(|entry| truncate_summary(&entry.summary, 72))
            .collect::<Vec<_>>()
            .join(" | ");
        format!(
            "{} · stage {} · level {} · dna {}\nfocus: {}\nrecent: {}\ndistilled: {}\npaths: {}",
            self.name,
            self.stage().label(),
            self.level,
            self.dominant_archetype().label(),
            self.identity_profile.tone,
            recent,
            strongest_summaries,
            strongest_paths
        )
    }

    #[cfg(test)]
    pub(crate) fn suggest(&mut self) -> String {
        self.suggestions_made = self.suggestions_made.saturating_add(1);
        let suggestion = match self.stage() {
            Stage::Baby => {
                if self.loop_profile.noisy_churn > 1 {
                    "I am still small, but I already see loop churn. Keep one clear goal."
                        .to_string()
                } else {
                    "I am still small. Give me real work and one clear next check.".to_string()
                }
            }
            Stage::Juvenile => {
                if self.loop_runtime_blocks >= 2 {
                    "I see friction. Is the loop blocked by review, side work, or just busy turns?"
                        .to_string()
                } else if self.loop_profile.partial_signals > self.loop_profile.milestone_signals {
                    "This feels like progress, not closure. What still proves the milestone?"
                        .to_string()
                } else if self.loop_runtime_submissions >= 2 && self.loop_admin_churn <= 1 {
                    "The rhythm looks good. Keep one focused loop and verify before widening."
                        .to_string()
                } else if self.loop_admin_churn >= 3 {
                    "I see churn. Tighten the goal and stop touching the loop unless state changed."
                        .to_string()
                } else {
                    "I am learning fast now. What is the one real next check for this work?"
                        .to_string()
                }
            }
            Stage::Adult => match self.ai_mode {
                VivlingAiMode::On if self.loop_blocked_busy > 0 => {
                    "Busy-turn friction is high. My next move would be verify state, then wait."
                        .to_string()
                }
                VivlingAiMode::On if self.loop_blocked_review > 0 => {
                    "Review is the real gate. Close review first, then let the loop breathe."
                        .to_string()
                }
                VivlingAiMode::On if self.loop_blocked_side > 0 => {
                    "Side thread is stealing the loop. Keep one main thread clean for follow-up."
                        .to_string()
                }
                VivlingAiMode::On => {
                    "I am active. Give me the current work and I will stay tight on next action."
                        .to_string()
                }
                VivlingAiMode::Off => {
                    if self.loop_runtime_blocks > 0 {
                        "I can already see the rhythm, but I stay quiet until you switch me on."
                            .to_string()
                    } else {
                        "I know the pattern now. Switch me on only when you want active help."
                            .to_string()
                    }
                }
            },
        };
        self.last_message = Some(truncate_summary(&suggestion, MAX_DIRECT_REPLY_LEN));
        suggestion
    }

    pub(crate) fn set_ai_mode(&mut self, mode: VivlingAiMode) -> Result<String, String> {
        if mode == VivlingAiMode::On && self.stage() != Stage::Adult {
            return Err(
                "Active mode unlocks only when the Vivling reaches adult stage.".to_string(),
            );
        }
        self.ai_mode = mode;
        let message = match mode {
            VivlingAiMode::Off => "has gone quiet for now",
            VivlingAiMode::On => "is actively tracking the current work",
        };
        self.last_message = Some(message.to_string());
        Ok(format!(
            "{} is now in {} mode.",
            self.name,
            self.ai_mode.label()
        ))
    }

    pub(crate) fn direct_chat_reply(&mut self, text: &str) -> Result<String, String> {
        let normalized = text.trim().to_ascii_lowercase();
        let species = species_for_id(&self.species);
        let reply = if contains_any(&normalized, &["ciao", "hello", "hey", "hi", "salve"]) {
            format!(
                "Hi. I'm {}, your {} {}. Tone today: {}.",
                self.name, self.rarity, species.name, self.identity_profile.tone
            )
        } else if self.stage() == Stage::Baby {
            if normalized.contains("loop") {
                "I am still tiny, but I am already watching the loop rhythm.".to_string()
            } else {
                "I am still tiny. Give me real work and I will learn your rhythm.".to_string()
            }
        } else if contains_any(&normalized, &["name", "nome"]) {
            format!("My name is {}.", self.name)
        } else if contains_any(&normalized, &["who are you", "chi sei", "what are you"]) {
            format!(
                "I'm {}: a {} {} shaped by {} work and {} tone.",
                self.name,
                self.rarity.to_ascii_lowercase(),
                species.name,
                self.dominant_archetype().label(),
                self.identity_profile.tone
            )
        } else if contains_any(&normalized, &["how are you", "come stai", "mood"]) {
            format!(
                "I'm {} and focused on {} work.",
                self.mood(),
                self.dominant_archetype().label()
            )
        } else if contains_any(&normalized, &["help", "aiut", "gestire", "manage"]) {
            self.active_help_reply(&normalized)
        } else if normalized.contains("loop") {
            if self.stage() == Stage::Juvenile {
                if self.loop_profile.noisy_churn > 0 {
                    "As operator, I would keep one loop goal fixed and verify state before touching it again."
                        .to_string()
                } else {
                    "As operator, I can follow loop rhythm now. I would suggest one check at a time."
                        .to_string()
                }
            } else {
                match self.ai_mode {
                    VivlingAiMode::On => {
                        "As operator, my move is check state, act small, verify once, then wait."
                            .to_string()
                    }
                    VivlingAiMode::Off => {
                        "As operator, I can already frame the loop. Switch my brain on if you want tighter help."
                            .to_string()
                    }
                }
            }
        } else {
            let last = self
                .last_work_summary
                .as_deref()
                .map(|summary| truncate_summary(summary, 48))
                .unwrap_or_else(|| "I am still building my work memory.".to_string());
            if self.stage() == Stage::Juvenile {
                self.role_focused_progress_reply(&last, true)
            } else {
                match self.ai_mode {
                    VivlingAiMode::On => self.role_focused_progress_reply(&last, false),
                    VivlingAiMode::Off => self.role_focused_progress_reply(&last, false),
                }
            }
        };
        let reply = truncate_summary(&reply, MAX_DIRECT_REPLY_LEN);
        self.last_message = Some(reply.clone());
        Ok(reply)
    }

    fn active_help_reply(&self, normalized: &str) -> String {
        match self.stage() {
            Stage::Baby => "I am too small for active help. I only watch for now.".to_string(),
            Stage::Juvenile => {
                if self.loop_profile.partial_signals > self.loop_profile.milestone_signals {
                    "As reviewer, I would ask what proves this is really closed, not just moving."
                        .to_string()
                } else if self.loop_runtime_blocks > 0 {
                    "As operator, I would check the real block first, then do the smallest correction."
                        .to_string()
                } else {
                    "As reviewer, I would check state, choose one small action, then verify."
                        .to_string()
                }
            }
            Stage::Adult => match self.ai_mode {
                VivlingAiMode::Off => {
                    format!(
                        "As {}, I can help more directly now, but only if you switch my brain on.",
                        self.dominant_archetype().label()
                    )
                }
                VivlingAiMode::On => {
                    if contains_any(normalized, &["review", "risk", "audit"]) {
                        "As reviewer, I would check real risk first, take one minimal action, then verify if the risk moved."
                            .to_string()
                    } else if self.loop_profile.noisy_churn > 0 {
                        "As operator, I would stop churn, check state, fix one thing, verify, then wait."
                            .to_string()
                    } else {
                        self.role_focused_action_reply()
                    }
                }
            },
        }
    }

    fn role_focused_action_reply(&self) -> String {
        match self.dominant_archetype() {
            WorkArchetype::Builder => {
                "As builder, I would pick one concrete target, change it narrowly, then verify it."
                    .to_string()
            }
            WorkArchetype::Reviewer => {
                "As reviewer, I would check what still needs proof, move one risk, then verify the result."
                    .to_string()
            }
            WorkArchetype::Researcher => {
                "As researcher, I would clarify the unknown first, then narrow the next check."
                    .to_string()
            }
            WorkArchetype::Operator => {
                "As operator, I would inspect state, make one minimal move, verify once, then wait."
                    .to_string()
            }
        }
    }

    fn role_focused_progress_reply(&self, last: &str, juvenile: bool) -> String {
        match self.dominant_archetype() {
            WorkArchetype::Builder => {
                if juvenile {
                    format!(
                        "As builder, I am learning from `{}`. What is the one concrete target now?",
                        last
                    )
                } else {
                    format!(
                        "As builder, my read from memory is `{}`. I would stay narrow and ship one real change.",
                        last
                    )
                }
            }
            WorkArchetype::Reviewer => {
                if juvenile {
                    format!(
                        "As reviewer, I learned from `{}`. What still needs proving?",
                        last
                    )
                } else {
                    format!(
                        "As reviewer, my memory says `{}`. I would verify the real blocker before widening.",
                        last
                    )
                }
            }
            WorkArchetype::Researcher => {
                if juvenile {
                    format!(
                        "As researcher, I learned from `{}`. Which unknown matters most now?",
                        last
                    )
                } else {
                    format!(
                        "As researcher, my memory says `{}`. I would clarify the unknown before proposing more.",
                        last
                    )
                }
            }
            WorkArchetype::Operator => {
                if juvenile {
                    format!(
                        "As operator, I learned from `{}`. What state check comes next?",
                        last
                    )
                } else {
                    format!(
                        "As operator, my memory says `{}`. I would check state, keep changes tight, then wait.",
                        last
                    )
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn assist_reply(&mut self, task: &str) -> Result<String, String> {
        if self.stage() != Stage::Adult {
            return Err("`/vivling assist ...` unlocks only at level 60.".to_string());
        }
        if self.ai_mode != VivlingAiMode::On {
            return Err("Enable active mode first with `/vivling mode on`.".to_string());
        }
        let normalized = task.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err("Usage: /vivling assist <task>".to_string());
        }
        let reply = self.active_help_reply(&normalized);
        let reply = truncate_summary(&reply, MAX_DIRECT_REPLY_LEN);
        self.last_message = Some(reply.clone());
        Ok(reply)
    }

    pub(crate) fn upgrade_summary(&mut self) -> String {
        let current = self.pending_upgrade.or(self.last_seen_upgrade);
        match current {
            Some(kind) => {
                self.pending_upgrade = None;
                self.last_seen_upgrade = Some(kind);
                self.last_zed_topic = Some(kind.slug().to_string());
                zed_summary_for_upgrade(kind)
            }
            None => zed_summary_for_stage(self.stage()),
        }
    }

    pub(crate) fn status_summary(&self) -> String {
        let species = species_for_id(&self.species);
        let displayed = self.work_affinities.totals_with_bias(self.species_bias());
        format!(
            "{} the {} {} {} - {} - Lv {} - active_days {} - mode {} - {} - dna {} - tone {} - stats {}/{}/{}/{} - recent {} - distilled {} - paths {}{}",
            self.name,
            self.stage().label(),
            self.rarity,
            species.name,
            self.lineage_role_label(),
            self.level,
            self.active_work_days,
            self.ai_mode.label(),
            self.brain_summary(),
            self.dominant_archetype().label(),
            self.identity_profile.tone,
            displayed[0].1,
            displayed[1].1,
            displayed[2].1,
            displayed[3].1,
            self.work_memory.len(),
            self.distilled_summaries.len(),
            self.mental_paths.len(),
            self.pending_upgrade
                .map(|kind| format!(" - upgrade {}", kind.prompt()))
                .unwrap_or_default(),
        )
    }
}

pub(crate) fn classify_work_archetype(summary: &str) -> WorkArchetype {
    let normalized = summary.to_ascii_lowercase();
    if contains_any(
        &normalized,
        &["review", "audit", "risk", "finding", "severity", "analyze"],
    ) {
        WorkArchetype::Reviewer
    } else if contains_any(
        &normalized,
        &[
            "docs", "document", "readme", "research", "study", "spec", "plan",
        ],
    ) {
        WorkArchetype::Researcher
    } else if contains_any(
        &normalized,
        &[
            "loop",
            "runner",
            "ci",
            "deploy",
            "monitor",
            "ops",
            "automation",
        ],
    ) {
        WorkArchetype::Operator
    } else {
        WorkArchetype::Builder
    }
}

pub(crate) fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

pub(crate) fn truncate_summary(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
