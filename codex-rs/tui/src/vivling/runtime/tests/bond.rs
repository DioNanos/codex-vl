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
fn version_is_current_after_init() {
    let state = seeded_state();
    assert_eq!(state.version, VERSION);
    // Memory V2 Step 12.B.A bumped the schema to 10. The exact value
    // is also pinned by `schema_v10::new_state_uses_version_10`; this
    // assertion stays here so the bond-suite catches an out-of-band
    // schema bump.
    assert_eq!(VERSION, 10);
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
fn vivling_assist_failure_with_brain_disabled_does_not_record_bond() {
    // V2 §8.1 (P0.2): the bond-on-failure invariant must hold for
    // *any* prepare-time validation failure, not just the (now
    // removed) missing-profile guard. With Memory V2 the missing
    // brain_profile path actually succeeds via SessionDefault, so we
    // exercise the invariant against the still-blocking guard:
    // `brain_enabled = false` makes compose_brain_prompt error out
    // for Assist before the bond credit lands.
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    // brain stays disabled — assist must fail.
    assert!(!vivling.state.as_ref().expect("state").brain_enabled);
    let before = vivling.state.as_ref().expect("state").bond.clone();

    let result = vivling.command(
        VivlingAction::Assist("review this blocker".to_string()),
        temp.path(),
    );
    assert!(
        result.is_err(),
        "expected assist to fail with brain disabled"
    );

    let after = &vivling.state.as_ref().expect("state").bond;
    assert_eq!(after.value, before.value);
    assert_eq!(after.assist_count, before.assist_count);
    assert_eq!(after.last_interaction, before.last_interaction);
}

#[test]
fn baby_chat_dispatch_records_bond_chat_after_step_12_b_e() {
    // Memory V2 Step 12.B.E: Baby `/vl` no longer takes the local-ack
    // path — it dispatches via LLM through `prepare_chat_request`,
    // which records a Chat interaction on `bond`. The previous test
    // (`local_chat_fallback_without_brain_does_not_record_bond`)
    // asserted no-op on bond and is therefore obsolete; the new
    // assertion proves the dispatch increments `chat_count`.
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let before = vivling.state.as_ref().expect("state").bond.clone();

    let _ = vivling
        .command(VivlingAction::Chat("ciao".to_string()), temp.path())
        .expect("chat dispatch");

    let after = &vivling.state.as_ref().expect("state").bond;
    assert!(
        after.value > before.value,
        "Baby /vl dispatch must increment bond.value"
    );
    assert_eq!(after.chat_count, before.chat_count + 1);
    assert!(after.last_interaction.is_some());
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

#[test]
fn chat_prompt_includes_bond_section() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .assign_brain_profile("vivling-spark".to_string())
        .expect("assign profile");

    let request = match vivling
        .command(VivlingAction::Chat("ciao bello".to_string()), temp.path())
        .expect("chat dispatch")
    {
        VivlingCommandOutcome::DispatchAssist(request) => request,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(
        request.prompt_context.contains("Bond with user:"),
        "chat prompt should include bond section header:\n{}",
        request.prompt_context
    );
    // Default bond.value is 20 → Strangers
    assert!(
        request.prompt_context.contains("Strangers"),
        "chat prompt should mention default Strangers level:\n{}",
        request.prompt_context
    );
}

#[test]
fn assist_prompt_includes_bond_section() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .assign_brain_profile("vivling-spark".to_string())
        .expect("assign profile");

    let request = match vivling
        .command(
            VivlingAction::Assist("review this blocker".to_string()),
            temp.path(),
        )
        .expect("assist dispatch")
    {
        VivlingCommandOutcome::DispatchAssist(request) => request,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(
        request.prompt_context.contains("Bond with user:"),
        "assist prompt should include bond section header:\n{}",
        request.prompt_context
    );
    assert!(
        request.prompt_context.contains("Strangers"),
        "assist prompt should mention default Strangers level:\n{}",
        request.prompt_context
    );
}

#[test]
fn loop_tick_prompt_omits_bond_section() {
    // The literal section header string must not appear in LoopTick prompts —
    // defensive against any future relabeling of level enum strings.
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .assign_brain_profile("vivling-spark".to_string())
        .expect("assign profile");
    let owner_id = vivling.state.as_ref().expect("state").vivling_id.clone();
    vivling.save_state().expect("persist state");

    let job = codex_state::ThreadLoopJob {
        id: "test-loop-job".to_string(),
        thread_id: codex_protocol::ThreadId::new(),
        label: "verify".to_string(),
        prompt_text: "please verify and report".to_string(),
        goal_text: Some("verify the bond foundation merge".to_string()),
        interval_seconds: 60,
        enabled: true,
        run_policy: "auto".to_string(),
        auto_remove_on_completion: false,
        created_by: "test".to_string(),
        next_run_ms: None,
        last_run_ms: None,
        last_status: None,
        last_error: None,
        pending_tick: false,
        created_at_ms: 0,
        updated_at_ms: 0,
    };

    let request = vivling
        .prepare_loop_tick_request(&owner_id, &job)
        .expect("loop tick request");

    assert!(
        !request.prompt_context.contains("Bond with user:"),
        "loop tick prompt must NOT include bond section header:\n{}",
        request.prompt_context
    );
}

#[test]
fn status_includes_bond_segment() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let status = vivling.status().expect("status");
    assert!(
        status.contains("bond Strangers 20/100"),
        "status should include bond segment for fresh hatch:\n{}",
        status
    );
}

#[test]
fn card_includes_bond_row() {
    let temp = TempDir::new().expect("tempdir");
    let vivling = hatched_vivling(temp.path());
    let mut state = vivling.state.clone().expect("state");

    let card = crate::vivling::runtime::render_vivling_card(&mut state);
    let narrow_has = card
        .narrow_lines
        .iter()
        .filter(|line| line.as_str() == "Bond Strangers 20/100")
        .count();
    let wide_has = card
        .wide_lines
        .iter()
        .filter(|line| line.as_str() == "Bond Strangers 20/100")
        .count();
    assert_eq!(
        narrow_has, 1,
        "narrow_lines should contain exactly one bond row:\n{:?}",
        card.narrow_lines
    );
    assert_eq!(
        wide_has, 1,
        "wide_lines should contain exactly one bond row:\n{:?}",
        card.wide_lines
    );
}

#[test]
fn vivling_record_brain_success_chat_adds_bond_chat_succeeded() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .assign_brain_profile("vivling-spark".to_string())
        .expect("assign profile");

    let before_value = vivling.state.as_ref().expect("state").bond.value;
    let before_chat_count = vivling.state.as_ref().expect("state").bond.chat_count;

    vivling
        .record_brain_success(crate::vivling::VivlingBrainRequestKind::Chat)
        .expect("record brain chat success");

    let after = &vivling.state.as_ref().expect("state").bond;
    // value +2, chat_count unchanged (counters are dispatch-only)
    assert_eq!(
        after.value,
        before_value + 2,
        "bond value should grow by 2 on BrainChatSucceeded"
    );
    assert_eq!(
        after.chat_count, before_chat_count,
        "chat_count must stay tied to dispatch, not success"
    );
    assert!(after.last_interaction.is_some());
}

#[test]
fn vivling_record_brain_success_assist_adds_bond_assist_succeeded() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .assign_brain_profile("vivling-spark".to_string())
        .expect("assign profile");

    let before_value = vivling.state.as_ref().expect("state").bond.value;
    let before_assist_count = vivling.state.as_ref().expect("state").bond.assist_count;

    vivling
        .record_brain_success(crate::vivling::VivlingBrainRequestKind::Assist)
        .expect("record brain assist success");

    let after = &vivling.state.as_ref().expect("state").bond;
    // value +3, assist_count unchanged (counters are dispatch-only)
    assert_eq!(
        after.value,
        before_value + 3,
        "bond value should grow by 3 on BrainAssistSucceeded"
    );
    assert_eq!(
        after.assist_count, before_assist_count,
        "assist_count must stay tied to dispatch, not success"
    );
}

#[test]
fn record_brain_success_without_hatch_fails_without_touching_bond() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = configured_vivling(temp.path());
    // Not hatched yet — ensure_hatched must reject.
    let result = vivling.record_brain_success(crate::vivling::VivlingBrainRequestKind::Chat);
    assert!(result.is_err(), "expected hatch precondition error");
}
