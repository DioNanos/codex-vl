use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

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
    #[serde(default)]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) summary: String,
    #[serde(default)]
    pub(crate) archetype: WorkArchetype,
    #[serde(default)]
    pub(crate) weight: u64,
    #[serde(default)]
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct VivlingDistilledSummary {
    #[serde(default)]
    pub(crate) topic: String,
    #[serde(default)]
    pub(crate) summary: String,
    #[serde(default)]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) archetype: WorkArchetype,
    #[serde(default)]
    pub(crate) total_weight: u64,
    #[serde(default)]
    pub(crate) observations: u64,
    #[serde(default)]
    pub(crate) first_seen_at: DateTime<Utc>,
    #[serde(default)]
    pub(crate) last_seen_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct VivlingMentalPath {
    #[serde(default)]
    pub(crate) from: String,
    #[serde(default)]
    pub(crate) to: String,
    #[serde(default)]
    pub(crate) weight: u64,
    #[serde(default)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
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
