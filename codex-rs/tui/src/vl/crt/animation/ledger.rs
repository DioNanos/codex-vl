//! Vivling CRT animation ledger.
//!
//! Tracks the four discrete state changes (mode, message, insight, boot)
//! that drive event-based transitions, and produces a `TransitionPhases`
//! snapshot per render. All other animations remain pure functions of
//! `(elapsed_ms, seed)` and stay in `effects.rs`.

use std::cell::Cell;
use std::cell::RefCell;
use std::time::Duration;
use std::time::Instant;

use super::transitions::BOOT_BLINK_DURATION;
use super::transitions::BOOT_EYES_CLOSED_DURATION;
use super::transitions::BOOT_SCANLINE_DURATION;
use super::transitions::BOOT_TOTAL_DURATION;
use super::transitions::INSIGHT_SLIDE_DURATION;
use super::transitions::MODE_FADE_DURATION;
use super::transitions::TYPEWRITER_CHAR_INTERVAL;
use super::transitions::TransitionPhases;
use super::transitions::ease_in_out_cubic;
use super::transitions::ease_out_cubic;
use super::transitions::linear_progress;
use crate::vl::crt::director::CrtMode;

#[derive(Default, Debug)]
pub(crate) struct CrtAnimationLedger {
    last_mode: Cell<Option<CrtMode>>,
    mode_changed_at: Cell<Option<Instant>>,

    last_message: RefCell<Option<String>>,
    message_changed_at: Cell<Option<Instant>>,

    last_insight: RefCell<Option<String>>,
    insight_changed_at: Cell<Option<Instant>>,

    boot_started_at: Cell<Option<Instant>>,
    boot_skipped: Cell<bool>,
}

#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct ObservedDeltas {
    pub mode_changed: bool,
    pub message_changed: bool,
    pub insight_changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BootPhase {
    /// Top→bottom scanline reveal of the boot strip area.
    ScanLineWipe { progress: f32 },
    /// Sprite shown with closed eyes; holding before blink.
    EyesClosed { progress: f32 },
    /// Eyes opening (blink reverse).
    Blink { progress: f32 },
    /// Greeting typewriter.
    Greeting { chars_revealed: usize },
}

impl CrtAnimationLedger {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Observe scene inputs at the start of render. Mutates internal state
    /// when something changed; returns what changed for callers that want
    /// to react (e.g. request an immediate frame).
    pub(crate) fn observe(
        &self,
        now: Instant,
        mode: CrtMode,
        message: Option<&str>,
        insight: Option<&str>,
    ) -> ObservedDeltas {
        let mut deltas = ObservedDeltas::default();

        if self.last_mode.get() != Some(mode) {
            self.last_mode.set(Some(mode));
            self.mode_changed_at.set(Some(now));
            deltas.mode_changed = true;
        }

        let prev_msg = self.last_message.borrow().clone();
        let new_msg = message.map(str::to_string);
        if prev_msg.as_deref() != new_msg.as_deref() {
            *self.last_message.borrow_mut() = new_msg;
            self.message_changed_at.set(Some(now));
            deltas.message_changed = true;
        }

        let prev_ins = self.last_insight.borrow().clone();
        let new_ins = insight.map(str::to_string);
        if prev_ins.as_deref() != new_ins.as_deref() {
            *self.last_insight.borrow_mut() = new_ins;
            self.insight_changed_at.set(Some(now));
            deltas.insight_changed = true;
        }

        deltas
    }

    /// Compute current transition phases. Pure read — does not mutate.
    pub(crate) fn phases(&self, now: Instant) -> TransitionPhases {
        let mode_fade = match self.mode_changed_at.get() {
            Some(t) => ease_out_cubic(linear_progress(
                now.saturating_duration_since(t),
                MODE_FADE_DURATION,
            )),
            None => 1.0,
        };

        let message_reveal_chars = match (
            self.message_changed_at.get(),
            self.last_message.borrow().as_ref(),
        ) {
            (Some(t), Some(msg)) => {
                let elapsed = now.saturating_duration_since(t).as_millis() as u64;
                let interval = TYPEWRITER_CHAR_INTERVAL.as_millis() as u64;
                let revealed = (elapsed / interval) as usize;
                revealed.min(msg.chars().count())
            }
            _ => self
                .last_message
                .borrow()
                .as_ref()
                .map(|m| m.chars().count())
                .unwrap_or(0),
        };

        let insight_slide = match self.insight_changed_at.get() {
            Some(t) => ease_in_out_cubic(linear_progress(
                now.saturating_duration_since(t),
                INSIGHT_SLIDE_DURATION,
            )),
            None => 1.0,
        };

        TransitionPhases {
            mode_fade,
            message_reveal_chars,
            insight_slide,
        }
    }

