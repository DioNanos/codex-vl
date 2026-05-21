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

// --- Step 5.B: language override CLI + generic-turn sampling ---

use crate::vivling::runtime::action::LanguageAction;
use codex_vivling_core::model::VivlingLanguageMode;

#[test]
fn parse_language_show_when_no_args() {
    assert_eq!(
        VivlingAction::parse("language").expect("parse"),
        VivlingAction::Language(LanguageAction::Show)
    );
    assert_eq!(
        VivlingAction::parse("language show").expect("parse"),
        VivlingAction::Language(LanguageAction::Show)
    );
}

#[test]
fn parse_language_auto_and_clear_aliases() {
    for alias in ["auto", "clear", "reset", "default"] {
        let parsed = VivlingAction::parse(&format!("language {alias}")).expect("parse");
        assert_eq!(parsed, VivlingAction::Language(LanguageAction::Auto));
    }
}

#[test]
fn parse_language_set_code() {
    assert_eq!(
        VivlingAction::parse("language it").expect("parse"),
        VivlingAction::Language(LanguageAction::Set("it".to_string()))
    );
}

#[test]
fn parse_language_mode_strict() {
    assert_eq!(
        VivlingAction::parse("language mode strict").expect("parse"),
        VivlingAction::Language(LanguageAction::Mode("strict".to_string()))
    );
}

#[test]
fn parse_language_mode_missing_argument_errors() {
    let err = VivlingAction::parse("language mode").expect_err("must error");
    assert!(err.contains("mirror-user"), "got: {err}");
}

#[test]
fn set_language_override_rejects_unknown_code() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let err = vivling
        .set_language_override(Some("zz".to_string()))
        .expect_err("must reject");
    assert!(
        err.contains("Unsupported language code"),
        "error must mention unsupported code; got: {err}"
    );
    // Supported list must be in the error so the user can recover.
    assert!(err.contains("it"));
    assert!(err.contains("en"));
}

#[test]
fn override_wins_over_detected_in_effective_language() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    // First detect italian via a real sample so detected_language is non-empty.
    vivling.record_user_language_sample("ciao come stai oggi davvero");
    assert_eq!(
        vivling
            .state
            .as_ref()
            .expect("state")
            .language_state
            .detected_language,
        "it"
    );

    // Then pin the override to spanish; effective must follow the override.
    let msg = vivling
        .set_language_override(Some("es".to_string()))
        .expect("set override");
    assert!(msg.contains("`es`"), "got: {msg}");
    let state = vivling.state.as_ref().expect("state");
    assert_eq!(state.language_state.effective_language(None), "es");

    // Clearing the override falls back to the detected verdict.
    let cleared = vivling.set_language_override(None).expect("clear");
    assert!(cleared.contains("cleared"));
    assert_eq!(
        vivling
            .state
            .as_ref()
            .expect("state")
            .language_state
            .effective_language(None),
        "it"
    );
}

#[test]
fn set_language_mode_persists_and_strict_freezes_detection() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    // Seed an italian sample, then switch to Strict — the mode change
    // should latch the current dominant detection.
    vivling.record_user_language_sample("ciao come stai oggi davvero");
    let msg = vivling
        .set_language_mode("strict")
        .expect("set strict mode");
    assert!(msg.contains("strict"), "got: {msg}");
    assert_eq!(
        vivling
            .state
            .as_ref()
            .expect("state")
            .language_state
            .language_mode,
        VivlingLanguageMode::Strict
    );

    // A flood of english afterwards must NOT shift detected_language.
    for _ in 0..10 {
        vivling.record_user_language_sample(
            "hello how are you today, the world is good and that is what they have",
        );
    }
    assert_eq!(
        vivling
            .state
            .as_ref()
            .expect("state")
            .language_state
            .detected_language,
        "it",
        "Strict mode must freeze the first dominant detection"
    );
}

#[test]
fn record_user_language_sample_ignores_whitespace_and_empty() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let before = vivling
        .state
        .as_ref()
        .expect("state")
        .language_state
        .recent_samples
        .len();
    vivling.record_user_language_sample("   \n\t  ");
    vivling.record_user_language_sample("");
    let after = vivling
        .state
        .as_ref()
        .expect("state")
        .language_state
        .recent_samples
        .len();
    assert_eq!(before, after, "whitespace samples must not be recorded");
}

#[test]
fn language_status_includes_supported_codes_and_mode() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let msg = vivling.show_language_status().expect("status");
    // The status text is the surface the user sees, so pin the labels.
    assert!(msg.contains("effective:"));
    assert!(msg.contains("detected:"));
    assert!(msg.contains("override:"));
    assert!(msg.contains("mode:"));
    assert!(msg.contains("samples:"));
    assert!(msg.contains("supported codes:"));
    assert!(msg.contains("it"));
    assert!(msg.contains("en"));
}
