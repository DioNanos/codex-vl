use super::*;
use crate::vivling::model::ADULT_LEVEL;
use crate::vivling::model::WORK_XP_PER_LEVEL;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;
use zip::ZipArchive;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

fn seeded_state() -> VivlingState {
    VivlingState::new(SeedIdentity {
        value: "install:test-seed".to_string(),
        install_id: Some("test-seed".to_string()),
    })
}

fn leveled_state(level: u64, active_days: u64) -> VivlingState {
    let mut state = seeded_state();
    state.active_work_days = active_days;
    state.work_xp = WORK_XP_PER_LEVEL.saturating_mul(level.saturating_sub(1));
    state.recompute_level();
    state
}

fn configured_vivling(home: &Path) -> Vivling {
    let mut vivling = Vivling::unavailable();
    vivling.configure(home, AuthCredentialsStoreMode::default());
    vivling.configure_runtime(FrameRequester::test_dummy(), false);
    vivling
}

fn hatched_vivling(home: &Path) -> Vivling {
    let mut vivling = configured_vivling(home);
    let _ = vivling
        .command(VivlingAction::Hatch, home)
        .expect("hatch vivling");
    vivling
}

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

fn set_active_level(vivling: &mut Vivling, level: u64) -> VivlingState {
    let mut state = vivling.state.clone().expect("active state");
    state.active_work_days = if level >= ADULT_LEVEL {
        90
    } else if level >= JUVENILE_LEVEL {
        30
    } else {
        level.max(1)
    };
    state.work_xp = WORK_XP_PER_LEVEL.saturating_mul(level.saturating_sub(1));
    state.xp = state.work_xp;
    state.recompute_level();
    vivling.active_vivling_id = Some(state.vivling_id.clone());
    vivling.state = Some(state.clone());
    vivling.save_state().expect("save leveled state");
    state
}

fn spawn_ids(vivling: &Vivling, primary_id: &str) -> Vec<String> {
    vivling
        .load_roster()
        .expect("roster")
        .vivling_ids
        .into_iter()
        .filter(|id| id != primary_id)
        .collect()
}

fn make_package(path: &Path, manifest: &VivlingPackageManifest, state: &VivlingState) {
    let file = File::create(path).expect("create vivegg");
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("manifest.json", options)
        .expect("manifest entry");
    zip.write_all(
        serde_json::to_string_pretty(manifest)
            .expect("manifest json")
            .as_bytes(),
    )
    .expect("write manifest");
    zip.start_file("state.json", options).expect("state entry");
    zip.write_all(
        serde_json::to_string_pretty(state)
            .expect("state json")
            .as_bytes(),
    )
    .expect("write state");
    zip.finish().expect("finish package");
}

fn exportable_state(level: u64) -> VivlingState {
    let mut state = leveled_state(
        level,
        if level >= ADULT_LEVEL {
            90
        } else if level >= JUVENILE_LEVEL {
            30
        } else {
            level.max(1)
        },
    );
    state.primary_vivling_id = state.vivling_id.clone();
    state.is_primary = true;
    state
}

#[test]
fn active_footer_pose_changes_while_task_running() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.configure_runtime(FrameRequester::test_dummy(), true);
    vivling.set_task_running(true);

    let state = vivling.visible_state().expect("hatched state");
    let sprite = vivling.current_sprite(state, Instant::now());
    assert_ne!(sprite, species_for_id(&state.species).ascii_baby);
}

#[test]
fn footer_pose_animates_while_visible_and_idle() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.configure_runtime(FrameRequester::test_dummy(), true);

    let state = vivling.visible_state().expect("hatched state");
    let start = Instant::now();
    let first = vivling.current_sprite(state, start);
    let second = vivling.current_sprite(state, start + ACTIVE_FOOTER_FRAME_INTERVAL);
    assert_ne!(first, second);
}

#[test]
fn static_footer_pose_used_when_animations_disabled() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.configure_runtime(FrameRequester::test_dummy(), false);
    vivling.set_task_running(true);

    let state = vivling.visible_state().expect("hatched state");
    let sprite = vivling.current_sprite(state, Instant::now());
    assert_eq!(sprite, species_for_id(&state.species).ascii_baby);
}

