//! Proactive triggers for adult Vivlings.
//!
//! When the Vivling reaches adult stage (level 60+) and has a brain
//! configured, it can autonomously suggest actions based on learned
//! patterns. Triggers are evaluated during loop events and turn completions.
//! They use milestone thresholds instead of wall-clock state so they remain
//! deterministic and do not repeat on every redraw.

use super::super::model::Stage;
use super::super::model::VivlingState;
use super::super::model::WorkArchetype;
use crate::vivling::BondTone;

/// Outcome of evaluating proactive triggers.
#[derive(Debug)]
pub(crate) struct ProactiveOutcome {
    /// Message to surface (goes into last_message).
    pub(crate) message: Option<String>,
}

/// Pick a tone-modulated message string. Centralised so each trigger branch
/// keeps to a single readable line.
fn tone_pick(tone: BondTone, neutral: &str, warm: &str, familiar: &str) -> String {
    match tone {
        BondTone::Neutral => neutral,
        BondTone::Warm => warm,
        BondTone::Familiar => familiar,
    }
    .to_string()
}

/// Evaluate all proactive triggers for the given state after a loop event.
pub(crate) fn evaluate_after_loop_event(
    state: &VivlingState,
    _now: chrono::DateTime<chrono::Utc>,
) -> ProactiveOutcome {
    if state.stage() != Stage::Adult {
        return ProactiveOutcome { message: None };
    }

    let tone = state.bond.tone();

    // Blocked loops accumulating
    if state.loop_runtime_blocks == 3 && state.loop_runtime_blocks > state.loop_runtime_submissions
    {
        return ProactiveOutcome {
            message: Some(tone_pick(
                tone,
                "loops blocked. need a check?",
                "loops blocked. want me to check?",
                "those loops keep blocking — let's check?",
            )),
        };
    }

    // High churn without runtime
    if state.loop_admin_churn == 5 && state.loop_runtime_submissions == 0 {
        return ProactiveOutcome {
            message: Some(tone_pick(
                tone,
                "high churn. pause edits",
                "too much churn. stop touching loops",
                "we keep poking — let the loops settle",
            )),
        };
    }

    // Many clean submissions
    if state.loop_runtime_submissions == 5
        && state.loop_runtime_blocks == 0
        && state.loop_admin_churn <= 2
    {
        return ProactiveOutcome {
            message: Some(tone_pick(
                tone,
                "loop rhythm clean",
                "loop rhythm clean. keep going",
                "our rhythm is clean — keep going",
            )),
        };
    }

    ProactiveOutcome { message: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vivling::model::ADULT_LEVEL;
    use crate::vivling::model::SeedIdentity;
    use crate::vivling::model::VivlingDistilledSummary;
    use crate::vivling::model::WORK_XP_PER_LEVEL;

    fn adult_state() -> VivlingState {
        adult_state_with_bond(60)
    }

    fn adult_state_with_bond(bond_value: u8) -> VivlingState {
        let mut state = VivlingState::new(SeedIdentity {
            value: "install:proactive-test".to_string(),
            install_id: Some("proactive-test".to_string()),
        });
        state.active_work_days = 90;
        state.work_xp = WORK_XP_PER_LEVEL.saturating_mul(ADULT_LEVEL.saturating_sub(1));
        state.recompute_level();
        // Place bond in the requested band: default 60 → Warm so existing
        // proactive expectations keep matching the pre-care-effects pool.
        state.bond.value = bond_value;
        state
    }

    #[test]
    fn loop_proactive_triggers_only_on_exact_blocked_threshold() {
        let now = chrono::Utc::now();
        let mut state = adult_state();
        state.loop_runtime_blocks = 2;
        assert!(evaluate_after_loop_event(&state, now).message.is_none());

        state.loop_runtime_blocks = 3;
        assert_eq!(
            evaluate_after_loop_event(&state, now).message.as_deref(),
            Some("loops blocked. want me to check?")
        );

        state.loop_runtime_blocks = 4;
        assert!(evaluate_after_loop_event(&state, now).message.is_none());
    }

    #[test]
    fn loop_proactive_churn_and_clean_rhythm_do_not_repeat_off_threshold() {
        let now = chrono::Utc::now();
        let mut churn = adult_state();
        churn.loop_admin_churn = 5;
        assert_eq!(
            evaluate_after_loop_event(&churn, now).message.as_deref(),
            Some("too much churn. stop touching loops")
        );
        churn.loop_admin_churn = 6;
        assert!(evaluate_after_loop_event(&churn, now).message.is_none());

        let mut clean = adult_state();
        clean.loop_runtime_submissions = 5;
        assert_eq!(
            evaluate_after_loop_event(&clean, now).message.as_deref(),
            Some("loop rhythm clean. keep going")
        );
        clean.loop_runtime_submissions = 6;
        assert!(evaluate_after_loop_event(&clean, now).message.is_none());
    }

    #[test]
    fn turn_proactive_prefers_periodic_recap_over_pattern_noise() {
        let now = chrono::Utc::now();
        let mut state = adult_state();
        state.turns_observed = 10;
        state.work_affinities.builder = 10;

        assert_eq!(
            evaluate_after_turn(&state, now).message.as_deref(),
            Some("10 turns. want a recap?")
        );

        state.turns_observed = 11;
        assert!(evaluate_after_turn(&state, now).message.is_none());
    }

    #[test]
    fn loop_proactive_blocked_neutral_pool_differs_from_warm() {
        let now = chrono::Utc::now();
        let mut state = adult_state_with_bond(10); // Neutral
        state.loop_runtime_blocks = 3;
        assert_eq!(
            evaluate_after_loop_event(&state, now).message.as_deref(),
            Some("loops blocked. need a check?")
        );
    }

    #[test]
    fn loop_proactive_blocked_familiar_pool_differs_from_warm() {
        let now = chrono::Utc::now();
        let mut state = adult_state_with_bond(95); // Familiar (Bonded)
        state.loop_runtime_blocks = 3;
        assert_eq!(
            evaluate_after_loop_event(&state, now).message.as_deref(),
            Some("those loops keep blocking — let's check?")
        );
    }

    #[test]
    fn turn_recap_message_is_tone_modulated() {
        let now = chrono::Utc::now();
        let mut state = adult_state_with_bond(95);
        state.turns_observed = 10;
        state.work_affinities.builder = 10;
        assert_eq!(
            evaluate_after_turn(&state, now).message.as_deref(),
            Some("10 turns together — our recap?")
        );
    }

    #[test]
    fn turn_proactive_release_and_verification_only_fire_on_six_turn_cadence() {
        let now = chrono::Utc::now();
        let mut state = adult_state();
        state.turns_observed = 6;
        state.distilled_summaries.push(VivlingDistilledSummary {
            topic: "release".to_string(),
            summary: "release checklist landed".to_string(),
            kind: "turn".to_string(),
            archetype: WorkArchetype::Operator,
            total_weight: 10,
            observations: 1,
            first_seen_at: now,
            last_seen_at: now,
        });
        assert_eq!(
            evaluate_after_turn(&state, now).message.as_deref(),
            Some("release pattern seen. checklist loop?")
        );

        state.turns_observed = 7;
        state.identity_profile.verification_bias = 5;
        assert!(evaluate_after_turn(&state, now).message.is_none());
    }
}

/// Evaluate proactive triggers after a turn completes.
pub(crate) fn evaluate_after_turn(
    state: &VivlingState,
    _now: chrono::DateTime<chrono::Utc>,
) -> ProactiveOutcome {
    if state.stage() != Stage::Adult {
        return ProactiveOutcome { message: None };
    }

    let turns = state.turns_observed;
    let tone = state.bond.tone();

    // Every ~10 turns, check in
    if turns > 0 && turns % 10 == 0 {
        let message = match tone {
            BondTone::Neutral => format!("{turns} turns. recap?"),
            BondTone::Warm => format!("{turns} turns. want a recap?"),
            BondTone::Familiar => format!("{turns} turns together — our recap?"),
        };
        return ProactiveOutcome {
            message: Some(message),
        };
    }

    if turns == 0 || turns % 6 != 0 {
        return ProactiveOutcome { message: None };
    }

    // Pattern detection from dominant archetype
    if turns >= 8 && state.dominant_archetype() == WorkArchetype::Builder {
        let builder_ratio = state.work_affinities.builder as f64
            / (state.work_affinities.builder
                + state.work_affinities.reviewer
                + state.work_affinities.researcher
                + state.work_affinities.operator)
                .max(1) as f64;
        if builder_ratio > 0.6 {
            return ProactiveOutcome {
                message: Some(tone_pick(
                    tone,
                    "builder pattern strong. build loop?",
                    "builder pattern strong. want a build loop?",
                    "you're building a lot — our build loop?",
                )),
            };
        }
    }

    // Release patterns in memory
    if state
        .distilled_summaries
        .iter()
        .any(|s| s.topic.contains("release"))
    {
        return ProactiveOutcome {
            message: Some(tone_pick(
                tone,
                "release pattern. checklist loop?",
                "release pattern seen. checklist loop?",
                "another release pattern — our checklist loop?",
            )),
        };
    }

    // Verification bias high
    if state.identity_profile.verification_bias >= 5 {
        return ProactiveOutcome {
            message: Some(tone_pick(
                tone,
                "verify before widening",
                "verify before widening",
                "let's verify before widening",
            )),
        };
    }

    ProactiveOutcome { message: None }
}
