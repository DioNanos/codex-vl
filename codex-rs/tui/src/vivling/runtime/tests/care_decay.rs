//! codex-vl care decay slowdown (Fase 4 iter 9) — integration tests.
//!
//! These tests pin the explicit `last_seen_at` contract described in
//! the care-decay slowdown design section 3.1.
//! Five branches map 1:1 to dedicated tests; Branch E gets three scenarios
//! (two weeks, three months, one year) to verify weekly rate + clamp.

use super::common::*;
use chrono::Duration;
use chrono::TimeZone;
use chrono::Utc;

fn anchor() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 14, 10, 0, 0).unwrap()
}

fn seeded_with_last_seen(last_seen: chrono::DateTime<Utc>) -> VivlingState {
    let mut state = seeded_state();
    state.last_seen_at = Some(last_seen);
    state
}

#[test]
fn decay_clock_skew_does_not_move_last_seen_backwards() {
    let now = anchor();
    let future_last_seen = now + Duration::hours(6);
    let mut state = seeded_with_last_seen(future_last_seen);

    state.apply_decay(now);

    assert_eq!(state.hunger, 82);
    assert_eq!(state.energy, 76);
    assert_eq!(state.happiness, 70);
    assert_eq!(state.social, 62);
    assert_eq!(
        state.last_seen_at,
        Some(future_last_seen),
        "clock skew (elapsed < 0) must not move last_seen_at backwards",
    );
}

#[test]
fn decay_under_12h_refreshes_last_seen_without_touching_meters() {
    let now = anchor();
    let mut state = seeded_with_last_seen(now - Duration::hours(3));

    state.apply_decay(now);

    assert_eq!(state.hunger, 82);
    assert_eq!(state.energy, 76);
    assert_eq!(state.happiness, 70);
    assert_eq!(state.social, 62);
    assert_eq!(
        state.last_seen_at,
        Some(now),
        "anti-tilt 12h refreshes last_seen_at to now",
    );
}

#[test]
fn decay_under_one_week_updates_last_seen_without_decay() {
    let now = anchor();
    let mut state = seeded_with_last_seen(now - Duration::days(6));

    state.apply_decay(now);

    assert_eq!(state.hunger, 82);
    assert_eq!(state.energy, 76);
    assert_eq!(state.happiness, 70);
    assert_eq!(state.social, 62);
    assert_eq!(
        state.last_seen_at,
        Some(now),
        "Branch D (12h..7d) advances last_seen_at to avoid sequential drift",
    );
}

#[test]
fn decay_two_weeks_reduces_meters_by_two_cycles() {
    let now = anchor();
    let mut state = seeded_with_last_seen(now - Duration::days(14));

    state.apply_decay(now);

    assert_eq!(state.hunger, 82 - 2 * 4);
    assert_eq!(state.energy, 76 - 2 * 2);
    assert_eq!(state.happiness, 70 - 2 * 3);
    assert_eq!(state.social, 62 - 2 * 3);
    assert_eq!(state.last_seen_at, Some(now));
}

#[test]
fn decay_three_months_keeps_meters_above_zero() {
    let now = anchor();
    let mut state = seeded_with_last_seen(now - Duration::days(90));

    state.apply_decay(now);

    let weeks = 90 / 7;
    assert_eq!(weeks, 12);
    assert_eq!(state.hunger, 82 - weeks * 4);
    assert_eq!(state.energy, 76 - weeks * 2);
    assert_eq!(state.happiness, 70 - weeks * 3);
    assert_eq!(state.social, 62 - weeks * 3);
    assert!(state.hunger > 0);
    assert!(state.energy > 0);
    assert!(state.happiness > 0);
    assert!(state.social > 0);
    assert_eq!(state.last_seen_at, Some(now));
}

#[test]
fn decay_year_clamps_at_zero() {
    let now = anchor();
    let mut state = seeded_with_last_seen(now - Duration::days(365));

    state.apply_decay(now);

    assert_eq!(state.hunger, 0);
    assert_eq!(state.energy, 0);
    assert_eq!(state.happiness, 0);
    assert_eq!(state.social, 0);
    assert_eq!(state.last_seen_at, Some(now));
}