#[test]
fn render_keeps_vivling_line_shape() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.configure_runtime(FrameRequester::test_dummy(), true);
    vivling.set_task_running(true);

    let area = Rect::new(0, 0, 80, 3);
    let mut buf = Buffer::empty(area);
    vivling.render(area, &mut buf);
    let rendered = buf
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("("));
    assert!(rendered.contains(")") || rendered.contains(">"));
    assert!(!rendered.contains("L01"));
    assert!(!rendered.contains("EN"));
    assert!(!rendered.contains("watching"));
    assert!(!rendered.contains("focus"));
    assert_eq!(vivling.desired_height(80), 3);
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
fn spawn_requires_level_30_and_persists_new_roster_member() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = configured_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::Hatch, temp.path())
        .expect("hatch");
    let err = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect_err("spawn should be gated");
    assert!(err.contains("level 30"));

    let mut state = vivling.state.clone().expect("hatched state");
    state.level = 30;
    state.active_work_days = 30;
    state.work_xp = WORK_XP_PER_LEVEL * 29;
    state.xp = state.work_xp;
    state.recompute_level();
    vivling.active_vivling_id = Some(state.vivling_id.clone());
    vivling.state = Some(state.clone());
    vivling.save_state().expect("save primary");

    let message = match vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect("spawn should work")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(message.contains("Spawned"));

    let roster = vivling.load_roster().expect("load roster");
    assert_eq!(roster.vivling_ids.len(), 2);
    let spawned_id = roster
        .vivling_ids
        .iter()
        .find(|entry| *entry != &state.vivling_id)
        .expect("spawned id");
    let spawned = vivling
        .load_state_for_id(spawned_id)
        .expect("load spawned")
        .expect("spawned state");
    assert_eq!(spawned.primary_vivling_id, state.primary_vivling_id);
    assert!(!spawned.is_primary);
    assert_eq!(spawned.lineage_role_label(), "spawned");
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
fn model_list_reads_profiles_from_config() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(
        temp.path().join(CONFIG_TOML_FILE),
        r#"
[profiles.vivling-spark]
model = "gpt-5.3-codex-spark"
model_provider = "openai"
model_reasoning_effort = "high"

[profiles.local-ollama]
model = "glm-5.1:cloud"
model_provider = "lm-studio"
"#,
    )
    .expect("write config");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling.assign_brain_profile("vivling-spark".to_string());

    let message = match vivling
        .command(VivlingAction::ModelList, temp.path())
        .expect("model list")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("* vivling-spark -> model gpt-5.3-codex-spark"));
    assert!(message.contains("- local-ollama -> model glm-5.1:cloud"));
}

#[test]
fn model_list_reads_global_provider_models_and_catalog() {
    let temp = TempDir::new().expect("tempdir");
    let catalog_path = temp.path().join("models_cache.json");
    fs::write(
        &catalog_path,
        r#"
{
  "models": [
    {
      "slug": "gpt-5.5",
      "display_name": "gpt-5.5",
      "default_reasoning_level": "medium",
      "visibility": "list"
    },
    {
      "slug": "hidden-model",
      "display_name": "hidden",
      "default_reasoning_level": "low",
      "visibility": "hidden"
    }
  ]
}
"#,
    )
    .expect("write catalog");
    fs::write(
        temp.path().join(CONFIG_TOML_FILE),
        format!(
            r#"
model = "gpt-5.5"
model_provider = "openai"
model_reasoning_effort = "medium"
model_catalog_json = "{}"

[model_providers.ollama_cloud]
name = "Ollama Cloud"
base_url = "http://localhost:11434/v1"
models = ["deepseek-v4-pro:cloud", "glm-5.1:cloud"]
"#,
            catalog_path.display()
        ),
    )
    .expect("write config");
    let mut vivling = hatched_vivling(temp.path());

    let message = match vivling
        .command(VivlingAction::ModelList, temp.path())
        .expect("model list")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("Current config model:"), "{message}");
    assert!(
        message.contains("- model gpt-5.5 · provider openai · effort medium"),
        "{message}"
    );
    assert!(message.contains("Configured provider models:"), "{message}");
    assert!(
        message.contains("/vivling model deepseek-v4-pro:cloud ollama_cloud"),
        "{message}"
    );
    assert!(
        message.contains("/vivling model glm-5.1:cloud ollama_cloud"),
        "{message}"
    );
    assert!(
        message.contains("Configured OpenAI catalog models:"),
        "{message}"
    );
    assert!(
        message.contains("/vivling model gpt-5.5 openai medium"),
        "{message}"
    );
    assert!(!message.contains("hidden-model"), "{message}");
}

