//! Structured prompt composition for the Vivling brain.
//!
//! The Vivling accumulates four very different kinds of signal:
//! identity bias, distilled patterns, recent observed work, and
//! the live status of the host TUI. Older code mixed them all into
//! a single memory digest, which made the brain treat stale loop
//! counters as proof of the current system state.
//!
//! `compose_brain_prompt` keeps each layer in its own section so the
//! brain can reason about *now* (live state + payload) without being
//! biased into a stale conclusion by historical counters.
//!
//! The `Live state contract:` and `Learned memory:` labels are kept
//! as substring anchors so existing TUI tests and downstream tooling
//! stay green; the layout gains explicit Identity / Stable memory /
//! Recent observed work / Live state (now) / Stale signals sections.
use super::super::model::Stage;
use super::super::model::VivlingState;
use super::super::model::truncate_summary;
use super::VivlingLiveContext;

#[derive(Debug, Clone)]
pub(crate) enum BrainPromptKind<'a> {
    Assist,
    Chat,
    LoopTick {
        label: &'a str,
        goal: &'a str,
        prompt_text: &'a str,
        auto_remove_on_completion: bool,
    },
}

impl<'a> BrainPromptKind<'a> {
    fn live_state_contract(&self) -> &'static str {
        match self {
            BrainPromptKind::Assist => {
                "Live state is unknown unless the task explicitly provides it. Treat learned memory as bias and history, not proof that the current system is blocked, idle, active, or complete."
            }
            BrainPromptKind::Chat => {
                "Live state is unknown unless the user message explicitly provides it. Treat learned memory as bias and history, not proof that the current system is blocked, idle, active, or complete."
            }
            BrainPromptKind::LoopTick { .. } => {
                "Live state is whatever the Live state (now) section reports plus the loop tick payload. Treat learned memory as bias and history, not proof that the current loop is blocked, idle, active, or complete."
            }
        }
    }

    fn payload_label(&self) -> &'static str {
        match self {
            BrainPromptKind::Assist => "Task",
            BrainPromptKind::Chat => "User message",
            BrainPromptKind::LoopTick { .. } => "Loop tick",
        }
    }
}

pub(crate) fn compose_brain_prompt(
    state: &VivlingState,
    kind: BrainPromptKind<'_>,
    payload: &str,
    live: Option<&VivlingLiveContext>,
) -> Result<String, String> {
    if matches!(kind, BrainPromptKind::Assist) && state.stage() != Stage::Adult {
        return Err("`/vivling assist ...` unlocks only at level 60.".to_string());
    }
    if matches!(kind, BrainPromptKind::Assist) && !state.brain_enabled {
        return Err("Enable the Vivling brain first with `/vivling brain on`.".to_string());
    }
    let profile = state.brain_profile.as_deref().ok_or_else(|| {
        "Set a Vivling brain profile first with `/vivling model ...`.".to_string()
    })?;
    let payload = payload.trim();
    if payload.is_empty() {
        return Err(match kind {
            BrainPromptKind::Assist => "Usage: /vivling assist <task>".to_string(),
            BrainPromptKind::Chat => "Usage: /vl <message>".to_string(),
            BrainPromptKind::LoopTick { .. } => "Loop tick payload is empty.".to_string(),
        });
    }

    let mut sections: Vec<String> = Vec::new();
    sections.push(identity_section(state, profile));
    sections.push(stable_memory_section(state));
    sections.push(recent_observed_work_section(state));
    sections.push(legacy_learned_memory_section(state));
    sections.push(live_state_section(live));
    if let Some(stale) = stale_signals_section(state) {
        sections.push(stale);
    }
    sections.push(format!(
        "Live state contract:\n{}",
        kind.live_state_contract()
    ));
    if let BrainPromptKind::LoopTick {
        label,
        goal,
        prompt_text,
        auto_remove_on_completion,
    } = &kind
    {
        sections.push(format!(
            "Loop:\n- label: {label}\n- goal: {goal}\n- prompt: {prompt_text}\n- auto_remove_on_completion: {auto_remove_on_completion}\n- response contract: return strict JSON with fields status, message, optional loop_action; status is one of progress|blocked|done; loop_action.action may be none|disable|remove|trigger|update; for update you may optionally provide interval, goal, prompt, enabled.",
        ));
    }
    sections.push(format!("{}:\n{}", kind.payload_label(), payload));

    Ok(sections.join("\n\n"))
}

fn identity_section(state: &VivlingState, profile: &str) -> String {
    format!(
        "Vivling identity:\n- id: {}\n- name: {}\n- profile: {}\n- stage: {}\n- dominant role: {}\n- tone: {}\n- verification bias: {}\n- caution bias: {}\n- question bias: {}",
        state.vivling_id,
        state.name,
        profile,
        state.stage().label(),
        state.dominant_archetype().label(),
        state.identity_profile.tone,
        state.identity_profile.verification_bias,
        state.identity_profile.caution_bias,
        state.identity_profile.question_bias,
    )
}

