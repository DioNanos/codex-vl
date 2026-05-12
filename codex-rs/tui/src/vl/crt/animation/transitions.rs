//! Animation timing constants, easing functions, and transition-phase struct.
//!
//! Pure helpers; no state. Consumed by `ledger.rs` to derive the
//! `TransitionPhases` snapshot every render.

use std::time::Duration;

// ---- timing constants -------------------------------------------------------

/// Mode change: fade-out previous face, fade-in new face.
pub(crate) const MODE_FADE_DURATION: Duration = Duration::from_millis(420);
/// Per-character delay for the typewriter effect on `last_message`.
pub(crate) const TYPEWRITER_CHAR_INTERVAL: Duration = Duration::from_millis(45);
/// Insight slide-in (vertical offset → settled).
pub(crate) const INSIGHT_SLIDE_DURATION: Duration = Duration::from_millis(280);

/// Idle micro-animation periods.
pub(crate) const FLICKER_PERIOD_MS: u64 = 2_700;
pub(crate) const FLICKER_BURST_MS: u64 = 110;

// ---- snapshot types ---------------------------------------------------------

#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct TransitionPhases {
    /// 0.0 = mode just changed; 1.0 = transition complete. Eased.
    pub mode_fade: f32,
    /// Number of chars of `last_message` to reveal (typewriter cursor).
    pub message_reveal_chars: usize,
    /// 0.0 = insight just appeared; 1.0 = settled. Eased.
    pub insight_slide: f32,
}

impl TransitionPhases {
    /// All transitions completed (no in-flight motion).
    pub(crate) fn is_settled(&self) -> bool {
        self.mode_fade >= 1.0 && self.insight_slide >= 1.0
    }
}

// ---- easing -----------------------------------------------------------------

pub(crate) fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub(crate) fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t.powi(3)
    } else {
        1.0 - ((-2.0 * t + 2.0).powi(3)) / 2.0
    }
}

/// Linear ramp 0→1 over `duration` since `start`. Saturates at 1.0.
/// Returns 1.0 if `start >= now` (clock skew) to avoid negative phases.
pub(crate) fn linear_progress(elapsed: Duration, duration: Duration) -> f32 {
    if duration.is_zero() {
        return 1.0;
    }
    let e = elapsed.as_millis() as f32;
    let d = duration.as_millis() as f32;
    (e / d).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_progress_clamps_to_unit_interval() {
        assert_eq!(linear_progress(Duration::ZERO, Duration::from_millis(100)), 0.0);
        assert!((linear_progress(Duration::from_millis(50), Duration::from_millis(100)) - 0.5).abs() < 1e-6);
        assert_eq!(linear_progress(Duration::from_millis(200), Duration::from_millis(100)), 1.0);
        assert_eq!(linear_progress(Duration::from_millis(50), Duration::ZERO), 1.0);
    }

    #[test]
    fn ease_out_cubic_endpoints() {
        assert!((ease_out_cubic(0.0)).abs() < 1e-6);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < 1e-6);
        assert!(ease_out_cubic(0.5) > 0.5); // front-loaded
    }

    #[test]
    fn ease_in_out_cubic_is_symmetric_at_midpoint() {
        let m = ease_in_out_cubic(0.5);
        assert!((m - 0.5).abs() < 1e-3);
        assert!((ease_in_out_cubic(0.0)).abs() < 1e-6);
        assert!((ease_in_out_cubic(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn settled_phases_report_settled() {
        let p = TransitionPhases {
            mode_fade: 1.0,
            message_reveal_chars: 0,
            insight_slide: 1.0,
        };
        assert!(p.is_settled());
        let p = TransitionPhases::default();
        assert!(!p.is_settled());
    }

    #[test]
    fn easing_clamps_out_of_range_inputs() {
        assert_eq!(ease_out_cubic(-1.0), 0.0);
        assert_eq!(ease_out_cubic(2.0), 1.0);
        assert_eq!(ease_in_out_cubic(-1.0), 0.0);
        assert_eq!(ease_in_out_cubic(2.0), 1.0);
    }
}
