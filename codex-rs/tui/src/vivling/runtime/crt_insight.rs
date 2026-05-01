//! CRT bubble insight layer.
//!
//! Distils one short, high-signal phrase from the live `VivlingState` for the
//! 3-row CRT strip. The bubble already truncates and styles the phrase, so
//! the goal here is just to pick the most useful thing to say right now —
//! never numeric labels, never long sentences.

use super::super::model::WorkArchetype;
use super::VivlingLiveContext;
use super::VivlingState;

/// Maximum chars in a generated phrase. Speech bubble does its own further
/// trimming; this is the upstream cap so we stay readable on narrow widths.
const INSIGHT_MAX_CHARS: usize = 28;

/// Compute the bubble text for the CRT strip. Returns `None` only when the
/// Vivling has nothing useful to say (no signals, no last message).
pub(crate) fn compute_insight(
    state: &VivlingState,
    live_context: Option<&VivlingLiveContext>,
) -> Option<String> {
    if let Some(phrase) = brain_error_phrase(state) {
        return Some(phrase);
    }
    if let Some(phrase) = live_context.and_then(VivlingLiveContext::crt_phrase) {
        return Some(phrase);
    }
    if let Some(phrase) = active_loop_phrase(state) {
        return Some(phrase);
    }
    if let Some(phrase) = blocked_loop_phrase(state) {
        return Some(phrase);
    }
    if let Some(phrase) = last_work_summary_phrase(state) {
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
    let blocked_total = state
        .loop_blocked_review
        .saturating_add(state.loop_blocked_side)
        .saturating_add(state.loop_blocked_busy);
    if blocked_total > 0 && blocked_total >= state.loop_runtime_submissions.max(1) {
        if state.loop_blocked_review >= state.loop_blocked_side
            && state.loop_blocked_review >= state.loop_blocked_busy
        {
            return Some("review gate blocking".to_string());
        }
        if state.loop_blocked_side >= state.loop_blocked_busy {
            return Some("side work blocking".to_string());
        }
        return Some("busy - wait for state".to_string());
    }
    if state.loop_admin_churn >= 3 && state.loop_runtime_submissions == 0 {
        return Some("churn - verify state".to_string());
    }
    None
}

fn active_loop_phrase(state: &VivlingState) -> Option<String> {
    if state.loop_runtime_submissions > 0
        && state.loop_runtime_submissions > state.loop_runtime_blocks
    {
        return Some("loop work landing".to_string());
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
            "plan approved"
        } else if lower.contains("plan") && lower.contains("proposed") {
            "plan ready"
        } else if lower.contains("test") {
            "tests checked"
        } else if lower.contains("review") {
            "review work"
        } else if lower.contains("release") {
            "release flow"
        } else if lower.contains("doc") {
            "docs work"
        } else if lower.contains("merge") || lower.contains("rebase") {
            "merge work"
        } else if lower.contains("verif") || lower.contains("verified") {
            "verify next step"
        } else if lower.contains("complet") || lower.contains("done") {
            "tests passed"
        } else if lower.contains("loop") {
            "watching upstream"
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
    use super::super::super::model::VivlingState;
    use super::super::super::model::WorkAffinitySet;
    use super::*;

    fn state_with_message(msg: &str) -> VivlingState {
        VivlingState {
            last_message: Some(msg.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn blocked_signal_beats_generic_last_message() {
        let mut state = state_with_message("greets the operator");
        state.loop_blocked_review = 2;
        state.loop_runtime_submissions = 0;
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "review gate blocking");
    }

    #[test]
    fn blocked_busy_signal_names_waiting_state() {
        let mut state = state_with_message("greets the operator");
        state.loop_blocked_busy = 2;
        state.loop_runtime_submissions = 0;
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "busy - wait for state");
    }

    #[test]
    fn live_context_beats_stale_blocked_signal() {
        let mut state = state_with_message("greets the operator");
        state.loop_blocked_review = 2;
        state.loop_runtime_submissions = 0;
        let context = VivlingLiveContext {
            active_agent_label: Some("main".to_string()),
            task_progress: Some("build running".to_string()),
            ..Default::default()
        };

        let phrase = compute_insight(&state, Some(&context)).expect("insight");
        assert_eq!(phrase, "active: main");
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
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "loop work landing");
    }

    #[test]
    fn last_work_summary_is_compacted() {
        let mut state = state_with_message("greets");
        state.last_work_summary = Some(
            "turn completed: review the README and audit the change carefully across files"
                .to_string(),
        );
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "review work");
        assert!(phrase.chars().count() <= INSIGHT_MAX_CHARS);
    }

    #[test]
    fn plan_approval_summary_has_specific_phrase() {
        let mut state = state_with_message("greets");
        state.last_work_summary = Some("plan approved: implement the selected fix".to_string());
        let phrase = compute_insight(&state, None).expect("insight");
        assert_eq!(phrase, "plan approved");
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
    fn live_active_agent_context_beats_generic_work_summary() {
        let mut state = state_with_message("watching completed turns closely");
        state.last_work_summary = Some("turn completed: tests passed".to_string());
        let context = VivlingLiveContext {
            active_agent_label: Some("Robie [worker]".to_string()),
            run_state: Some("Working".to_string()),
            ..Default::default()
        };

        let phrase = compute_insight(&state, Some(&context)).expect("insight");
        assert_eq!(phrase, "active: Robie [worker]");
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
        use crate::vl::crt::render_crt_scene;

        let mut state = state_with_message("watching upstream");
        state.loop_blocked_busy = 2;
        let phrase = compute_insight(&state, None).expect("insight");

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

        let mut error = VivlingState::default();
        error.brain_last_error = Some("oops".to_string());

        let mut active = VivlingState::default();
        active.loop_runtime_submissions = 2;

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