fn stable_memory_section(state: &VivlingState) -> String {
    if state.distilled_summaries.is_empty() {
        return "Stable memory (distilled patterns):\n- (none yet — patterns still forming)"
            .to_string();
    }
    let mut lines = vec!["Stable memory (distilled patterns):".to_string()];
    for entry in state.distilled_summaries.iter().take(3) {
        lines.push(format!(
            "- {} [{}] x{}: {}",
            entry.topic,
            entry.archetype.label(),
            entry.observations,
            truncate_summary(&entry.summary, 96),
        ));
    }
    lines.join("\n")
}

fn recent_observed_work_section(state: &VivlingState) -> String {
    let observed: Vec<_> = state
        .work_memory
        .iter()
        .rev()
        .filter(|entry| entry.kind != "live_context")
        .take(5)
        .collect();
    if observed.is_empty() {
        return "Recent observed work:\n- (no observed work captured yet)".to_string();
    }
    let mut lines = vec!["Recent observed work:".to_string()];
    for entry in observed {
        lines.push(format!(
            "- {} [{}]: {}",
            entry.kind,
            entry.archetype.label(),
            truncate_summary(&entry.summary, 96),
        ));
    }
    lines.join("\n")
}

fn legacy_learned_memory_section(state: &VivlingState) -> String {
    let last = state
        .last_work_summary
        .as_deref()
        .map(|summary| truncate_summary(summary, 96))
        .unwrap_or_else(|| "No recent work summary yet.".to_string());
    format!(
        "Learned memory:\n- recent summary: {}\n- level {} · active_days {} · turns_observed {} · loop_exposure {}",
        last, state.level, state.active_work_days, state.turns_observed, state.loop_exposure,
    )
}

fn live_state_section(live: Option<&VivlingLiveContext>) -> String {
    match live.and_then(live_context_lines) {
        Some(lines) => format!("Live state (now):\n{lines}"),
        None => "Live state (now):\n- unknown — only the payload below reflects current state"
            .to_string(),
    }
}

fn live_context_lines(ctx: &VivlingLiveContext) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    if let Some(value) = clean(&ctx.run_state) {
        lines.push(format!("- run state: {value}"));
    }
    if let Some(value) = clean(&ctx.active_agent_label) {
        lines.push(format!("- active agent: {value}"));
    }
    if let Some(value) = clean(&ctx.task_progress) {
        lines.push(format!("- task progress: {value}"));
    }
    if let Some(value) = clean(&ctx.model) {
        lines.push(format!("- model: {value}"));
    }
    if let Some(value) = clean(&ctx.git_branch) {
        lines.push(format!("- git branch: {value}"));
    }
    if let Some(value) = clean(&ctx.cwd) {
        lines.push(format!("- cwd: {value}"));
    }
    if let Some(value) = clean(&ctx.thread_title) {
        lines.push(format!("- thread: {value}"));
    }
    if lines.is_empty() {
        return None;
    }
    Some(lines.join("\n"))
}

