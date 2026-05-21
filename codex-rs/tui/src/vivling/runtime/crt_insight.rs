//! CRT bubble insight layer.
//!
//! Distils one short, high-signal phrase from the live `VivlingState` for the
//! 3-row CRT strip. The bubble already truncates and styles the phrase, so
//! the goal here is just to pick the most useful thing to say right now —
//! never numeric labels, never long sentences.

use super::super::model::Stage;
use super::super::model::VivlingWorkMemoryEntry;
use super::super::model::WorkArchetype;
use super::VivlingLiveContext;
use super::VivlingState;

/// Maximum chars in a generated phrase. Speech bubble does its own further
/// trimming; this is the upstream cap so we stay readable on narrow widths.
const INSIGHT_MAX_CHARS: usize = 28;

/// Compute the bubble text for the CRT strip. Returns `None` only when the
/// Vivling has nothing useful to say (no signals, no last message).
///
/// Wrapper around [`compute_insight_at`] that uses `Utc::now()` as the
/// wall clock; tests should call `compute_insight_at` directly to pin
/// the TTL check.
pub(crate) fn compute_insight(
    state: &VivlingState,
    live_context: Option<&VivlingLiveContext>,
) -> Option<String> {
    compute_insight_at(state, live_context, chrono::Utc::now())
}

/// Memory V2 Step 12.B.D.1 — clock-injectable variant. Lets the CRT
/// renderer read the cached LLM phrase produced by the Expression
/// channel (Step 12.B.B `try_reserve_llm_call` + 12.B.D.2 dispatch)
/// without making the chain non-deterministic in tests.
///
/// Cached phrase precedence is **middle of the chain**: safety
/// templates (`brain_error_phrase`, `blocked_loop_phrase`,
/// `active_loop_phrase`) always win so a stale LLM bubble cannot
/// mask a current brain error or blocked loop. Below them, the fresh
/// cached entry overrides the template fallbacks; once TTL expires
/// the chain falls back to `proactive_next_phrase` and beyond.
pub(crate) fn compute_insight_at(
    state: &VivlingState,
    live_context: Option<&VivlingLiveContext>,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    if let Some(phrase) = brain_error_phrase(state) {
        return Some(phrase);
    }
    if let Some(phrase) = blocked_loop_phrase(state) {
        return Some(phrase);
    }
    if let Some(phrase) = active_loop_phrase(state) {
        return Some(phrase);
    }
    if let Some(phrase) = cached_crt_phrase_at(state, now) {
        return Some(phrase);
    }
    if let Some(phrase) = proactive_next_phrase_at(state, now) {
        return Some(phrase);
    }
    if let Some(phrase) = recent_memory_phrase(state) {
        return Some(phrase);
    }
    if let Some(phrase) = last_work_summary_phrase(state) {
        return Some(phrase);
    }
    if let Some(phrase) = live_context.and_then(VivlingLiveContext::crt_phrase) {
        return Some(phrase);
    }
    if let Some(phrase) = focus_phrase(state) {
        return Some(phrase);
    }
    if let Some(phrase) = learning_phrase(state) {
        return Some(phrase);
    }
    state
        .last_message
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(compact_freeform)
}

fn brain_error_phrase(state: &VivlingState) -> Option<String> {
    if state
        .brain_last_error
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Some("check brain error".to_string());
    }
    None
}

fn blocked_loop_phrase(state: &VivlingState) -> Option<String> {
    let latest = state
        .work_memory
        .iter()
        .rev()
        .find(|entry| entry.kind != "live_context");
    if let Some(entry) = latest {
        match entry.kind.as_str() {
            "loop_blocked_review" => return Some("review gate blocking".to_string()),
            "loop_blocked_side" => return Some("side work blocking".to_string()),
            "loop_blocked_busy" => return Some("busy - wait for state".to_string()),
            _ => {}
        }
    }
    if state.loop_admin_churn >= 3 && state.loop_runtime_submissions == 0 {
        return Some("churn - verify state".to_string());
    }
    None
}

