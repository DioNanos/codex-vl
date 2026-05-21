//! Memory V2 Step 12.B.A schema-bump tests (V9 -> V10).
//!
//! These tests pin the V10 contract:
//!   - VERSION constant is 10 (so `bond::version_is_current_after_init`
//!     and similar pins survive future bumps).
//!   - V9-shaped JSON loads into a V10 binary with serde defaults for
//!     every new field (additive migration).
//!   - V10 fields scaffold runtime LLM budget counters + expression
//!     mode; no runtime behaviour is wired here, that lives in Steps
//!     12.B.B/C/D.
//!
//! Backward-compat with V8 is still covered by `schema_v9::*` (V8 ->
//! V9 -> V10 chain); we don't duplicate it here.

use super::common::*;
use codex_vivling_core::model::VERSION as CURRENT_STATE_VERSION;
use codex_vivling_core::model::VivlingExpressionMode;

#[test]
fn current_schema_version_is_10() {
    assert_eq!(CURRENT_STATE_VERSION, 10);
}

#[test]
fn new_state_uses_version_10() {
    let state = seeded_state();
    assert_eq!(state.version, 10);
}

#[test]
fn fresh_state_defaults_expression_mode_to_default() {
    let state = seeded_state();
    assert_eq!(state.crt_brain_mode, VivlingExpressionMode::default());
    assert_eq!(state.crt_brain_mode, VivlingExpressionMode::Default);
}

#[test]
fn fresh_state_defaults_llm_counters_to_zero_and_empty_key() {
    let state = seeded_state();
    assert_eq!(state.daily_llm_call_count, 0);
    assert_eq!(state.daily_llm_chat_calls, 0);
    assert_eq!(state.daily_llm_assist_calls, 0);
    assert_eq!(state.daily_llm_loop_tick_calls, 0);
    assert_eq!(state.daily_llm_expression_calls, 0);
    assert_eq!(state.daily_llm_failure_count, 0);
    assert_eq!(state.daily_llm_throttle_skips, 0);
    assert_eq!(state.daily_llm_dedup_skips, 0);
    assert_eq!(state.daily_llm_budget_skips, 0);
    assert_eq!(state.daily_llm_optout_skips, 0);
    assert_eq!(state.daily_llm_day_key, "");
    assert!(state.last_llm_dispatch_at.is_none());
}

#[test]
fn v9_state_loads_in_v10_binary_with_defaults() {
    // Minimal V9-shaped JSON. None of the V10 fields are present;
    // serde defaults must fill them all in without complaint.
    let v9_json = r#"{
        "version": 9,
        "hatched": true,
        "vivling_id": "viv-v9-fixture",
        "primary_vivling_id": "viv-v9-fixture",
        "species": "syllo",
        "rarity": "common",
        "name": "Aelia",
        "level": 23,
        "xp": 1380,
        "work_xp": 1380,
        "active_work_days": 21,
        "language_state": {
            "detected_language": "it",
            "language_mode": "mirror_user",
            "recent_samples": [],
            "language_override": null
        },
        "accumulated_bias": {"caution": 5, "verification": 7},
        "recent_bias": {"caution": 1, "verification": 2}
    }"#;

    let state: VivlingState =
        serde_json::from_str(v9_json).expect("V9 JSON must deserialize into V10 VivlingState");

    // V9-era fields preserved verbatim.
    assert_eq!(state.version, 9, "load must not silently rewrite version");
    assert_eq!(state.vivling_id, "viv-v9-fixture");
    assert_eq!(state.level, 23);
    assert_eq!(state.work_xp, 1380);
    assert_eq!(state.accumulated_bias.caution, 5);
    assert_eq!(state.recent_bias.verification, 2);
    assert_eq!(state.language_state.detected_language, "it");

    // V10 scaffolds default cleanly.
    assert_eq!(state.crt_brain_mode, VivlingExpressionMode::Default);
    assert_eq!(state.daily_llm_call_count, 0);
    assert_eq!(state.daily_llm_chat_calls, 0);
    assert_eq!(state.daily_llm_assist_calls, 0);
    assert_eq!(state.daily_llm_loop_tick_calls, 0);
    assert_eq!(state.daily_llm_expression_calls, 0);
    assert_eq!(state.daily_llm_failure_count, 0);
    assert_eq!(state.daily_llm_throttle_skips, 0);
    assert_eq!(state.daily_llm_dedup_skips, 0);
    assert_eq!(state.daily_llm_budget_skips, 0);
    assert_eq!(state.daily_llm_optout_skips, 0);
    assert_eq!(state.daily_llm_day_key, "");
    assert!(state.last_llm_dispatch_at.is_none());
}

#[test]
fn v10_state_round_trip_preserves_new_fields() {
    let mut state = seeded_state();
    state.crt_brain_mode = VivlingExpressionMode::Off;
    state.daily_llm_call_count = 12;
    state.daily_llm_chat_calls = 3;
    state.daily_llm_expression_calls = 8;
    state.daily_llm_failure_count = 1;
    state.daily_llm_day_key = "2026-05-21".to_string();

    let serialized = serde_json::to_string(&state).expect("serialize");
    let restored: VivlingState = serde_json::from_str(&serialized).expect("deserialize back");

    assert_eq!(restored.crt_brain_mode, VivlingExpressionMode::Off);
    assert_eq!(restored.daily_llm_call_count, 12);
    assert_eq!(restored.daily_llm_chat_calls, 3);
    assert_eq!(restored.daily_llm_expression_calls, 8);
    assert_eq!(restored.daily_llm_failure_count, 1);
    assert_eq!(restored.daily_llm_day_key, "2026-05-21");
}