#[test]
fn model_show_without_profile_lists_configured_profiles() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(
        temp.path().join(CONFIG_TOML_FILE),
        r#"
[profiles.vivling-spark]
model = "gpt-5.3-codex-spark"
model_provider = "openai"
model_reasoning_effort = "high"

[profiles.local-ollama]
model = "glm-5.1:cloud"
model_provider = "lm-studio"
"#,
    )
    .expect("write config");
    let mut vivling = hatched_vivling(temp.path());

    let message = match vivling
        .command(VivlingAction::ModelShow, temp.path())
        .expect("model show")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("profile none"));
    assert!(message.contains("No Vivling brain profile is selected."));
    assert!(message.contains("Select one with `/vivling model <profile>`."));
    assert!(message.contains("- vivling-spark -> model gpt-5.3-codex-spark"));
    assert!(message.contains("- local-ollama -> model glm-5.1:cloud"));
}

#[test]
fn model_show_without_profile_explains_creation_when_no_profiles_exist() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let message = match vivling
        .command(VivlingAction::ModelShow, temp.path())
        .expect("model show")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("No Vivling brain profile is selected."));
    assert!(message.contains("Create one with `/vivling model <model> [provider] [effort]`."));
    assert!(message.contains("No models are configured"));
}

#[test]
fn brain_on_without_profile_lists_configured_profiles() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(
        temp.path().join(CONFIG_TOML_FILE),
        r#"
[profiles.vivling-spark]
model = "gpt-5.3-codex-spark"
model_provider = "openai"
model_reasoning_effort = "high"

[profiles.local-ollama]
model = "glm-5.1:cloud"
model_provider = "lm-studio"
"#,
    )
    .expect("write config");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");

    let message = match vivling
        .command(VivlingAction::Brain(true), temp.path())
        .expect("brain on guidance")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("Select a Vivling brain profile before enabling the brain."));
    assert!(message.contains("Use `/vivling model <profile>`"));
    assert!(message.contains("- vivling-spark -> model gpt-5.3-codex-spark"));
    assert!(message.contains("- local-ollama -> model glm-5.1:cloud"));
    assert!(!vivling.state.as_ref().expect("state").brain_enabled);
}

#[test]
fn brain_on_without_profile_explains_creation_when_no_profiles_exist() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");

    let message = match vivling
        .command(VivlingAction::Brain(true), temp.path())
        .expect("brain on guidance")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("Select a Vivling brain profile before enabling the brain."));
    assert!(message.contains("Create one with `/vivling model <model> [provider] [effort]`."));
    assert!(message.contains("No models are configured"));
}

#[test]
fn assigning_brain_profile_after_adult_promotion_auto_enables_brain() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");

    let message = vivling
        .assign_brain_profile("vivling-spark".to_string())
        .expect("assign profile");
    assert!(message.contains("brain enabled"));

    let summary = match vivling
        .command(VivlingAction::ModelShow, temp.path())
        .expect("model show")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(summary.contains("brain on"));
    assert!(summary.contains("vivling-spark"));
}

#[test]
fn adult_direct_chat_is_role_focused_instead_of_generic() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");

    let message = match vivling
        .command(
            VivlingAction::DirectMessage("Dimmi cosa faresti".to_string()),
            temp.path(),
        )
        .expect("chat")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("As reviewer"));
    assert!(!message.contains("I remember this pattern. Lately:"));
}

#[test]
fn chat_falls_back_to_direct_reply_without_ready_brain() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let message = match vivling
        .command(VivlingAction::Chat("ciao bello".to_string()), temp.path())
        .expect("chat")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.contains("I'm ") || message.contains("As "));
}

