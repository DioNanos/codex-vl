use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VivlingBrainRequestKind {
    Assist,
    Chat,
}

/// Where the brain backend resolution should look for the model and
/// provider when this Vivling speaks.
///
/// Memory V2 §8.1 (P0.2) — the inheritance rule depends only on
/// `state.brain_profile`. `brain_enabled` stays a pure feature gate and
/// is intentionally **not** folded into this enum.
///
/// - `SessionDefault`: the Vivling has no explicit profile assigned
///   (`brain_profile == None`). Inheritance kicks in: the dispatcher
///   reads `config.model` from the active session and uses it as-is,
///   without registering any synthetic profile on disk.
/// - `Profile(p)`: the user pinned a brain via `/vivling model <p>`.
///   The dispatcher rebuilds a `Config` through the standard
///   `ConfigBuilder` with `config_profile = Some(p)`, picking up the
///   profile's model/provider/effort overrides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BrainTarget {
    SessionDefault,
    Profile(String),
}

/// Pure helper mapping `state.brain_profile` to a `BrainTarget`. Kept
/// separate from the runtime call sites so the inheritance rule is
/// trivially testable in isolation.
pub(crate) fn brain_target_from_profile(profile: Option<&str>) -> BrainTarget {
    match profile {
        Some(p) => BrainTarget::Profile(p.to_string()),
        None => BrainTarget::SessionDefault,
    }
}

/// Memory V2 Step 12.B.C — pick the LLM brain target for the
/// `/vl` chat + Expression channels.
///
/// Distinct from [`brain_target_from_profile`] because `/vl` chat is
/// decoupled from `brain_enabled` (DAG: "STESSO MODELLO CHAT — se
/// parto con codex-vl uso il modello /model; A MENO che non ho
/// settato il brain ALLORA usa il modello BRAIN"). A pinned profile
/// overrides the session model *only* when the user has explicitly
/// turned the brain on; otherwise we always fall back to whatever
/// `/model` (or the wrapper) selected for the session.
pub(crate) fn resolve_expression_target(
    brain_enabled: bool,
    brain_profile: Option<&str>,
) -> BrainTarget {
    match (
        brain_enabled,
        brain_profile.and_then(|p| {
            let trimmed = p.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }),
    ) {
        (true, Some(name)) => BrainTarget::Profile(name),
        _ => BrainTarget::SessionDefault,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VivlingAssistRequest {
    pub(crate) vivling_id: String,
    pub(crate) vivling_name: String,
    pub(crate) brain_target: BrainTarget,
    pub(crate) kind: VivlingBrainRequestKind,
    pub(crate) task: String,
    pub(crate) prompt_context: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VivlingLoopTickRequest {
    pub(crate) vivling_id: String,
    pub(crate) vivling_name: String,
    pub(crate) brain_target: BrainTarget,
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

#[cfg(test)]
mod brain_target_tests {
    use super::BrainTarget;
    use super::brain_target_from_profile;

    #[test]
    fn none_profile_resolves_to_session_default() {
        assert_eq!(brain_target_from_profile(None), BrainTarget::SessionDefault);
    }

    #[test]
    fn some_profile_resolves_to_profile_variant() {
        assert_eq!(
            brain_target_from_profile(Some("opus")),
            BrainTarget::Profile("opus".to_string())
        );
    }

    #[test]
    fn empty_profile_string_is_not_normalised_to_session_default() {
        // Empty-string profile is treated as an explicit (though
        // probably broken) profile name, not as "missing profile".
        // ConfigBuilder will surface the real error downstream; the
        // mapping helper must stay rule-pure.
        assert_eq!(
            brain_target_from_profile(Some("")),
            BrainTarget::Profile(String::new())
        );
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VivlingCommandOutcome {
    Message(String),
    OpenCard(VivlingPanelData),
    OpenUpgrade(VivlingPanelData),
    DispatchAssist(VivlingAssistRequest),
    /// Memory V2 Step 12.B.H — `/vivling crt-brain refresh` outcome.
    /// chatwidget dispatcher emits a forced Expression refresh via
    /// `maybe_trigger_vivling_expression_refresh_forced()` and
    /// surfaces a user-visible status message.
    CrtBrainRefresh,
    PersistBrainProfile(VivlingBrainProfileRequest),
    /// codex-vl iter 1C: emitted by `/vivling spawn`. Carries both the
    /// chat-history message (L1) and the ZED Lineage panel (L2).
    SpawnNarration {
        message: String,
        panel: VivlingPanelData,
    },
}