    /// Boot starts the first time this is called (idempotent thereafter).
    pub(crate) fn ensure_boot_started(&self, now: Instant) {
        if self.boot_started_at.get().is_none() {
            self.boot_started_at.set(Some(now));
        }
    }

    /// User keypress to skip boot. Idempotent.
    pub(crate) fn skip_boot(&self) {
        self.boot_skipped.set(true);
    }

    pub(crate) fn boot_skipped(&self) -> bool {
        self.boot_skipped.get()
    }

    /// Current boot phase, or None if boot is finished/skipped/disabled.
    pub(crate) fn boot_phase(&self, now: Instant) -> Option<BootPhase> {
        if self.boot_skipped.get() {
            return None;
        }
        let started = self.boot_started_at.get()?;
        let elapsed = now.saturating_duration_since(started);
        if elapsed >= BOOT_TOTAL_DURATION {
            return None;
        }

        let mut cursor = Duration::ZERO;
        let scan_end = cursor + BOOT_SCANLINE_DURATION;
        if elapsed < scan_end {
            return Some(BootPhase::ScanLineWipe {
                progress: linear_progress(elapsed - cursor, BOOT_SCANLINE_DURATION),
            });
        }
        cursor = scan_end;

        let eyes_end = cursor + BOOT_EYES_CLOSED_DURATION;
        if elapsed < eyes_end {
            return Some(BootPhase::EyesClosed {
                progress: linear_progress(elapsed - cursor, BOOT_EYES_CLOSED_DURATION),
            });
        }
        cursor = eyes_end;

        let blink_end = cursor + BOOT_BLINK_DURATION;
        if elapsed < blink_end {
            return Some(BootPhase::Blink {
                progress: ease_out_cubic(linear_progress(elapsed - cursor, BOOT_BLINK_DURATION)),
            });
        }
        cursor = blink_end;

        let greeting_phase = elapsed - cursor;
        let interval = TYPEWRITER_CHAR_INTERVAL.as_millis() as u64;
        let revealed = (greeting_phase.as_millis() as u64 / interval) as usize;
        Some(BootPhase::Greeting {
            chars_revealed: revealed,
        })
    }