#[test]
fn adult_chat_with_ready_brain_dispatches_chat_request() {
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
        .expect("chat")
    {
        VivlingCommandOutcome::DispatchAssist(request) => request,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert_eq!(request.kind, VivlingBrainRequestKind::Chat);
    assert_eq!(request.task, "ciao bello");
    assert_eq!(request.brain_profile, "vivling-spark");
    assert!(request.prompt_context.contains("User message:\nciao bello"));
    assert!(request.prompt_context.contains("Live state contract:"));
}

#[test]
fn assist_request_keeps_assist_kind() {
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
        .expect("assist")
    {
        VivlingCommandOutcome::DispatchAssist(request) => request,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert_eq!(request.kind, VivlingBrainRequestKind::Assist);
    assert_eq!(request.task, "review this blocker");
}

#[test]
fn assist_prompt_context_declares_memory_and_live_state_boundary() {
    use super::brain_context::BrainPromptKind;
    use super::brain_context::compose_brain_prompt;

    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .assign_brain_profile("vivling-spark".to_string())
        .expect("assign profile");

    let prompt = compose_brain_prompt(
        vivling.state.as_ref().expect("state"),
        BrainPromptKind::Assist,
        "review this blocker",
        None,
    )
    .expect("prompt");

    assert!(prompt.contains("Learned memory:"));
    assert!(prompt.contains("Live state contract:"));
    assert!(prompt.contains("Live state is unknown unless the task explicitly provides it."));
    assert!(prompt.contains("Task:\nreview this blocker"));
    // The structured prompt now also exposes a dedicated live-state section
    // distinct from the historical memory digest, so the brain stops
    // confusing past loop counters with current state.
    assert!(prompt.contains("Live state (now):"));
    assert!(prompt.contains("Recent observed work:"));
}

#[test]
fn export_and_import_roundtrip_uses_external_slots_without_auto_focus() {
    let source = TempDir::new().expect("source tempdir");
    let target = TempDir::new().expect("target tempdir");

    let mut source_vivling = configured_vivling(source.path());
    let _ = source_vivling
        .command(VivlingAction::Hatch, source.path())
        .expect("hatch");
    let source_state = leveled_state(30, 30);
    source_vivling.active_vivling_id = Some(source_state.vivling_id.clone());
    source_vivling.state = Some(source_state.clone());
    source_vivling.save_state().expect("save source");

    let export_path = source.path().join("demo.vivegg");
    let export_message = match source_vivling
        .command(
            VivlingAction::Export(Some(export_path.display().to_string())),
            source.path(),
        )
        .expect("export")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(export_message.contains("Exported"));
    assert!(export_path.exists());

    let mut target_vivling = configured_vivling(target.path());
    let _ = target_vivling
        .command(VivlingAction::Hatch, target.path())
        .expect("target hatch");
    let active_before = target_vivling.active_vivling_id.clone();

    let import_message = match target_vivling
        .command(
            VivlingAction::Import(export_path.display().to_string()),
            target.path(),
        )
        .expect("import")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(import_message.contains("Imported"));
    assert_eq!(target_vivling.active_vivling_id, active_before);
    let roster = target_vivling.load_roster().expect("target roster");
    assert_eq!(roster.external_vivling_ids.len(), 1);
}

#[test]
fn remove_blocks_active_vivling() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = configured_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::Hatch, temp.path())
        .expect("hatch");
    let active_id = vivling.active_vivling_id.clone().expect("active id");
    let err = vivling
        .command(VivlingAction::Remove(active_id), temp.path())
        .expect_err("active remove should fail");
    assert!(err.contains("Focus another one first"));
}

#[test]
fn spawn_slot_progression_enforces_level_30_60_90_thresholds() {
    for (level, expected_capacity) in [(29, 0usize), (30, 1), (60, 2), (90, 3)] {
        let temp = TempDir::new().expect("tempdir");
        let mut vivling = hatched_vivling(temp.path());
        let primary = set_active_level(&mut vivling, level);

        for spawn_index in 0..expected_capacity {
            let message = match vivling
                .command(VivlingAction::Spawn, temp.path())
                .expect("spawn attempt")
            {
                VivlingCommandOutcome::Message(message) => message,
                other => panic!("unexpected outcome: {other:?}"),
            };
            assert!(message.contains("Local spawn slots now"));
            assert!(message.contains(&(spawn_index + 1).to_string()));
        }

        let roster = vivling.load_roster().expect("roster");
        assert_eq!(roster.vivling_ids.len(), expected_capacity + 1);

        let next_spawn = vivling.command(VivlingAction::Spawn, temp.path());
        if expected_capacity == 0 {
            let err = next_spawn.expect_err("spawn should be gated");
            assert!(err.contains("level 30"));
        } else {
            let err = next_spawn.expect_err("quota should block extra spawn");
            assert!(err.contains("No free local spawn slots"));
        }

        let lineage_states = vivling
            .load_lineage_states(&primary.primary_vivling_id)
            .expect("lineage");
        let spawned_count = lineage_states
            .iter()
            .filter(|entry| !entry.is_primary && !entry.is_imported)
            .count();
        assert_eq!(spawned_count, expected_capacity);
    }
}

