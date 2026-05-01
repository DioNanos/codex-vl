use super::*;

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
