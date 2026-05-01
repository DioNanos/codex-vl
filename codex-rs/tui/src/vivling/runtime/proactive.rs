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

/// Outcome of evaluating proactive triggers.
#[derive(Debug)]
pub(crate) struct ProactiveOutcome {
    /// Message to surface (goes into last_message).
    pub(crate) message: Option<String>,
}

/// Evaluate all proactive triggers for the given state after a loop event.
pub(crate) fn evaluate_after_loop_event(
    state: &VivlingState,
    _now: chrono::DateTime<chrono::Utc>,
) -> ProactiveOutcome {
    if state.stage() != Stage::Adult {
        return ProactiveOutcome { message: None };
    }

    // Blocked loops accumulating
    if state.loop_runtime_blocks == 3 && state.loop_runtime_blocks > state.loop_runtime_submissions
    {
        return ProactiveOutcome {
            message: Some("loops blocked. want me to check?".to_string()),
        };
    }

    // High churn without runtime
    if state.loop_admin_churn == 5 && state.loop_runtime_submissions == 0 {
        return ProactiveOutcome {
            message: Some("too much churn. stop touching loops".to_string()),
        };
    }

    // Many clean submissions
    if state.loop_runtime_submissions == 5
        && state.loop_runtime_blocks == 0
        && state.loop_admin_churn <= 2
    {
        return ProactiveOutcome {
            message: Some("loop rhythm clean. keep going".to_string()),
        };
    }

    ProactiveOutcome { message: None }
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

    // Every ~10 turns, check in
    if turns > 0 && turns % 10 == 0 {
        return ProactiveOutcome {
            message: Some(format!("{} turns. want a recap?", turns)),
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
                message: Some("builder pattern strong. want a build loop?".to_string()),
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
            message: Some("release pattern seen. checklist loop?".to_string()),
        };
    }

    // Verification bias high
    if state.identity_profile.verification_bias >= 5 {
        return ProactiveOutcome {
            message: Some("verify before widening".to_string()),
        };
    }

    ProactiveOutcome { message: None }
}