#[test]
fn spawn_rejects_non_primary_vivling() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let primary = set_active_level(&mut vivling, 30);
    let spawned_id = {
        let _ = vivling
            .command(VivlingAction::Spawn, temp.path())
            .expect("spawn");
        spawn_ids(&vivling, &primary.vivling_id)
            .into_iter()
            .next()
            .expect("spawn id")
    };

    let _ = vivling
        .command(VivlingAction::Focus(spawned_id), temp.path())
        .expect("focus spawn");
    let err = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect_err("spawned vivling cannot spawn");
    assert!(err.contains("Only a primary Vivling"));
}

#[test]
fn roster_focus_and_reload_preserve_active_member_and_alias_resolution() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let primary = set_active_level(&mut vivling, 30);
    let _ = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect("spawn");

    let roster_text = match vivling
        .command(VivlingAction::Roster, temp.path())
        .expect("roster")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(roster_text.contains("Vivling roster"));
    assert!(roster_text.contains("top-level slots 1/3"));
    assert!(roster_text.contains("[primary]"));

    let _ = vivling
        .command(VivlingAction::Focus("spawn-1".to_string()), temp.path())
        .expect("focus by alias");
    let spawned_id = spawn_ids(&vivling, &primary.vivling_id)
        .into_iter()
        .next()
        .expect("spawned id");
    assert_eq!(
        vivling.active_vivling_id.as_deref(),
        Some(spawned_id.as_str())
    );
    let focused = vivling.state.as_ref().expect("focused state");
    assert_eq!(
        focused.last_work_summary.as_deref(),
        Some(format!("{} active", focused.name).as_str())
    );

    let status = match vivling
        .command(VivlingAction::Status, temp.path())
        .expect("status")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(status.contains("spawned"));
    assert!(status.contains("local spawn slots"));

    let reloaded = configured_vivling(temp.path());
    assert_eq!(
        reloaded.active_vivling_id.as_deref(),
        Some(spawned_id.as_str())
    );
    assert_eq!(
        reloaded
            .state
            .as_ref()
            .map(|state| state.lineage_role_label()),
        Some("spawned")
    );
}

#[test]
fn remove_spawned_vivling_frees_local_spawn_capacity() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let primary = set_active_level(&mut vivling, 30);
    let _ = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect("spawn");
    let spawned_id = spawn_ids(&vivling, &primary.vivling_id)
        .into_iter()
        .next()
        .expect("spawned id");

    let removed = match vivling
        .command(VivlingAction::Remove(spawned_id.clone()), temp.path())
        .expect("remove spawned")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(removed.contains("Removed"));

    let respawned = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect("spawn after removal");
    match respawned {
        VivlingCommandOutcome::Message(message) => assert!(message.contains("Spawned")),
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn remove_imported_vivling_frees_external_slot() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");

    let mut exporter = hatched_vivling(source.path());
    let state = set_active_level(&mut exporter, 30);
    let export_one = source.path().join("one.vivegg");
    let export_two = source.path().join("two.vivegg");
    let _ = exporter
        .command(
            VivlingAction::Export(Some(export_one.display().to_string())),
            source.path(),
        )
        .expect("export one");

    let second_state = VivlingState {
        vivling_id: "viv-import-two".to_string(),
        name: "Import Two".to_string(),
        ..state.clone()
    };
    let second_manifest = VivlingPackageManifest {
        package_version: VIVPKG_VERSION,
        exported_at: Utc::now(),
        vivling_id: second_state.vivling_id.clone(),
        primary_vivling_id: second_state.primary_vivling_id.clone(),
        species: second_state.species.clone(),
        rarity: second_state.rarity.clone(),
        level: second_state.level,
        is_primary: true,
        is_imported: false,
        spawn_generation: 0,
    };
    make_package(&export_two, &second_manifest, &second_state);

    let mut target_vivling = hatched_vivling(target.path());
    let imported_id = {
        let _ = target_vivling
            .command(
                VivlingAction::Import(export_one.display().to_string()),
                target.path(),
            )
            .expect("import one");
        target_vivling
            .load_roster()
            .expect("roster")
            .external_vivling_ids
            .into_iter()
            .next()
            .expect("imported id")
    };

    let _ = target_vivling
        .command(VivlingAction::Remove(imported_id), target.path())
        .expect("remove imported");
    let _ = target_vivling
        .command(
            VivlingAction::Import(export_two.display().to_string()),
            target.path(),
        )
        .expect("import after free slot");
    assert_eq!(
        target_vivling
            .load_roster()
            .expect("roster")
            .external_vivling_ids
            .len(),
        1
    );
}