fn active_loop_phrase(state: &VivlingState) -> Option<String> {
    let latest = state
        .work_memory
        .iter()
        .rev()
        .find(|entry| entry.kind != "live_context")?;
    if latest.kind != "loop_runtime" || !latest.summary.to_ascii_lowercase().contains("submitted") {
        return None;
    }
    Some(
        match state.stage() {
            Stage::Baby => "loop landed!",
            Stage::Juvenile => "clean - verify next",
            Stage::Adult => "landed - check fallout",
        }
        .to_string(),
    )
}

/// Memory V2 Step 12.B.D.1 — surface the LLM-written CRT bubble when
/// it is still within its TTL. Returns `None` when the cache is
/// absent, when the text trims to empty, or when `ttl_expires_at <=
/// now`. The chain caller (`compute_insight_at`) keeps this slot in
/// the *middle* of the priority list so safety templates above
/// (brain error / blocked loop / active loop) cannot be masked by a
/// stale or wrong cached phrase.
fn cached_crt_phrase_at(
    state: &VivlingState,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    let cached = state.cached_crt_phrase.as_ref()?;
    let expires = cached.ttl_expires_at?;
    if expires <= now {
        return None;
    }
    let text = cached.text.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

/// Memory V2 Step 12.B.D.1 — clock-injectable variant of
/// [`proactive_next_phrase`]. Prefers the fresh `cached_proactive`
/// payload (Expression channel) over the template fallback;
/// stale/empty cache falls through to the existing deterministic
/// template logic.
fn proactive_next_phrase_at(
    state: &VivlingState,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    if let Some(cached) = state.cached_proactive.as_ref() {
        let fresh = cached.ttl_expires_at.map(|exp| exp > now).unwrap_or(false);
        if fresh {
            let text = cached.text.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    proactive_next_phrase(state)
}

fn proactive_next_phrase(state: &VivlingState) -> Option<String> {
    let raw = state.last_work_summary.as_deref()?.trim();
    if raw.is_empty() {
        return None;
    }
    let lower = raw.to_ascii_lowercase();
    if lower.contains("merge") || lower.contains("rebase") || lower.contains("conflict") {
        return Some("merge: protect vl hooks".to_string());
    }
    if lower.contains("plan") && (lower.contains("approved") || lower.contains("accepted")) {
        return Some("plan set - do one slice".to_string());
    }
    if lower.contains("review") || lower.contains("audit") {
        return Some("review: name the risk".to_string());
    }
    if lower.contains("test") || lower.contains("check") {
        return Some("tests: isolate failure".to_string());
    }
    if lower.contains("install") || lower.contains("release") || lower.contains("publish") {
        return Some("release: verify path".to_string());
    }
    match state.dominant_archetype() {
        WorkArchetype::Builder if state.work_affinities.builder > 0 => {
            Some("build one narrow change".to_string())
        }
        WorkArchetype::Reviewer if state.work_affinities.reviewer > 0 => {
            Some("prove the blocker".to_string())
        }
        WorkArchetype::Researcher if state.work_affinities.researcher > 0 => {
            Some("clarify one unknown".to_string())
        }
        WorkArchetype::Operator if state.work_affinities.operator > 0 => {
            Some("check state, then act".to_string())
        }
        _ => None,
    }
}

fn recent_memory_phrase(state: &VivlingState) -> Option<String> {
    state
        .work_memory
        .iter()
        .rev()
        .find(|entry| entry.kind != "live_context")
        .and_then(memory_entry_phrase)
}

fn memory_entry_phrase(entry: &VivlingWorkMemoryEntry) -> Option<String> {
    let summary = entry.summary.trim();
    if summary.is_empty() {
        return None;
    }
    let lower = summary.to_ascii_lowercase();
    if entry.kind.contains("blocked") || lower.contains("blocked") {
        return Some("memory: loop blocked".to_string());
    }
    if entry.kind == "loop_runtime" && lower.contains("submitted") {
        return Some("memory: loop submitted".to_string());
    }
    if lower.contains("verify") || lower.contains("verified") || lower.contains("smoke") {
        return Some("memory: verify path".to_string());
    }
    if lower.contains("release") || lower.contains("publish") {
        return Some("memory: release path".to_string());
    }
    if lower.contains("merge") || lower.contains("rebase") || lower.contains("conflict") {
        return Some("memory: merge risk".to_string());
    }
    if lower.contains("review") || lower.contains("audit") {
        return Some("memory: review risk".to_string());
    }
    if lower.contains("test") || lower.contains("check") {
        return Some("memory: test signal".to_string());
    }
    None
}

fn last_work_summary_phrase(state: &VivlingState) -> Option<String> {
    let raw = state.last_work_summary.as_deref()?.trim();
    if raw.is_empty() {
        return None;
    }
    compact_summary(raw)
}

fn focus_phrase(state: &VivlingState) -> Option<String> {
    let totals = &state.work_affinities;
    let any = totals.builder | totals.reviewer | totals.researcher | totals.operator;
    if any == 0 {
        return None;
    }
    let label = match state.dominant_archetype() {
        WorkArchetype::Builder => "builder",
        WorkArchetype::Reviewer => "reviewer",
        WorkArchetype::Researcher => "researcher",
        WorkArchetype::Operator => "operator",
    };
    Some(format!("focus {label}"))
}

fn learning_phrase(state: &VivlingState) -> Option<String> {
    if state.distilled_summaries.is_empty() {
        return None;
    }
    if state
        .distilled_summaries
        .iter()
        .any(|entry| entry.topic.contains("release") || entry.summary.contains("release"))
    {
        return Some("learning release flow".to_string());
    }
    Some("learning patterns".to_string())
}

/// Map a verbose work summary to a short, plain phrase. Falls back to a
/// trimmed prefix when no keyword matches.
fn compact_summary(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let phrase =
        if lower.contains("plan") && (lower.contains("approved") || lower.contains("accepted")) {
            "plan set - do one slice"
        } else if lower.contains("plan") && lower.contains("proposed") {
            "plan ready"
        } else if lower.contains("blocked") {
            "blocked - name the gate"
        } else if lower.contains("build") {
            "build: verify result"
        } else if lower.contains("test") {
            "tests: isolate failure"
        } else if lower.contains("review") {
            "review: name the risk"
        } else if lower.contains("release") {
            "release: verify path"
        } else if lower.contains("doc") {
            "docs: keep source tight"
        } else if lower.contains("merge") || lower.contains("rebase") {
            "merge: protect vl hooks"
        } else if lower.contains("verif") || lower.contains("verified") {
            "verify before widening"
        } else if lower.contains("complet") || lower.contains("done") {
            "done - check fallout"
        } else if lower.contains("loop") {
            "loop: verify next wake"
        } else {
            return compact_freeform(raw);
        };
    Some(phrase.to_string())
}

fn compact_freeform(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("check blocked") || (lower.contains("blocked") && lower.contains("loop")) {
        return Some("blocked loop needs review".to_string());
    }
    let trimmed = strip_numeric_noise(raw);
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() <= INSIGHT_MAX_CHARS {
        return Some(trimmed);
    }
    let cut = INSIGHT_MAX_CHARS.saturating_sub(2);
    let mut out: String = trimmed.chars().take(cut).collect();
    out.push_str("..");
    Some(out)
}

fn strip_numeric_noise(raw: &str) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_digit() || ch.is_control() {
            pending_space = true;
            continue;
        }
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::super::model::ADULT_LEVEL;
    use super::super::super::model::JUVENILE_LEVEL;
    use super::super::super::model::VivlingState;
    use super::super::super::model::VivlingWorkMemoryEntry;
    use super::super::super::model::WorkAffinitySet;
    use super::super::super::model::WorkArchetype;
    use super::*;
    use chrono::Utc;

    fn state_with_message(msg: &str) -> VivlingState {
        VivlingState {
            last_message: Some(msg.to_string()),
            ..Default::default()
        }
    }

    fn loop_submitted_memory() -> VivlingWorkMemoryEntry {
        VivlingWorkMemoryEntry {
            kind: "loop_runtime".to_string(),
            summary: "loop trigger `codex-vl` (scheduled, status submitted, agent)".to_string(),
            archetype: WorkArchetype::Operator,
            weight: 14,
            created_at: Utc::now(),
        }
    }

    fn loop_blocked_memory(kind: &str) -> VivlingWorkMemoryEntry {
        VivlingWorkMemoryEntry {
            kind: kind.to_string(),
            summary: format!("loop trigger `codex-vl` (pending, status {kind}, agent)"),
            archetype: WorkArchetype::Operator,
            weight: 0,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn blocked_signal_beats_generic_last_message() {
        let mut state = state_with_message("greets the operator");
        state.loop_blocked_review = 2;
        state.loop_runtime_submissions = 0;
        state
            .work_memory
            .push(loop_blocked_memory("loop_blocked_review"));
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "review gate blocking");
    }

    #[test]
    fn blocked_busy_signal_names_waiting_state() {
        let mut state = state_with_message("greets the operator");
        state.loop_blocked_busy = 2;
        state.loop_runtime_submissions = 0;
        state
            .work_memory
            .push(loop_blocked_memory("loop_blocked_busy"));
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "busy - wait for state");
    }

    #[test]
    fn blocked_signal_beats_live_context_for_actionability() {
        let mut state = state_with_message("greets the operator");
        state.loop_blocked_review = 2;
        state.loop_runtime_submissions = 0;
        state
            .work_memory
            .push(loop_blocked_memory("loop_blocked_review"));
        let context = VivlingLiveContext {
            active_agent_label: Some("main".to_string()),
            task_progress: Some("build running".to_string()),
            ..Default::default()
        };

        let phrase = compute_insight(&state, Some(&context)).expect("insight");
        assert_eq!(phrase, "review gate blocking");
    }

    #[test]
    fn stale_check_blocked_message_is_normalized() {
        let state = state_with_message("< check blocked");
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "blocked loop needs review");
    }

    #[test]
    fn churn_without_runtime_submissions_surfaces_verify_state() {
        let mut state = state_with_message("noticed loop add `foo`");
        state.loop_admin_churn = 5;
        state.loop_runtime_submissions = 0;
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "churn - verify state");
    }

    #[test]
    fn brain_error_outranks_other_signals() {
        let mut state = state_with_message("everything is fine");
        state.brain_last_error = Some("model 500".to_string());
        state.loop_runtime_submissions = 5;
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "check brain error");
    }

    #[test]
    fn active_loop_signal_surfaces_landing() {
        let mut state = state_with_message("noticed loop");
        state.loop_runtime_submissions = 3;
        state.loop_runtime_blocks = 0;
        state.work_memory.push(loop_submitted_memory());
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "loop landed!");
    }

    #[test]
    fn old_loop_submission_counters_do_not_stick_without_recent_runtime_memory() {
        let mut state = state_with_message("noticed loop");
        state.loop_runtime_submissions = 3;
        state.loop_runtime_blocks = 0;
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "noticed loop");
    }

    #[test]
    fn old_blocked_counters_do_not_stick_without_recent_block_memory() {
        let mut state = state_with_message("greets the operator");
        state.loop_blocked_busy = 4;
        state.loop_runtime_submissions = 0;
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "greets the operator");
    }

    #[test]
    fn newer_work_memory_beats_old_blocked_memory() {
        let mut state = state_with_message("greets the operator");
        state.loop_blocked_busy = 4;
        state
            .work_memory
            .push(loop_blocked_memory("loop_blocked_busy"));
        state.work_memory.push(VivlingWorkMemoryEntry {
            kind: "turn".to_string(),
            summary: "turn completed: verified the state after loop cleanup".to_string(),
            archetype: WorkArchetype::Operator,
            weight: 14,
            created_at: Utc::now(),
        });
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "memory: verify path");
    }

    #[test]
    fn active_loop_signal_uses_stage_voice() {
        let mut juvenile = state_with_message("noticed loop");
        juvenile.level = JUVENILE_LEVEL;
        juvenile.work_memory.push(loop_submitted_memory());
        let phrase = compute_insight(&juvenile, None).expect("insight");
        assert_eq!(phrase, "clean - verify next");

        let mut adult = state_with_message("noticed loop");
        adult.level = ADULT_LEVEL;
        adult.work_memory.push(loop_submitted_memory());
        let phrase = compute_insight(&adult, None).expect("insight");
        assert_eq!(phrase, "landed - check fallout");
    }

    #[test]
    fn newer_work_memory_beats_old_loop_submission_memory() {
        let mut state = state_with_message("noticed loop");
        state.work_memory.push(loop_submitted_memory());
        state.work_memory.push(VivlingWorkMemoryEntry {
            kind: "turn".to_string(),
            summary: "turn completed: tests verified".to_string(),
            archetype: WorkArchetype::Builder,
            weight: 14,
            created_at: Utc::now(),
        });
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "memory: verify path");
    }

    #[test]
    fn blocked_loop_beats_landed_loop_signal() {
        let mut state = state_with_message("noticed loop");
        state.loop_runtime_submissions = 2;
        state.loop_runtime_blocks = 3;
        state.loop_blocked_review = 3;
        state.work_memory.push(loop_submitted_memory());
        state
            .work_memory
            .push(loop_blocked_memory("loop_blocked_review"));
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "review gate blocking");
    }

    #[test]
    fn recent_memory_beats_live_context_and_ignores_live_context_capsules() {
        let mut state = state_with_message("greets");
        state.work_memory.push(VivlingWorkMemoryEntry {
            kind: "live_context".to_string(),
            summary: "live context: state Working; cwd codex-vl".to_string(),
            archetype: WorkArchetype::Operator,
            weight: 0,
            created_at: Utc::now(),
        });
        state.work_memory.push(VivlingWorkMemoryEntry {
            kind: "turn".to_string(),
            summary: "turn completed: verified the local build path".to_string(),
            archetype: WorkArchetype::Operator,
            weight: 14,
            created_at: Utc::now(),
        });
        let context = VivlingLiveContext {
            active_agent_label: Some("main".to_string()),
            ..Default::default()
        };

        let phrase = compute_insight(&state, Some(&context)).expect("insight");
        assert_eq!(phrase, "memory: verify path");
    }

    #[test]
    fn last_work_summary_is_compacted() {
        let mut state = state_with_message("greets");
        state.last_work_summary = Some(
            "turn completed: review the README and audit the change carefully across files"
                .to_string(),
        );
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "review: name the risk");
        assert!(phrase.chars().count() <= INSIGHT_MAX_CHARS);
    }

    #[test]
    fn plan_approval_summary_has_specific_phrase() {
        let mut state = state_with_message("greets");
        state.last_work_summary = Some("plan approved: implement the selected fix".to_string());
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "plan set - do one slice");
    }

    #[test]
    fn fallback_keeps_last_message_when_no_signals() {
        let state = state_with_message("watching upstream branch");
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "watching upstream branch");
    }

    #[test]
    fn fallback_strips_numeric_noise() {
        let state = state_with_message("Lv 5 work 64 loops 3");
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "Lv work loops");
        assert!(!phrase.chars().any(|c| c.is_ascii_digit()));
    }

    #[test]
    fn unclassified_summary_strips_numeric_noise() {
        let mut state = state_with_message("greets");
        state.last_work_summary = Some("Lv 5 signal 64 ticks 3".to_string());
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "Lv signal ticks");
        assert!(!phrase.chars().any(|c| c.is_ascii_digit()));
    }

    #[test]
    fn proactive_work_summary_beats_live_context_echo() {
        let mut state = state_with_message("watching completed turns closely");
        state.last_work_summary = Some("turn completed: tests passed".to_string());
        let context = VivlingLiveContext {
            active_agent_label: Some("Robie [worker]".to_string()),
            run_state: Some("Working".to_string()),
            ..Default::default()
        };

        let phrase = compute_insight(&state, Some(&context)).expect("insight");
        assert_eq!(phrase, "tests: isolate failure");
    }

    #[test]
    fn empty_last_message_with_no_signals_returns_none() {
        let state = VivlingState::default();
        assert!(compute_insight(&state, None).is_none());
    }

    #[test]
    fn focus_phrase_uses_dominant_archetype_when_affinities_present() {
        let mut state = VivlingState::default();
        // Reviewer must dominate even after the species bias (Syllo defaults
        // to BuilderResearch); pick a large enough value to swamp the bias.
        state.work_affinities = WorkAffinitySet {
            builder: 0,
            reviewer: 200,
            researcher: 0,
            operator: 0,
        };
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "focus reviewer");
    }

    #[test]
    fn no_dashboard_numeric_labels_are_emitted() {
        const FORBIDDEN: &[&str] = &[
            "EN", "HU", "LOOP0", "LOOP1", "L05", "WK", "MOOD", "BLD", "RVW",
        ];
        let candidates = sample_states();
        for state in candidates {
            if let Some(phrase) = compute_insight(&state, None) {
                for needle in FORBIDDEN {
                    assert!(
                        !phrase.contains(needle),
                        "phrase {phrase:?} contained forbidden token {needle}"
                    );
                }
                assert!(
                    !phrase.chars().any(|c| c.is_ascii_digit()),
                    "phrase {phrase:?} contained a digit"
                );
            }
        }
    }

    #[test]
    fn crt_strip_renders_three_rows_at_canonical_widths() {
        use crate::vivling::Stage;
        use crate::vl::crt::CrtScene;
        use crate::vl::crt::CrtSurface;
        use crate::vl::crt::CrtTier;
        use crate::vl::crt::VivlingCrtConfig;
        use crate::vl::crt::animation::TransitionPhases;
        use crate::vl::crt::render_crt_scene;

        let mut state = state_with_message("watching upstream");
        state.loop_blocked_busy = 2;
        let phrase = compute_insight(&state, None).expect("insight");
        let cfg = VivlingCrtConfig::default();
        let trans = TransitionPhases {
            mode_fade: 1.0,
            message_reveal_chars: usize::MAX,
            insight_slide: 1.0,
        };

        for width in [24u16, 40, 80] {
            let mut surface = CrtSurface::new(width, 3, ratatui::style::Style::default());
            let scene = CrtScene {
                species_id: "syllo",
                stage: Stage::Baby,
                name: "Nilo",
                level: 5,
                role: "builder",
                mood: "curious",
                energy: 73,
                hunger: 74,
                loop_count: 0,
                sprite: "('.')=  .",
                seed: 7,
                elapsed_ms: 0,
                last_message: Some(phrase.as_str()),
                activity: None,
                tier: CrtTier::Safe,
                crt_config: &cfg,
                transitions: trans,
            };
            render_crt_scene(&mut surface, &scene);
            let mut buf =
                ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, width, 3));
            surface.render(ratatui::layout::Rect::new(0, 0, width, 3), &mut buf);
            let rendered: String = buf.content.iter().map(|cell| cell.symbol()).collect();
            assert_eq!(
                rendered.len(),
                width as usize * 3,
                "width {width} must render exactly 3 rows"
            );
            assert!(
                !rendered.chars().any(|c| c.is_ascii_digit()),
                "rendered strip at width {width} contained digits: {rendered:?}"
            );
        }
    }

    fn sample_states() -> Vec<VivlingState> {
        let mut churn = VivlingState::default();
        churn.loop_admin_churn = 4;

        let mut blocked = VivlingState::default();
        blocked.loop_blocked_busy = 1;
        blocked
            .work_memory
            .push(loop_blocked_memory("loop_blocked_busy"));

        let mut error = VivlingState::default();
        error.brain_last_error = Some("oops".to_string());

        let mut active = VivlingState::default();
        active.loop_runtime_submissions = 2;
        active.work_memory.push(loop_submitted_memory());

        let mut summary = state_with_message("watching");
        summary.last_work_summary = Some("loop runtime work continues".to_string());

        let mut focus = VivlingState::default();
        focus.work_affinities = WorkAffinitySet {
            builder: 5,
            reviewer: 0,
            researcher: 0,
            operator: 0,
        };

        let plain = state_with_message("watching upstream");

        vec![churn, blocked, error, active, summary, focus, plain]
    }
}