fn clean(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn stale_signals_section(state: &VivlingState) -> Option<String> {
    let blocked_total = state
        .loop_runtime_blocks
        .saturating_add(state.loop_admin_churn);
    if blocked_total == 0
        && state.loop_blocked_review == 0
        && state.loop_blocked_side == 0
        && state.loop_blocked_busy == 0
    {
        return None;
    }
    let mut lines = vec![
        "Stale signals (history, not proof of current state):".to_string(),
        format!(
            "- loop_runtime_blocks: {} (cumulative)",
            state.loop_runtime_blocks
        ),
        format!(
            "- loop_admin_churn: {} (cumulative)",
            state.loop_admin_churn
        ),
    ];
    if state.loop_blocked_review > 0 {
        lines.push(format!(
            "- loop_blocked_review: {} (cumulative)",
            state.loop_blocked_review
        ));
    }
    if state.loop_blocked_side > 0 {
        lines.push(format!(
            "- loop_blocked_side: {} (cumulative)",
            state.loop_blocked_side
        ));
    }
    if state.loop_blocked_busy > 0 {
        lines.push(format!(
            "- loop_blocked_busy: {} (cumulative)",
            state.loop_blocked_busy
        ));
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::super::super::model::VivlingWorkMemoryEntry;
    use super::super::super::model::WorkArchetype;
    use super::*;
    use chrono::Utc;

    fn adult_state_with_profile() -> VivlingState {
        let mut state = VivlingState::default();
        state.hatched = true;
        state.name = "Nilo".to_string();
        state.vivling_id = "viv-test".to_string();
        state.level = 60;
        state.brain_enabled = true;
        state.brain_profile = Some("vivling-spark".to_string());
        state
    }

    #[test]
    fn live_state_unknown_when_context_missing() {
        let state = adult_state_with_profile();
        let prompt = compose_brain_prompt(&state, BrainPromptKind::Assist, "review blocker", None)
            .expect("prompt");
        assert!(prompt.contains("Live state (now):"));
        assert!(prompt.contains("unknown — only the payload below reflects current state"));
        assert!(prompt.contains("Live state contract:"));
        assert!(prompt.contains("Task:\nreview blocker"));
        assert!(prompt.contains("Learned memory:"));
    }

    #[test]
    fn live_state_includes_live_context_fields() {
        let state = adult_state_with_profile();
        let live = VivlingLiveContext {
            run_state: Some("Working".to_string()),
            active_agent_label: Some("worker".to_string()),
            task_progress: Some("12% (3/25)".to_string()),
            git_branch: Some("develop".to_string()),
            cwd: Some("/home/dag/Dev/60_toolchains/codex-vl".to_string()),
            ..Default::default()
        };
        let prompt = compose_brain_prompt(&state, BrainPromptKind::Chat, "ciao", Some(&live))
            .expect("prompt");
        assert!(prompt.contains("- run state: Working"));
        assert!(prompt.contains("- active agent: worker"));
        assert!(prompt.contains("- task progress: 12% (3/25)"));
        assert!(prompt.contains("- git branch: develop"));
        assert!(prompt.contains("User message:\nciao"));
        // Live state contract still present so the brain knows memory is bias.
        assert!(prompt.contains("Live state contract:"));
    }

    #[test]
    fn recent_observed_work_excludes_live_context_capsules() {
        let mut state = adult_state_with_profile();
        let now = Utc::now();
        state.work_memory.push(VivlingWorkMemoryEntry {
            kind: "turn".to_string(),
            summary: "shipped a small fix".to_string(),
            archetype: WorkArchetype::Builder,
            weight: 14,
            created_at: now,
        });
        state.work_memory.push(VivlingWorkMemoryEntry {
            kind: "live_context".to_string(),
            summary: "live context: state Working; active worker".to_string(),
            archetype: WorkArchetype::Operator,
            weight: 0,
            created_at: now,
        });

        let prompt = compose_brain_prompt(&state, BrainPromptKind::Assist, "next step", None)
            .expect("prompt");
        // The volatile live capsule must not pretend to be observed work.
        assert!(!prompt.contains("- live_context ["));
        assert!(prompt.contains("- turn [builder]: shipped a small fix"));
    }

    #[test]
    fn stale_signals_section_appears_only_with_history() {
        let state = adult_state_with_profile();
        let prompt = compose_brain_prompt(&state, BrainPromptKind::Assist, "next step", None)
            .expect("prompt");
        assert!(!prompt.contains("Stale signals"));

        let mut noisy = adult_state_with_profile();
        noisy.loop_runtime_blocks = 3;
        noisy.loop_blocked_review = 2;
        let prompt = compose_brain_prompt(&noisy, BrainPromptKind::Assist, "next step", None)
            .expect("prompt");
        assert!(prompt.contains("Stale signals (history, not proof of current state):"));
        assert!(prompt.contains("loop_runtime_blocks: 3"));
        assert!(prompt.contains("loop_blocked_review: 2"));
    }

    #[test]
    fn loop_tick_prompt_includes_live_state_and_loop_section() {
        let state = adult_state_with_profile();
        let live = VivlingLiveContext {
            run_state: Some("Working".to_string()),
            ..Default::default()
        };
        let prompt = compose_brain_prompt(
            &state,
            BrainPromptKind::LoopTick {
                label: "babysit-pr",
                goal: "watch PR feedback",
                prompt_text: "check the PR queue",
                auto_remove_on_completion: false,
            },
            "check the PR queue",
            Some(&live),
        )
        .expect("prompt");
        assert!(prompt.contains("Loop:\n- label: babysit-pr"));
        assert!(prompt.contains("- goal: watch PR feedback"));
        assert!(prompt.contains("- run state: Working"));
        assert!(prompt.contains("Loop tick:\ncheck the PR queue"));
        assert!(prompt.contains("response contract: return strict JSON"));
    }

    #[test]
    fn assist_requires_adult_brain() {
        let mut state = adult_state_with_profile();
        state.level = 30;
        let err = compose_brain_prompt(&state, BrainPromptKind::Assist, "task", None)
            .expect_err("must error");
        assert!(err.contains("level 60"));
    }

    #[test]
    fn chat_does_not_require_adult_or_brain_enabled() {
        let mut state = adult_state_with_profile();
        state.level = 30;
        state.brain_enabled = false;
        let prompt = compose_brain_prompt(&state, BrainPromptKind::Chat, "ciao", None)
            .expect("chat prompt should be allowed without adult brain");
        assert!(prompt.contains("User message:\nciao"));
    }
}