#[test]
fn remove_rejects_primary_with_spawned_children() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let primary = set_active_level(&mut vivling, 30);
    let _ = vivling
        .command(VivlingAction::Spawn, temp.path())
        .expect("spawn");
    let spawned_id = spawn_ids(&vivling, &primary.vivling_id)
        .into_iter()
        .next()
        .expect("spawned id");
    let _ = vivling
        .command(VivlingAction::Focus(spawned_id), temp.path())
        .expect("focus spawn");
    let err = vivling
        .command(VivlingAction::Remove(primary.vivling_id), temp.path())
        .expect_err("primary remove should fail");
    assert!(err.contains("spawned lineage children"));
}

#[test]
fn export_supports_default_and_relative_custom_paths() {
    let temp = TempDir::new().expect("tempdir");
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).expect("cwd dir");
    let mut vivling = hatched_vivling(temp.path());
    let state = set_active_level(&mut vivling, 30);

    let default_message = match vivling
        .command(VivlingAction::Export(None), &cwd)
        .expect("default export")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    let default_path = temp
        .path()
        .join("vivlings")
        .join("exports")
        .join(format!("{}.vivegg", state.vivling_id));
    assert!(default_message.contains(default_path.to_string_lossy().as_ref()));
    assert!(default_path.exists());

    let relative = match vivling
        .command(
            VivlingAction::Export(Some("nested/demo-export".to_string())),
            &cwd,
        )
        .expect("relative export")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    let relative_path = cwd.join("nested").join("demo-export.vivegg");
    assert!(relative.contains(relative_path.to_string_lossy().as_ref()));
    assert!(relative_path.exists());
}

#[test]
fn export_package_contains_manifest_and_state_and_updates_export_count() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let state = set_active_level(&mut vivling, 30);
    let export_path = temp.path().join("inspect.vivegg");

    let _ = vivling
        .command(
            VivlingAction::Export(Some(export_path.display().to_string())),
            temp.path(),
        )
        .expect("export");

    let file = File::open(&export_path).expect("open vivegg");
    let mut zip = ZipArchive::new(file).expect("zip archive");
    let manifest: VivlingPackageManifest =
        read_zip_json(&mut zip, "manifest.json").expect("manifest");
    let exported_state: VivlingState = read_zip_json(&mut zip, "state.json").expect("state");
    assert_eq!(manifest.vivling_id, state.vivling_id);
    assert_eq!(manifest.primary_vivling_id, state.primary_vivling_id);
    assert_eq!(exported_state.vivling_id, state.vivling_id);
    assert_eq!(exported_state.primary_vivling_id, state.primary_vivling_id);
    assert_eq!(
        vivling.state.as_ref().map(|entry| entry.export_count),
        Some(1)
    );
}

#[test]
fn import_rejects_non_vivegg_files() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let invalid = temp.path().join("bad.txt");
    fs::write(&invalid, "not a package").expect("write invalid");
    let err = vivling
        .command(
            VivlingAction::Import(invalid.display().to_string()),
            temp.path(),
        )
        .expect_err("should reject extension");
    assert!(err.contains(".vivegg"));
}

#[test]
fn import_rejects_malformed_zip_file() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let invalid = temp.path().join("broken.vivegg");
    fs::write(&invalid, "definitely not zip").expect("write invalid");
    let err = vivling
        .command(
            VivlingAction::Import(invalid.display().to_string()),
            temp.path(),
        )
        .expect_err("should reject malformed zip");
    assert!(!err.is_empty());
}

#[test]
fn import_rejects_missing_manifest_or_state_entries() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    let missing_manifest = temp.path().join("missing-manifest.vivegg");
    {
        let file = File::create(&missing_manifest).expect("create package");
        let mut zip = ZipWriter::new(file);
        zip.start_file("state.json", options).expect("state entry");
        zip.write_all(b"{}").expect("state body");
        zip.finish().expect("finish");
    }
    let err = vivling
        .command(
            VivlingAction::Import(missing_manifest.display().to_string()),
            temp.path(),
        )
        .expect_err("missing manifest");
    assert!(err.contains("specified file not found") || err.contains("manifest"));

    let missing_state = temp.path().join("missing-state.vivegg");
    {
        let file = File::create(&missing_state).expect("create package");
        let mut zip = ZipWriter::new(file);
        zip.start_file("manifest.json", options)
            .expect("manifest entry");
        zip.write_all(b"{}").expect("manifest body");
        zip.finish().expect("finish");
    }
    let err = vivling
        .command(
            VivlingAction::Import(missing_state.display().to_string()),
            temp.path(),
        )
        .expect_err("missing state");
    assert!(!err.is_empty());
}