#[cfg(test)]
mod step_12bd1_cache_tests {
    use super::*;
    use chrono::Duration as ChronoDuration;
    use chrono::TimeZone;
    use codex_vivling_core::model::CachedCrtPhrase;
    use codex_vivling_core::model::CachedProactive;
    use codex_vivling_core::model::WorkArchetype;

    fn pin_now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 5, 21, 12, 0, 0).unwrap()
    }

    fn state_with_message(msg: &str) -> VivlingState {
        let mut s = VivlingState::default();
        s.last_message = Some(msg.to_string());
        s
    }

    fn loop_blocked_memory(kind: &str) -> VivlingWorkMemoryEntry {
        VivlingWorkMemoryEntry {
            kind: kind.to_string(),
            summary: "blocked".to_string(),
            archetype: WorkArchetype::Operator,
            weight: 1,
            created_at: chrono::Utc::now(),
        }
    }

    fn fresh_crt(text: &str, now: chrono::DateTime<chrono::Utc>) -> CachedCrtPhrase {
        CachedCrtPhrase {
            text: text.to_string(),
            generated_at: Some(now),
            prompt_hash: Some(7),
            ttl_expires_at: Some(now + ChronoDuration::minutes(10)),
        }
    }

    fn stale_crt(text: &str, now: chrono::DateTime<chrono::Utc>) -> CachedCrtPhrase {
        CachedCrtPhrase {
            text: text.to_string(),
            generated_at: Some(now - ChronoDuration::hours(1)),
            prompt_hash: Some(7),
            ttl_expires_at: Some(now - ChronoDuration::minutes(5)),
        }
    }

    #[test]
    fn cached_crt_fresh_overrides_template_middle_chain() {
        let now = pin_now();
        let mut state = state_with_message("local fallback should not surface");
        state.cached_crt_phrase = Some(fresh_crt("llm bubble live", now));
        let out = compute_insight_at(&state, None, now).expect("phrase");
        assert_eq!(out, "llm bubble live");
    }

    #[test]
    fn cached_crt_stale_falls_back_to_template_chain() {
        let now = pin_now();
        let mut state = state_with_message("template");
        state.cached_crt_phrase = Some(stale_crt("expired bubble", now));
        let out = compute_insight_at(&state, None, now).expect("phrase");
        assert_ne!(out, "expired bubble");
    }

    #[test]
    fn cached_crt_with_empty_text_is_ignored() {
        let now = pin_now();
        let mut state = state_with_message("template");
        state.cached_crt_phrase = Some(CachedCrtPhrase {
            text: "   ".to_string(),
            generated_at: Some(now),
            prompt_hash: Some(7),
            ttl_expires_at: Some(now + ChronoDuration::minutes(10)),
        });
        let out = compute_insight_at(&state, None, now).expect("phrase");
        assert_ne!(out, "");
        assert_ne!(out.trim(), "");
    }

    #[test]
    fn cached_crt_with_no_ttl_is_treated_stale() {
        let now = pin_now();
        let mut state = state_with_message("template");
        state.cached_crt_phrase = Some(CachedCrtPhrase {
            text: "no ttl".to_string(),
            generated_at: Some(now),
            prompt_hash: Some(7),
            ttl_expires_at: None,
        });
        let out = compute_insight_at(&state, None, now).expect("phrase");
        assert_ne!(out, "no ttl");
    }

    #[test]
    fn brain_error_phrase_wins_over_cached_crt() {
        let now = pin_now();
        let mut state = VivlingState::default();
        state.brain_last_error = Some("brain oops".to_string());
        state.cached_crt_phrase = Some(fresh_crt("should be masked", now));
        let out = compute_insight_at(&state, None, now).expect("phrase");
        assert_ne!(out, "should be masked");
    }

    #[test]
    fn blocked_loop_phrase_wins_over_cached_crt() {
        let now = pin_now();
        let mut state = VivlingState::default();
        state.loop_blocked_busy = 1;
        state
            .work_memory
            .push(loop_blocked_memory("loop_blocked_busy"));
        state.cached_crt_phrase = Some(fresh_crt("should be masked", now));
        let out = compute_insight_at(&state, None, now).expect("phrase");
        assert_ne!(out, "should be masked");
    }

    /// Sonnet 4.6 P1 (Step 12.B.D.1 audit) — make the third safety
    /// template's precedence over the cached LLM bubble explicit.
    /// Without this test a future refactor could swap the ordering
    /// between `active_loop_phrase` and `cached_crt_phrase_at`
    /// without any unit test catching it.
    #[test]
    fn active_loop_phrase_wins_over_cached_crt() {
        let now = pin_now();
        let mut state = VivlingState::default();
        // Active loop trigger: a `loop_runtime` work-memory entry
        // whose summary contains "submitted" lets active_loop_phrase
        // produce a non-None string.
        state.work_memory.push(VivlingWorkMemoryEntry {
            kind: "loop_runtime".to_string(),
            summary: "loop submitted: ftri_check".to_string(),
            archetype: WorkArchetype::Operator,
            weight: 1,
            created_at: chrono::Utc::now(),
        });
        // Cache a fresh LLM bubble that must NOT mask the active
        // loop landing.
        state.cached_crt_phrase = Some(fresh_crt("should be masked", now));
        let out = compute_insight_at(&state, None, now).expect("phrase");
        assert_ne!(out, "should be masked");
    }

    #[test]
    fn cached_proactive_fresh_overrides_template_proactive() {
        let now = pin_now();
        let mut state = state_with_message("freeform");
        state.cached_proactive = Some(CachedProactive {
            text: "live proactive footer".to_string(),
            generated_at: Some(now),
            prompt_hash: Some(11),
            ttl_expires_at: Some(now + ChronoDuration::minutes(20)),
        });
        let out = compute_insight_at(&state, None, now).expect("phrase");
        assert_eq!(out, "live proactive footer");
    }

    #[test]
    fn cached_proactive_stale_falls_back_to_template() {
        let now = pin_now();
        let mut state = state_with_message("freeform");
        state.cached_proactive = Some(CachedProactive {
            text: "stale".to_string(),
            generated_at: Some(now - ChronoDuration::hours(1)),
            prompt_hash: Some(11),
            ttl_expires_at: Some(now - ChronoDuration::minutes(5)),
        });
        let out = compute_insight_at(&state, None, now).expect("phrase");
        assert_ne!(out, "stale");
    }

    #[test]
    fn compute_insight_wrapper_uses_utc_now() {
        let state = state_with_message("hello CRT");
        let out = compute_insight(&state, None).expect("phrase");
        assert!(!out.is_empty());
    }
}
