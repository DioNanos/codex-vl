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
