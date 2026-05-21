//! Memory V2 Step 5.A — TUI integration tests for axis G language wiring.
//!
//! Verifies that the runtime hooks actually feed `prepare_chat_request`
//! / `prepare_assist_request` into the rolling sample window and that
//! `compose_brain_prompt` surfaces the resulting language contract.

use super::common::*;

#[test]
fn brain_prompt_contains_language_contract_section() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .set_brain_enabled_with_guidance(true)
        .expect("enable brain");

    let request = match vivling
        .command(
            VivlingAction::Chat("ciao come stai oggi davvero".to_string()),
            temp.path(),
        )
        .expect("chat")
    {
        VivlingCommandOutcome::DispatchAssist(req) => req,
        other => panic!("expected DispatchAssist, got {other:?}"),
    };

    assert!(
        request.prompt_context.contains("Language contract:"),
        "prompt must include Language contract section; got:\n{}",
        request.prompt_context
    );
    assert!(
        request.prompt_context.contains("mode: mirror-user"),
        "default mode must be mirror-user; got:\n{}",
        request.prompt_context
    );
}

#[test]
fn chat_sample_updates_detected_language_to_italian() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .set_brain_enabled_with_guidance(true)
        .expect("enable brain");

    let _ = vivling
        .command(
            VivlingAction::Chat("ciao come stai oggi davvero".to_string()),
            temp.path(),
        )
        .expect("chat");

    let state = vivling.state.as_ref().expect("state");
    assert_eq!(
        state.language_state.recent_samples.len(),
        1,
        "chat must record one sample"
    );
    assert_eq!(state.language_state.detected_language, "it");
}

#[test]
fn assist_sample_updates_detected_language_to_english() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .set_brain_enabled_with_guidance(true)
        .expect("enable brain");

    let request = match vivling
        .command(
            VivlingAction::Assist(
                "review this blocker and tell me what you would just check next".to_string(),
            ),
            temp.path(),
        )
        .expect("assist")
    {
        VivlingCommandOutcome::DispatchAssist(req) => req,
        other => panic!("expected DispatchAssist, got {other:?}"),
    };

    let state = vivling.state.as_ref().expect("state");
    assert_eq!(state.language_state.recent_samples.len(), 1);
    assert_eq!(state.language_state.detected_language, "en");
    assert!(
        request.prompt_context.contains("effective language: en"),
        "language contract must echo detected language; got:\n{}",
        request.prompt_context
    );
}