#[test]
fn import_rejects_manifest_state_id_mismatch() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let path = temp.path().join("mismatch.vivegg");
    let state = exportable_state(30);
    let manifest = VivlingPackageManifest {
        package_version: VIVPKG_VERSION,
        exported_at: Utc::now(),
        vivling_id: "other-id".to_string(),
        primary_vivling_id: state.primary_vivling_id.clone(),
        species: state.species.clone(),
        rarity: state.rarity.clone(),
        level: state.level,
        is_primary: true,
        is_imported: false,
        spawn_generation: 0,
    };
    make_package(&path, &manifest, &state);

    let err = vivling
        .command(
            VivlingAction::Import(path.display().to_string()),
            temp.path(),
        )
        .expect_err("mismatch should fail");
    assert!(err.contains("manifest/state id mismatch"));
}

#[test]
fn import_rejects_duplicate_ids_and_full_external_slots() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());

    let make_distinct_pkg = |idx: usize| {
        let path = temp.path().join(format!("import-{idx}.vivegg"));
        let mut state = exportable_state(30);
        state.vivling_id = format!("viv-import-{idx}");
        state.name = format!("Import {idx}");
        state.primary_vivling_id = String::new();
        state.origin_install_id = None;
        state.is_primary = false;
        let manifest = VivlingPackageManifest {
            package_version: VIVPKG_VERSION,
            exported_at: Utc::now(),
            vivling_id: state.vivling_id.clone(),
            primary_vivling_id: state.vivling_id.clone(),
            species: state.species.clone(),
            rarity: state.rarity.clone(),
            level: state.level,
            is_primary: true,
            is_imported: false,
            spawn_generation: 0,
        };
        make_package(&path, &manifest, &state);
        path
    };

    let duplicate_path = make_distinct_pkg(1);
    let _ = vivling
        .command(
            VivlingAction::Import(duplicate_path.display().to_string()),
            temp.path(),
        )
        .expect("first import");
    let err = vivling
        .command(
            VivlingAction::Import(duplicate_path.display().to_string()),
            temp.path(),
        )
        .expect_err("duplicate should fail");
    assert!(err.contains("already exists"));

    let path = make_distinct_pkg(2);
    let _ = vivling
        .command(
            VivlingAction::Import(path.display().to_string()),
            temp.path(),
        )
        .expect("fill top-level slot");
    let fourth_path = make_distinct_pkg(3);
    let err = vivling
        .command(
            VivlingAction::Import(fourth_path.display().to_string()),
            temp.path(),
        )
        .expect_err("next import should fail");
    assert!(err.contains("All top-level Vivling slots are full"));

    let imported_id = vivling
        .load_roster()
        .expect("roster")
        .external_vivling_ids
        .first()
        .cloned()
        .expect("imported id");
    let imported = vivling
        .load_state_for_id(&imported_id)
        .expect("load imported")
        .expect("imported state");
    assert_eq!(imported.primary_vivling_id, imported.vivling_id);
    assert!(imported.is_primary);
    assert!(imported.is_imported);
}

#[test]
fn imported_primary_can_use_local_spawn_capacity() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    let mut exporter = hatched_vivling(source.path());
    let state = set_active_level(&mut exporter, 60);
    let import_path = source.path().join("primary-import.vivegg");
    let _ = exporter
        .command(
            VivlingAction::Export(Some(import_path.display().to_string())),
            source.path(),
        )
        .expect("export source primary");

    let mut vivling = hatched_vivling(target.path());
    let _ = vivling
        .command(
            VivlingAction::Import(import_path.display().to_string()),
            target.path(),
        )
        .expect("import");
    let imported_id = vivling
        .load_roster()
        .expect("roster")
        .external_vivling_ids
        .into_iter()
        .next()
        .expect("imported id");
    let _ = vivling
        .command(VivlingAction::Focus(imported_id), target.path())
        .expect("focus imported");
    assert_eq!(
        vivling.state.as_ref().map(|entry| entry.level),
        Some(state.level)
    );

    let _ = vivling
        .command(VivlingAction::Spawn, target.path())
        .expect("spawn one");
    let _ = vivling
        .command(VivlingAction::Spawn, target.path())
        .expect("spawn two");
    let err = vivling
        .command(VivlingAction::Spawn, target.path())
        .expect_err("third spawn should fail at level 60");
    assert!(err.contains("No free local spawn slots"));
}

