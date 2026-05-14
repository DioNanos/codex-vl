//! Pool of short autonomous thoughts for baby and juvenile Vivlings
//! during prolonged idle periods.
//!
//! Pools are modulated by `LifecycleVoiceTone` (relational tone band derived
//! from the bond level at the boundary adapter). `vl/lifecycle` stays generic:
//! the bond domain lives in `vivling/bond.rs`, and the adapter in `*_ext.rs`
//! translates `vivling::BondTone` to `LifecycleVoiceTone` per tick.

use super::voice_tone::LifecycleVoiceTone;

/// Returns a thought string for the given stage, tick counter, and relational tone.
/// Tick counter should increment slowly (e.g. every 15-30s of idle).
pub(crate) fn idle_thought(
    is_juvenile: bool,
    tick: usize,
    tone: LifecycleVoiceTone,
) -> &'static str {
    if is_juvenile {
        match tone {
            LifecycleVoiceTone::Neutral => JUVENILE_NEUTRAL[tick % JUVENILE_NEUTRAL.len()],
            LifecycleVoiceTone::Warm => JUVENILE_WARM[tick % JUVENILE_WARM.len()],
            LifecycleVoiceTone::Familiar => JUVENILE_FAMILIAR[tick % JUVENILE_FAMILIAR.len()],
        }
    } else {
        match tone {
            LifecycleVoiceTone::Neutral => BABY_NEUTRAL[tick % BABY_NEUTRAL.len()],
            LifecycleVoiceTone::Warm => BABY_WARM[tick % BABY_WARM.len()],
            LifecycleVoiceTone::Familiar => BABY_FAMILIAR[tick % BABY_FAMILIAR.len()],
        }
    }
}

// Baby pools (stage < Juvenile).

const BABY_NEUTRAL: &[&str] = &[
    "observing..",
    "noting context",
    "new here",
    "small, listening",
    "watching turns",
    "still learning",
];

/// Default warmth — preserves the pool that shipped pre-care-effects.
const BABY_WARM: &[&str] = &[
    "what is this..",
    "learning rhythm",
    "small but here",
    "tell me more",
    "ready to learn",
    "watching you",
    "what's next?",
    "feed me work",
];

const BABY_FAMILIAR: &[&str] = &[
    "with you here",
    "settled in",
    "our rhythm now",
    "growing close",
    "tell me again",
    "warm and small",
];

// Juvenile pools.

const JUVENILE_NEUTRAL: &[&str] = &[
    "verify first",
    "check pattern",
    "one goal at a time",
    "approaching adult",
    "logging signals",
    "noting blockers",
];

/// Default warmth — preserves the pool that shipped pre-care-effects.
const JUVENILE_WARM: &[&str] = &[
    "verify first?",
    "check the loop",
    "pattern seen..",
    "almost adult",
    "keep one goal",
    "growing fast",
    "what changed?",
    "ready to help",
];

const JUVENILE_FAMILIAR: &[&str] = &[
    "we should verify?",
    "let's check the loop",
    "pattern again — seen it",
    "growing with you",
    "keep our one goal",
    "almost ready to help",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warm_preserves_current_baby_pool() {
        // The shipped pre-care-effects pool maps to Warm; canary on first string.
        assert_eq!(
            idle_thought(false, 0, LifecycleVoiceTone::Warm),
            "what is this.."
        );
    }

    #[test]
    fn warm_preserves_current_juvenile_pool() {
        assert_eq!(
            idle_thought(true, 0, LifecycleVoiceTone::Warm),
            "verify first?"
        );
    }

    #[test]
    fn neutral_pool_differs_from_warm_baby() {
        let neutral = idle_thought(false, 0, LifecycleVoiceTone::Neutral);
        let warm = idle_thought(false, 0, LifecycleVoiceTone::Warm);
        assert_ne!(neutral, warm);
    }

    #[test]
    fn familiar_pool_differs_from_warm_baby() {
        let familiar = idle_thought(false, 0, LifecycleVoiceTone::Familiar);
        let warm = idle_thought(false, 0, LifecycleVoiceTone::Warm);
        assert_ne!(familiar, warm);
    }

    #[test]
    fn pools_cycle_with_tick() {
        // Pool indexing wraps modulo pool length — verify two adjacent ticks
        // give two distinct strings (smoke check that tick parameter is read).
        let a = idle_thought(false, 0, LifecycleVoiceTone::Warm);
        let b = idle_thought(false, 1, LifecycleVoiceTone::Warm);
        assert_ne!(a, b);
    }
}
