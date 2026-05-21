use super::common::*;

#[test]
fn hatch_uses_unlocked_species_after_adult_progression() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let mut state = vivling.state.clone().expect("state");
    state.install_id = Some("odd-1".to_string());
    state.work_xp = WORK_XP_PER_LEVEL.saturating_mul(ADULT_LEVEL.saturating_sub(1));
    state.active_work_days = 90;
    state.recompute_level();
    assert!(state.unlocked_species.iter().any(|id| id == "orchestra"));
    vivling.state = Some(state);
    vivling.save_state().expect("save adult state");

    let message = match vivling
        .command(VivlingAction::Hatch, temp.path())
        .expect("second hatch")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("Orchestra"));
    assert_eq!(
        vivling.state.as_ref().map(|state| state.species.as_str()),
        Some("orchestra")
    );
}

#[test]
fn action_parse_supports_spawn_transfer_and_roster_commands() {
    assert_eq!(VivlingAction::parse(""), Ok(VivlingAction::Dashboard));
    assert_eq!(VivlingAction::parse("help"), Ok(VivlingAction::Help));
    assert_eq!(VivlingAction::parse("roster"), Ok(VivlingAction::Roster));
    assert_eq!(VivlingAction::parse("list"), Ok(VivlingAction::Roster));
    assert_eq!(VivlingAction::parse("spawn"), Ok(VivlingAction::Spawn));
    assert_eq!(
        VivlingAction::parse("assist review the blocker"),
        Ok(VivlingAction::Assist("review the blocker".to_string()))
    );
    assert_eq!(
        VivlingAction::parse("brain on"),
        Ok(VivlingAction::Brain(true))
    );
    assert_eq!(
        VivlingAction::parse("model list"),
        Ok(VivlingAction::ModelList)
    );
    assert_eq!(
        VivlingAction::parse("model spark-fast"),
        Ok(VivlingAction::ModelProfile("spark-fast".to_string()))
    );
    assert_eq!(
        VivlingAction::parse("model gpt-5.3-codex-spark zai-a high"),
        Ok(VivlingAction::ModelCustom {
            model: "gpt-5.3-codex-spark".to_string(),
            provider: Some("zai-a".to_string()),
            effort: Some(ReasoningEffortConfig::High),
        })
    );
    assert_eq!(VivlingAction::parse("recap"), Ok(VivlingAction::Recap));
    assert_eq!(
        VivlingAction::parse("promote 10"),
        Ok(VivlingAction::PromoteEarly)
    );
    assert_eq!(
        VivlingAction::parse("promote 60"),
        Ok(VivlingAction::PromoteAdult)
    );
    assert_eq!(
        VivlingAction::parse("focus viv-123"),
        Ok(VivlingAction::Focus("viv-123".to_string()))
    );
    assert_eq!(
        VivlingAction::parse("switch viv-123"),
        Ok(VivlingAction::Focus("viv-123".to_string()))
    );
    assert_eq!(
        VivlingAction::parse("export out.vivegg"),
        Ok(VivlingAction::Export(Some("out.vivegg".to_string())))
    );
    assert_eq!(
        VivlingAction::parse("import in.vivegg"),
        Ok(VivlingAction::Import("in.vivegg".to_string()))
    );
    assert_eq!(
        VivlingAction::parse("remove viv-123"),
        Ok(VivlingAction::Remove("viv-123".to_string()))
    );
}

#[test]
fn action_parse_supports_crt_brain_subcommands() {
    use super::super::action::CrtBrainAction;
    assert_eq!(
        VivlingAction::parse("crt-brain"),
        Ok(VivlingAction::CrtBrain(CrtBrainAction::Show))
    );
    assert_eq!(
        VivlingAction::parse("crt-brain show"),
        Ok(VivlingAction::CrtBrain(CrtBrainAction::Show))
    );
    assert_eq!(
        VivlingAction::parse("crt-brain on"),
        Ok(VivlingAction::CrtBrain(CrtBrainAction::On))
    );
    assert_eq!(
        VivlingAction::parse("crt-brain off"),
        Ok(VivlingAction::CrtBrain(CrtBrainAction::Off))
    );
    assert_eq!(
        VivlingAction::parse("crt-brain default"),
        Ok(VivlingAction::CrtBrain(CrtBrainAction::Default))
    );
    // Underscore + snake-case aliases must also work — keymap muscle
    // memory tends to drop the dash.
    assert_eq!(
        VivlingAction::parse("crt_brain on"),
        Ok(VivlingAction::CrtBrain(CrtBrainAction::On))
    );
    // Unknown sub-argument must error rather than silently degrading
    // to a chat message.
    assert!(VivlingAction::parse("crt-brain wat").is_err());
}