    /// Hint when next render frame should fire. None = no animation in
    /// flight; caller may schedule a long lazy pulse instead.
    pub(crate) fn next_wake(&self, now: Instant) -> Option<Duration> {
        let mut wake: Option<Duration> = None;
        let push = |wake: &mut Option<Duration>, d: Duration| {
            if d.is_zero() {
                *wake = Some(Duration::from_millis(16));
            } else if wake.map_or(true, |w| d < w) {
                *wake = Some(d);
            }
        };

        if self.boot_phase(now).is_some()
            && let Some(start) = self.boot_started_at.get()
        {
            let elapsed = now.saturating_duration_since(start);
            if elapsed < BOOT_TOTAL_DURATION {
                push(&mut wake, Duration::from_millis(50));
            }
        }

        if let Some(t) = self.mode_changed_at.get() {
            let elapsed = now.saturating_duration_since(t);
            if elapsed < MODE_FADE_DURATION {
                push(&mut wake, MODE_FADE_DURATION - elapsed);
            }
        }
        if let Some(t) = self.insight_changed_at.get() {
            let elapsed = now.saturating_duration_since(t);
            if elapsed < INSIGHT_SLIDE_DURATION {
                push(&mut wake, INSIGHT_SLIDE_DURATION - elapsed);
            }
        }
        if let (Some(t), Some(msg)) = (
            self.message_changed_at.get(),
            self.last_message.borrow().as_ref(),
        ) {
            let elapsed = now.saturating_duration_since(t).as_millis() as u64;
            let interval = TYPEWRITER_CHAR_INTERVAL.as_millis() as u64;
            let revealed = (elapsed / interval) as usize;
            let total = msg.chars().count();
            if revealed < total {
                push(&mut wake, TYPEWRITER_CHAR_INTERVAL);
            }
        }
        wake
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now0() -> Instant {
        Instant::now()
    }

    #[test]
    fn first_observation_marks_all_changed() {
        let l = CrtAnimationLedger::new();
        let d = l.observe(now0(), CrtMode::Idle, Some("hi"), Some("focus"));
        assert!(d.mode_changed);
        assert!(d.message_changed);
        assert!(d.insight_changed);
    }

    #[test]
    fn unchanged_observation_reports_no_deltas() {
        let l = CrtAnimationLedger::new();
        let t = now0();
        l.observe(t, CrtMode::Idle, Some("hi"), Some("focus"));
        let d = l.observe(t + Duration::from_millis(5), CrtMode::Idle, Some("hi"), Some("focus"));
        assert!(!d.mode_changed);
        assert!(!d.message_changed);
        assert!(!d.insight_changed);
    }

    #[test]
    fn mode_fade_progresses_after_change() {
        let l = CrtAnimationLedger::new();
        let t = now0();
        l.observe(t, CrtMode::Idle, None, None);
        l.observe(t + Duration::from_millis(10), CrtMode::Working, None, None);
        let p_early = l.phases(t + Duration::from_millis(10));
        let p_mid = l.phases(t + Duration::from_millis(10) + MODE_FADE_DURATION / 2);
        let p_done = l.phases(t + Duration::from_millis(10) + MODE_FADE_DURATION + Duration::from_millis(50));
        assert!(p_early.mode_fade < 0.1);
        assert!(p_mid.mode_fade > 0.4 && p_mid.mode_fade < 1.0);
        assert!((p_done.mode_fade - 1.0).abs() < 1e-6);
    }

    #[test]
    fn typewriter_reveals_chars_over_time() {
        let l = CrtAnimationLedger::new();
        let t = now0();
        l.observe(t, CrtMode::Idle, Some("hello"), None);
        let p0 = l.phases(t);
        let p_mid = l.phases(t + TYPEWRITER_CHAR_INTERVAL * 2 + Duration::from_millis(5));
        let p_done = l.phases(t + TYPEWRITER_CHAR_INTERVAL * 10);
        assert_eq!(p0.message_reveal_chars, 0);
        assert_eq!(p_mid.message_reveal_chars, 2);
        assert_eq!(p_done.message_reveal_chars, 5);
    }

    #[test]
    fn changing_message_resets_typewriter() {
        let l = CrtAnimationLedger::new();
        let t = now0();
        l.observe(t, CrtMode::Idle, Some("first"), None);
        let _ = l.phases(t + TYPEWRITER_CHAR_INTERVAL * 5);
        l.observe(t + TYPEWRITER_CHAR_INTERVAL * 5, CrtMode::Idle, Some("new!"), None);
        let p = l.phases(t + TYPEWRITER_CHAR_INTERVAL * 5);
        assert_eq!(p.message_reveal_chars, 0);
    }

    #[test]
    fn boot_phase_progresses_through_stages() {
        let l = CrtAnimationLedger::new();
        let t = now0();
        l.ensure_boot_started(t);
        assert!(matches!(l.boot_phase(t), Some(BootPhase::ScanLineWipe { .. })));
        assert!(matches!(
            l.boot_phase(t + BOOT_SCANLINE_DURATION + Duration::from_millis(10)),
            Some(BootPhase::EyesClosed { .. })
        ));
        assert!(matches!(
            l.boot_phase(t + BOOT_SCANLINE_DURATION + BOOT_EYES_CLOSED_DURATION + Duration::from_millis(10)),
            Some(BootPhase::Blink { .. })
        ));
        assert!(matches!(
            l.boot_phase(t + BOOT_SCANLINE_DURATION + BOOT_EYES_CLOSED_DURATION + BOOT_BLINK_DURATION + Duration::from_millis(10)),
            Some(BootPhase::Greeting { .. })
        ));
        assert!(l.boot_phase(t + BOOT_TOTAL_DURATION + Duration::from_millis(10)).is_none());
    }

    #[test]
    fn boot_skip_terminates_phases_immediately() {
        let l = CrtAnimationLedger::new();
        let t = now0();
        l.ensure_boot_started(t);
        l.skip_boot();
        assert!(l.boot_phase(t + Duration::from_millis(10)).is_none());
        assert!(l.boot_skipped());
    }

    #[test]
    fn next_wake_is_none_when_idle_settled() {
        let l = CrtAnimationLedger::new();
        let t = now0();
        l.observe(t, CrtMode::Idle, None, None);
        let after = t + MODE_FADE_DURATION + Duration::from_millis(100);
        let _ = l.phases(after);
        assert!(l.next_wake(after).is_none());
    }

    #[test]
    fn next_wake_present_during_typewriter() {
        let l = CrtAnimationLedger::new();
        let t = now0();
        l.observe(t, CrtMode::Idle, Some("longer message"), None);
        let w = l.next_wake(t + TYPEWRITER_CHAR_INTERVAL);
        assert_eq!(w, Some(TYPEWRITER_CHAR_INTERVAL));
    }
}
