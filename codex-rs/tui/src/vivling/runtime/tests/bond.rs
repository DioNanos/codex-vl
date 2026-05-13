//! codex-vl bond meter foundation — integration tests.

use super::common::*;
use crate::vivling::VivlingBond;
use crate::vivling::VivlingInteractionKind;
use crate::vivling::model::VERSION;
use chrono::TimeZone;
use chrono::Utc;

#[test]
fn fresh_state_has_default_bond_20() {
    let state = seeded_state();
    assert_eq!(state.bond.value, 20);
    assert_eq!(state.bond.chat_count, 0);
    assert_eq!(state.bond.assist_count, 0);
    assert_eq!(state.bond.loop_ticks_count, 0);
    assert!(state.bond.last_interaction.is_none());
}

#[test]
fn version_is_8_after_init() {
    let state = seeded_state();
    assert_eq!(state.version, VERSION);
    assert_eq!(VERSION, 8);
}

#[test]
fn normalize_loaded_state_backfills_default_bond() {
    let mut state = seeded_state();
    // Simulate a legacy state where bond was never written: pre-version-8
    state.bond = VivlingBond::default();
    state.version = 7;
    state.normalize_loaded_state();
    assert_eq!(state.version, VERSION);
    assert_eq!(state.bond.value, 20);
}

#[test]
fn vl_chat_dispatch_records_bond_chat() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .assign_brain_profile("vivling-spark".to_string())
        .expect("assign profile");

    let before_value = vivling.state.as_ref().expect("state").bond.value;
    let before_count = vivling.state.as_ref().expect("state").bond.chat_count;

    let _ = vivling
        .command(VivlingAction::Chat("ciao bello".to_string()), temp.path())
        .expect("chat dispatch");

    let after = &vivling.state.as_ref().expect("state").bond;
    assert_eq!(after.chat_count, before_count + 1);
    assert!(
        after.value >= before_value + 1,
        "value {} should be >= {}+1",
        after.value,
        before_value
    );
    assert!(after.last_interaction.is_some());
}

#[test]
fn vivling_assist_dispatch_records_bond_assist() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .assign_brain_profile("vivling-spark".to_string())
        .expect("assign profile");

    let before_value = vivling.state.as_ref().expect("state").bond.value;
    let before_count = vivling.state.as_ref().expect("state").bond.assist_count;

    let _ = vivling
        .command(
            VivlingAction::Assist("review this blocker".to_string()),
            temp.path(),
        )
        .expect("assist dispatch");

    let after = &vivling.state.as_ref().expect("state").bond;
    assert_eq!(after.assist_count, before_count + 1);
    assert!(
        after.value >= before_value + 2,
        "value {} should be >= {}+2",
        after.value,
        before_value
    );
    assert!(after.last_interaction.is_some());
}

#[test]
fn vivling_assist_failure_without_brain_profile_does_not_record_bond() {
    // /vivling assist on an Adult Vivling WITHOUT brain profile must fail in
    // prepare_assist_request after compose_brain_prompt but at the brain_profile
    // check. The fix moves record_interaction AFTER all validation so bond
    // state must stay unchanged on failure.
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    // No assign_brain_profile — assist will fail.
    let before = vivling.state.as_ref().expect("state").bond.clone();

    let result = vivling.command(
        VivlingAction::Assist("review this blocker".to_string()),
        temp.path(),
    );
    assert!(result.is_err(), "expected assist to fail without profile");

    let after = &vivling.state.as_ref().expect("state").bond;
    assert_eq!(after.value, before.value);
    assert_eq!(after.assist_count, before.assist_count);
    assert_eq!(after.last_interaction, before.last_interaction);
}

#[test]
fn local_chat_fallback_without_brain_does_not_record_bond() {
    // /vl on a non-adult Vivling falls back to local chat and never reaches
    // prepare_chat_request, so bond should NOT increment.
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    // No PromoteAdult, no brain profile → local fallback path
    let before = vivling.state.as_ref().expect("state").bond.clone();

    let _ = vivling
        .command(VivlingAction::Chat("ciao".to_string()), temp.path())
        .expect("chat fallback");

    let after = &vivling.state.as_ref().expect("state").bond;
    assert_eq!(after.value, before.value);
    assert_eq!(after.chat_count, before.chat_count);
    assert!(after.last_interaction.is_none());
}