#[test]
fn legacy_single_state_migrates_into_roster_on_configure() {
    let temp = TempDir::new().expect("tempdir");
    let legacy_path = temp.path().join("vivling.json");
    let mut legacy_state = exportable_state(30);
    legacy_state.primary_vivling_id = String::new();
    legacy_state.origin_install_id = None;
    legacy_state.is_primary = false;
    fs::write(
        &legacy_path,
        serde_json::to_string_pretty(&legacy_state).expect("legacy json"),
    )
    .expect("write legacy state");

    let vivling = configured_vivling(temp.path());
    assert!(!legacy_path.exists());
    assert_eq!(
        vivling.active_vivling_id.as_deref(),
        Some(legacy_state.vivling_id.as_str())
    );
    let roster = vivling.load_roster().expect("roster");
    assert_eq!(roster.vivling_ids, vec![legacy_state.vivling_id.clone()]);
    let migrated = vivling
        .load_state_for_id(&legacy_state.vivling_id)
        .expect("load migrated")
        .expect("migrated state");
    assert_eq!(migrated.primary_vivling_id, migrated.vivling_id);
    assert!(migrated.is_primary);
}

#[test]
fn legacy_single_state_with_suggest_ai_mode_migrates_into_roster() {
    let temp = TempDir::new().expect("tempdir");
    let legacy_path = temp.path().join("vivling.json");
    let mut legacy_state = exportable_state(30);
    legacy_state.primary_vivling_id = String::new();
    legacy_state.origin_install_id = None;
    legacy_state.is_primary = false;
    let mut raw = serde_json::to_value(&legacy_state).expect("serialize legacy state for rewrite");
    raw["ai_mode"] = serde_json::Value::String("suggest".to_string());
    fs::write(
        &legacy_path,
        serde_json::to_string_pretty(&raw).expect("legacy json"),
    )
    .expect("write legacy state");

    let vivling = configured_vivling(temp.path());
    assert!(!legacy_path.exists());
    let roster = vivling.load_roster().expect("roster");
    assert_eq!(roster.vivling_ids, vec![legacy_state.vivling_id.clone()]);
    let migrated = vivling
        .load_state_for_id(&legacy_state.vivling_id)
        .expect("load migrated")
        .expect("migrated state");
    assert_eq!(migrated.ai_mode, VivlingAiMode::On);
    assert_eq!(migrated.primary_vivling_id, migrated.vivling_id);
    assert!(migrated.is_primary);
}

#[test]
fn legacy_single_state_with_sparse_memory_entries_migrates_into_roster() {
    let temp = TempDir::new().expect("tempdir");
    let legacy_path = temp.path().join("vivling.json");
    let mut legacy_state = exportable_state(30);
    legacy_state.primary_vivling_id = String::new();
    legacy_state.origin_install_id = None;
    legacy_state.is_primary = false;
    let mut raw = serde_json::to_value(&legacy_state).expect("serialize legacy state");
    raw["work_memory"] = serde_json::json!([
        {
            "kind": "turn",
            "summary": "reviewed docs smoke"
        },
        {
            "summary": ""
        }
    ]);
    raw["distilled_summaries"] = serde_json::json!([
        {
            "topic": "verify"
        }
    ]);
    raw["mental_paths"] = serde_json::json!([
        {
            "from": "kind:turn"
        }
    ]);
    fs::write(
        &legacy_path,
        serde_json::to_string_pretty(&raw).expect("legacy json"),
    )
    .expect("write legacy state");

    let vivling = configured_vivling(temp.path());
    assert!(!legacy_path.exists());
    let roster = vivling.load_roster().expect("roster");
    assert_eq!(roster.vivling_ids, vec![legacy_state.vivling_id.clone()]);
    let migrated = vivling
        .load_state_for_id(&legacy_state.vivling_id)
        .expect("load migrated")
        .expect("migrated state");
    assert_eq!(migrated.primary_vivling_id, migrated.vivling_id);
    assert!(migrated.is_primary);
    assert_eq!(migrated.work_memory.len(), 2);
    assert!(
        migrated
            .work_memory
            .iter()
            .all(|entry| !entry.summary.trim().is_empty())
    );
    assert!(migrated.work_xp > 0);
    assert!(migrated.level >= 1);
}
