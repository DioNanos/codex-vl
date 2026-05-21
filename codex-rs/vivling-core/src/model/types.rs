use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VivlingAiMode {
    Off,
    #[serde(alias = "suggest")]
    #[default]
    On,
}

impl VivlingAiMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "on" | "assistant" => Some(Self::On),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VivlingUpgrade {
    YoungVoice,
    ActiveMode,
}

impl VivlingUpgrade {
    pub fn slug(self) -> &'static str {
        match self {
            Self::YoungVoice => "young-voice",
            Self::ActiveMode => "active-mode",
        }
    }

    pub fn prompt(self) -> &'static str {
        match self {
            Self::YoungVoice => "/vivling upgrade for young voice",
            Self::ActiveMode => "/vivling mode on",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkArchetype {
    #[default]
    Builder,
    Reviewer,
    Researcher,
    Operator,
}

impl WorkArchetype {
    pub fn label(self) -> &'static str {
        match self {
            Self::Builder => "builder",
            Self::Reviewer => "reviewer",
            Self::Researcher => "researcher",
            Self::Operator => "operator",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkAffinitySet {
    #[serde(default)]
    pub builder: u64,
    #[serde(default)]
    pub reviewer: u64,
    #[serde(default)]
    pub researcher: u64,
    #[serde(default)]
    pub operator: u64,
}

impl WorkAffinitySet {
    pub fn add(&mut self, archetype: WorkArchetype, weight: u64) {
        match archetype {
            WorkArchetype::Builder => self.builder = self.builder.saturating_add(weight),
            WorkArchetype::Reviewer => self.reviewer = self.reviewer.saturating_add(weight),
            WorkArchetype::Researcher => {
                self.researcher = self.researcher.saturating_add(weight);
            }
            WorkArchetype::Operator => self.operator = self.operator.saturating_add(weight),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Baby,
    Juvenile,
    Adult,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Baby => "baby",
            Self::Juvenile => "juvenile",
            Self::Adult => "adult",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct VivlingWorkMemoryEntry {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub archetype: WorkArchetype,
    #[serde(default)]
    pub weight: u64,
    #[serde(default)]
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct VivlingDistilledSummary {
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub archetype: WorkArchetype,
    #[serde(default)]
    pub total_weight: u64,
    #[serde(default)]
    pub observations: u64,
    #[serde(default)]
    pub first_seen_at: DateTime<Utc>,
    #[serde(default)]
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct VivlingMentalPath {
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub weight: u64,
    #[serde(default)]
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct VivlingIdentityProfile {
    #[serde(default)]
    pub tone: String,
    #[serde(default)]
    pub dominant_focus: WorkArchetype,
    #[serde(default)]
    pub question_bias: u64,
    #[serde(default)]
    pub caution_bias: u64,
    #[serde(default)]
    pub verification_bias: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct VivlingLoopProfile {
    #[serde(default)]
    pub clean_submissions: u64,
    #[serde(default)]
    pub noisy_churn: u64,
    #[serde(default)]
    pub blocked_runs: u64,
    #[serde(default)]
    pub milestone_signals: u64,
    #[serde(default)]
    pub partial_signals: u64,
    #[serde(default)]
    pub verification_signals: u64,
    #[serde(default)]
    pub wait_signals: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VivlingLoopEventKind {
    Config,
    Runtime,
}

#[derive(Clone, Debug)]
pub enum VivlingLoopEventSource {
    User,
    Agent,
}

#[derive(Clone, Debug)]
pub struct VivlingLoopEvent {
    pub kind: VivlingLoopEventKind,
    pub source: VivlingLoopEventSource,
    pub action: String,
    pub label: String,
    pub runtime_state: Option<String>,
    pub last_status: Option<String>,
    pub goal: Option<String>,
}

// --- Memory V2 Step 2.A scaffolding ---
//
// The types below are added for the Vivling Memory V2 schema (state
// version 9). They are intentionally pure data: no runtime logic is wired
// up in this step. Each type is `Default`-able and `#[serde(default)]`-
// friendly so that V8 state JSON keeps loading into V9 binaries.

/// Auto-written Vivling identity paragraph produced by the sleep-time
/// memory agent (axis A). Persisted alongside `VivlingState` so the
/// next chat turn can inject it into the brain prompt as "voice".
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct VivlingVoice {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub generated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub source_capsules_count: u64,
    #[serde(default)]
    pub version: u32,
}

/// Split bias counters introduced by Memory V2 §10.2 to fix the
/// accumulator drift observed on Nilo (verification_bias = 5963 on a
/// level-22 Vivling). `accumulated` is monotonic from hatch; `recent`
/// tracks a sliding window (target: 30 days) and is rebuilt by the
/// memory agent. Wiring lives in later steps; this scaffolding only
/// reserves the storage and the default value (all zeros).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct BiasCounters {
    #[serde(default)]
    pub caution: u64,
    #[serde(default)]
    pub verification: u64,
    #[serde(default)]
    pub question: u64,
    #[serde(default)]
    pub milestone: u64,
    #[serde(default)]
    pub partial: u64,
    #[serde(default)]
    pub wait: u64,
}

/// How the Vivling reacts when the user's recent messages mix languages.
/// `MirrorUser` is the default per design §8.2 P2.10 q.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VivlingLanguageMode {
    DominantOnly,
    MirrorUser,
    Strict,
}

impl Default for VivlingLanguageMode {
    fn default() -> Self {
        Self::MirrorUser
    }
}

/// Memory V2 Step 12.B.A — tri-state opt-in for the *expression* LLM
/// channel (CRT live phrase + proactive footer). Decoupled from
/// `brain_enabled` on purpose: `brain_enabled` gates `/vivling assist`
/// and loop-tick LLM ownership, this gates only the always-on
/// "Vivling speaks" surface.
///
/// `Default` is stage-driven: Adult/Juvenile run normally, Baby fires
/// only on rare events (e.g. `turn_complete`). `On` forces the channel
/// on regardless of stage; `Off` mutes it entirely for this Vivling.
/// Persisted as part of the V10 schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VivlingExpressionMode {
    Default,
    On,
    Off,
}

impl Default for VivlingExpressionMode {
    fn default() -> Self {
        Self::Default
    }
}

/// Detected/override language state for the Vivling. Memory agent will
/// refresh `detected_language` from the rolling `recent_samples` window;
/// `language_override` is set explicitly by `/vivling language <code>`.
/// Pure-data here, no behaviour wired yet.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct VivlingLanguageState {
    #[serde(default)]
    pub detected_language: String,
    #[serde(default)]
    pub language_override: Option<String>,
    #[serde(default)]
    pub language_mode: VivlingLanguageMode,
    /// Last user-message samples used for language detection. Bounded
    /// to ~20 by the memory agent when it refreshes. Stored as plain
    /// vec for serde simplicity; a deque equivalent is fine at runtime.
    #[serde(default)]
    pub recent_samples: Vec<(DateTime<Utc>, String)>,
}

impl Default for VivlingLanguageState {
    fn default() -> Self {
        Self {
            detected_language: String::new(),
            language_override: None,
            language_mode: VivlingLanguageMode::MirrorUser,
            recent_samples: Vec::new(),
        }
    }
}

/// Skill abstracted from a recurring work pattern by the memory agent
/// (axis B). Persisted to a sidecar `<vivling_id>_skills.json` derived
/// at runtime — **not** a field of `VivlingState` (design §4.2 P1.4).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct VivlingSkill {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub trigger_keywords: Vec<String>,
    #[serde(default)]
    pub step_sequence: Vec<String>,
    #[serde(default)]
    pub success_count: u64,
    #[serde(default)]
    pub failure_count: u64,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub abstracted_from_capsules: Vec<String>,
    #[serde(default)]
    pub superseded_by: Option<String>,
}

/// Lineage knowledge a child Vivling inherits from its cultural parent
/// at spawn time (axis D extended). Not active behaviour: holding the
/// seed so the runtime can read it later.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct LineageInheritance {
    #[serde(default)]
    pub voice_fragment: Option<String>,
    #[serde(default)]
    pub skills: Vec<VivlingSkill>,
    #[serde(default)]
    pub preference_seed: VivlingPreferenceSeed,
    #[serde(default)]
    pub suggested_brain_profile: Option<String>,
}

/// Seed weights handed down to a newly-spawned Vivling so it doesn't
/// start from a perfectly neutral identity. Pure storage.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct VivlingPreferenceSeed {
    #[serde(default)]
    pub caution_bias_seed: u64,
    #[serde(default)]
    pub verification_bias_seed: u64,
    #[serde(default)]
    pub preferred_archetype: WorkArchetype,
}

/// Provenance metadata attached to memory records (capsules, distilled
/// summaries, skills, voice). Memory V2 §11.3 — supports `conflict /
/// supersedes` semantics via the tombstone trio
/// (`valid_until`, `superseded_by`). Stored separately from each
/// record's payload for now; field-level adoption lives in later steps.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Provenance {
    #[serde(default)]
    pub source: ProvenanceSource,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub valid_from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub valid_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub superseded_by: Option<String>,
}

impl Default for Provenance {
    fn default() -> Self {
        Self {
            source: ProvenanceSource::Turn,
            confidence: 0.0,
            valid_from: None,
            valid_until: None,
            superseded_by: None,
        }
    }
}

/// Where a memory record was produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceSource {
    Turn,
    Loop,
    Lineage,
    MemoryAgent,
    UserExplicit,
}

impl Default for ProvenanceSource {
    fn default() -> Self {
        Self::Turn
    }
}

/// Cached CRT footer phrase produced live by the lightweight LLM
/// (axis F). Volatile and reconstructible; `#[serde(skip)]` on the
/// state field prevents the cache from polluting on-disk snapshots.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CachedCrtPhrase {
    pub text: String,
    pub generated_at: Option<DateTime<Utc>>,
}

/// Cached proactive message produced live by the lightweight LLM after
/// a turn or loop event (axis F). Same volatility contract as
/// [`CachedCrtPhrase`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CachedProactive {
    pub text: String,
    pub generated_at: Option<DateTime<Utc>>,
}