#[test]
fn help_lists_supported_commands_instead_of_falling_back_to_chat() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = configured_vivling(temp.path());

    let message = match vivling
        .command(VivlingAction::Help, temp.path())
        .expect("help should work")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("Vivling commands:"));
    assert!(message.contains("Ctrl+J"));
    assert!(message.contains("/vivling hatch"));
    assert!(message.contains("/vivling status"));
    assert!(message.contains("/vivling roster"));
    assert!(message.contains("/vivling list"));
    assert!(message.contains("/vivling switch"));
    assert!(message.contains("/vivling spawn"));
    assert!(message.contains("/vivling assist <task>"));
    assert!(message.contains("/vivling brain <on|off>"));
    assert!(message.contains("/vivling model"));
    assert!(message.contains("/vivling model list"));
    assert!(message.contains("/vivling recap"));
    assert!(message.contains("/vivling promote 10"));
    assert!(message.contains("/vivling promote 60"));
    assert!(message.contains("/vivling export [path.vivegg]"));
    assert!(message.contains("/vivling import <path.vivegg>"));
    assert!(message.contains("/vivling <message>"));
    assert!(message.contains("/vivling zed"));
}

#[test]
fn hatch_fills_top_level_slots_before_failing() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = configured_vivling(temp.path());

    for expected in 1..=EXTERNAL_SLOT_LIMIT {
        let message = match vivling
            .command(VivlingAction::Hatch, temp.path())
            .expect("hatch should work")
        {
            VivlingCommandOutcome::Message(message) => message,
            other => panic!("unexpected outcome: {other:?}"),
        };
        assert!(message.contains(&format!(
            "Top-level slots now {expected}/{EXTERNAL_SLOT_LIMIT}"
        )));
    }

    let err = vivling
        .command(VivlingAction::Hatch, temp.path())
        .expect_err("slots should be full");
    assert!(err.contains("All top-level Vivling slots are full"));

    let roster = vivling.load_roster().expect("roster");
    assert_eq!(roster.vivling_ids.len(), EXTERNAL_SLOT_LIMIT);
}

#[test]
fn promote_10_applies_early_seed_baseline() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let message = match vivling
        .command(VivlingAction::PromoteEarly, temp.path())
        .expect("promote")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("level 10"));
    let state = vivling.state.as_ref().expect("state");
    assert_eq!(state.level, 10);
    assert!(!state.adult_bootstrap);
    assert_eq!(state.seed_origin.as_deref(), Some("early_seed_v1"));
    assert!(!state.work_memory.is_empty());
    assert_eq!(state.stage(), Stage::Baby);
}

#[test]
fn promote_60_applies_adult_seed_baseline() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let message = match vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("adult baseline"));
    let state = vivling.state.as_ref().expect("state");
    assert_eq!(state.level, ADULT_LEVEL);
    assert!(state.adult_bootstrap);
    assert_eq!(state.seed_origin.as_deref(), Some("adult_seed_v1"));
    assert!(!state.work_memory.is_empty());
    assert!(!state.distilled_summaries.is_empty());
}

#[test]
fn promote_60_persists_across_new_instance_reload() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");

    let reloaded = configured_vivling(temp.path());
    let state = reloaded.state.as_ref().expect("reloaded state");
    assert_eq!(state.level, ADULT_LEVEL);
    assert!(state.adult_bootstrap);
    assert_eq!(state.seed_origin.as_deref(), Some("adult_seed_v1"));
}

#[test]
fn recap_reads_synthesized_memory_view() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteEarly, temp.path())
        .expect("promote early");

    let message = match vivling
        .command(VivlingAction::Recap, temp.path())
        .expect("recap")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("stage baby"));
    assert!(message.contains("distilled:"));
    assert!(message.contains("paths:"));
}

#[test]
fn missing_focus_target_points_to_roster() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let err = vivling
        .command(VivlingAction::Focus("unknown".to_string()), temp.path())
        .expect_err("missing target should fail");

    assert!(err.contains("No Vivling matches `unknown`"), "{err}");
    assert!(err.contains("/vivling roster"), "{err}");
}

#[test]
fn missing_import_package_points_to_import_usage() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let err = vivling
        .command(
            VivlingAction::Import("missing.vivegg".to_string()),
            temp.path(),
        )
        .expect_err("missing package should fail");

    assert!(err.contains("Failed to open Vivling package"), "{err}");
    assert!(err.contains("/vivling import <path.vivegg>"), "{err}");
}
