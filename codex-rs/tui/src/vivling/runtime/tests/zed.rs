//! codex-vl ZED Companion panel — integration tests.

use super::common::*;

#[test]
fn zed_action_parses_correctly() {
    assert_eq!(VivlingAction::parse("zed"), Ok(VivlingAction::Zed));
}

#[test]
fn zed_command_returns_open_upgrade_outcome() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let outcome = vivling
        .command(VivlingAction::Zed, temp.path())
        .expect("zed companion outcome");

    // ZED Companion must go through OpenUpgrade (existing ZED channel)
    // and NOT OpenCard (which would open VivlingCardView). This is the
    // Codex design-review iter 1 §1 constraint expressed as a regression test.
    match outcome {
        VivlingCommandOutcome::OpenUpgrade(panel) => {
            assert!(panel.title.contains("Companion"), "{}", panel.title);
            assert!(!panel.narrow_lines.is_empty(), "narrow_lines empty");
            assert!(!panel.wide_lines.is_empty(), "wide_lines empty");
        }
        other => {
            panic!("expected VivlingCommandOutcome::OpenUpgrade for /vivling zed, got {other:?}")
        }
    }
}

#[test]
fn zed_companion_panel_contains_bond_and_gene_sections() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let outcome = vivling
        .command(VivlingAction::Zed, temp.path())
        .expect("zed companion outcome");

    let panel = match outcome {
        VivlingCommandOutcome::OpenUpgrade(panel) => panel,
        other => panic!("unexpected outcome: {other:?}"),
    };

    let narrow_joined = panel.narrow_lines.join("\n");
    let wide_joined = panel.wide_lines.join("\n");

    for snippet in ["Bond:", "Gene:", "Stripe", "Temperament"] {
        assert!(
            narrow_joined.contains(snippet),
            "narrow_lines missing `{snippet}`:\n{narrow_joined}"
        );
        assert!(
            wide_joined.contains(snippet),
            "wide_lines missing `{snippet}`:\n{wide_joined}"
        );
    }
}

#[test]
fn zed_companion_persists_last_zed_topic() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let _ = vivling
        .command(VivlingAction::Zed, temp.path())
        .expect("zed companion outcome");

    let topic = vivling
        .state
        .as_ref()
        .expect("state")
        .last_zed_topic
        .clone();
    assert_eq!(topic.as_deref(), Some("companion"));
}

#[test]
fn zed_companion_requires_hatch() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = configured_vivling(temp.path());
    // Not hatched yet.
    let result = vivling.command(VivlingAction::Zed, temp.path());
    assert!(result.is_err(), "expected hatch precondition error");
}