#[test]
fn loop_tick_success_path_records_bond_loop_tick() {
    // mark_brain_reply_for is the exclusive success path for Vivling loop ticks.
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .assign_brain_profile("vivling-spark".to_string())
        .expect("assign profile");

    let vivling_id = vivling.state.as_ref().expect("state").vivling_id.clone();
    let before_value = vivling.state.as_ref().expect("state").bond.value;
    let before_count = vivling.state.as_ref().expect("state").bond.loop_ticks_count;

    vivling
        .mark_brain_reply_for(&vivling_id, "loop tick done — verify next")
        .expect("mark reply for loop owner");

    let after = &vivling.state.as_ref().expect("state").bond;
    assert_eq!(after.loop_ticks_count, before_count + 1);
    assert!(
        after.value >= before_value + 1,
        "value {} should be >= {}+1",
        after.value,
        before_value
    );
    assert!(after.last_interaction.is_some());
}

#[test]
fn loop_tick_failure_path_does_not_record_bond() {
    // mark_brain_runtime_error_for is the failure path. Bond MUST NOT grow.
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .assign_brain_profile("vivling-spark".to_string())
        .expect("assign profile");

    let vivling_id = vivling.state.as_ref().expect("state").vivling_id.clone();
    let before = vivling.state.as_ref().expect("state").bond.clone();

    vivling
        .mark_brain_runtime_error_for(&vivling_id, "auth missing")
        .expect("mark error");

    let after = &vivling.state.as_ref().expect("state").bond;
    assert_eq!(after.value, before.value);
    assert_eq!(after.loop_ticks_count, before.loop_ticks_count);
    assert_eq!(after.last_interaction, before.last_interaction);
}

#[test]
fn spawn_offspring_starts_bond_at_10() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let primary_id = vivling.state.as_ref().expect("state").vivling_id.clone();
    let _ = set_active_level(&mut vivling, JUVENILE_LEVEL);

    // Make sure spawn path is unlocked; if /vivling spawn requires Adult, promote first.
    let _ = set_active_level(&mut vivling, ADULT_LEVEL);

    let _ = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect("spawn");

    let spawned_ids = spawn_ids(&vivling, &primary_id);
    assert!(!spawned_ids.is_empty(), "expected at least one spawned id");
    let child = vivling
        .load_state_for_id(&spawned_ids[0])
        .expect("load child")
        .expect("child state");

    assert_eq!(child.bond.value, 10);
    assert_eq!(child.bond.chat_count, 0);
    assert_eq!(child.bond.assist_count, 0);
    assert_eq!(child.bond.loop_ticks_count, 0);
    assert!(child.bond.last_interaction.is_none());
}

#[test]
fn apply_decay_propagates_bond_decay() {
    // VivlingState::apply_decay should propagate to bond. Verify directly on state.
    let mut state = seeded_state();
    let day1 = Utc.with_ymd_and_hms(2026, 5, 13, 10, 0, 0).unwrap();
    let day3 = Utc.with_ymd_and_hms(2026, 5, 16, 22, 0, 0).unwrap();

    // Simulate an interaction at day1
    state
        .bond
        .record_interaction(VivlingInteractionKind::Chat, day1);
    let value_after_chat = state.bond.value;

    // 3+ days later — apply_decay propagates and bond should decrease
    state.apply_decay(day3);
    assert!(
        state.bond.value < value_after_chat,
        "bond {} should be less than {}",
        state.bond.value,
        value_after_chat
    );
}

#[test]
fn serde_round_trip_preserves_bond() {
    let mut state = seeded_state();
    state.bond.record_interaction(
        VivlingInteractionKind::Chat,
        Utc.with_ymd_and_hms(2026, 5, 13, 10, 0, 0).unwrap(),
    );
    state.bond.record_interaction(
        VivlingInteractionKind::Assist,
        Utc.with_ymd_and_hms(2026, 5, 14, 10, 0, 0).unwrap(),
    );

    let json = serde_json::to_string(&state).expect("serialize");
    let restored: VivlingState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(state.bond, restored.bond);
}
