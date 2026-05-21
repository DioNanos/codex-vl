use super::common::*;

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
fn brain_on_without_profile_enables_with_inheritance_note_and_profile_list() {
    // V2 §8.1 (P0.2): `/vivling brain on` without a pinned profile now
    // SUCCEEDS and surfaces a note that the brain inherits the
    // session model. We still surface the configured profiles so the
    // user can pin one with `/vivling model <profile>` if they want.
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

    assert!(vivling.state.as_ref().expect("state").brain_enabled);
    assert!(
        message.contains("inherit") || message.contains("session"),
        "inheritance hint missing: {message}"
    );
    assert!(message.contains("/vivling model <profile>"));
    assert!(message.contains("- vivling-spark -> model gpt-5.3-codex-spark"));
    assert!(message.contains("- local-ollama -> model glm-5.1:cloud"));
}

#[test]
fn brain_on_without_profile_enables_even_when_no_profiles_exist() {
    // V2 §8.1 (P0.2): missing-profile path must reach SessionDefault
    // even on a fresh codex_home with zero profiles configured. The
    // V1 hard block ("Create one with `/vivling model …`") would
    // have made the inheritance path unreachable here.
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

    assert!(vivling.state.as_ref().expect("state").brain_enabled);
    assert!(
        message.contains("inherit") || message.contains("session"),
        "inheritance hint missing: {message}"
    );
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

    assert!(message.starts_with("Local fallback: "), "{message}");
    assert!(message.contains("I'm ") || message.contains("As "));
}

#[test]
fn adult_chat_with_brain_disabled_is_explicit_local_fallback() {
    // V2 §8.1 (P0.2): the only blockers for brain dispatch are
    // adulthood and `brain_enabled`. A missing brain_profile is no
    // longer a blocker — it just routes to BrainTarget::SessionDefault.
    // Brain disabled still falls back to the local templated reply.
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    // brain stays disabled (default for a freshly hatched Vivling).

    let message = match vivling
        .command(
            VivlingAction::Chat("what should we check?".to_string()),
            temp.path(),
        )
        .expect("chat")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert!(message.starts_with("Local fallback: "), "{message}");
    assert!(!message.contains("brain is thinking"), "{message}");
}

#[test]
fn adult_chat_with_brain_enabled_and_no_profile_dispatches_session_default() {
    // V2 §8.1 (P0.2) inheritance reachability check: the user enabled
    // the brain but never pinned a profile. The dispatcher must build
    // a real chat request with `BrainTarget::SessionDefault`, not fall
    // back to the local templated reply.
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .set_brain_enabled_with_guidance(true)
        .expect("enable brain without profile");
    assert!(vivling.state.as_ref().expect("state").brain_enabled);
    assert!(
        vivling
            .state
            .as_ref()
            .expect("state")
            .brain_profile
            .is_none()
    );

    let request = match vivling
        .command(VivlingAction::Chat("ciao".to_string()), temp.path())
        .expect("chat")
    {
        VivlingCommandOutcome::DispatchAssist(req) => req,
        other => panic!("expected DispatchAssist, got: {other:?}"),
    };
    assert_eq!(request.kind, VivlingBrainRequestKind::Chat);
    assert_eq!(request.task, "ciao");
    assert_eq!(
        request.brain_target,
        crate::vivling::BrainTarget::SessionDefault,
        "no pinned profile must inherit the session model"
    );
}

#[test]
fn brain_enable_on_adult_without_profile_succeeds_with_inheritance_note() {
    // V2 §8.1 (P0.2): `/vivling brain on` must succeed even when no
    // profile is pinned, leaving the dispatcher free to use
    // SessionDefault. The previous "Select a brain profile first"
    // hard block made the inheritance path unreachable.
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");

    let message = vivling
        .set_brain_enabled_with_guidance(true)
        .expect("brain enable must succeed without a pinned profile");

    assert!(vivling.state.as_ref().expect("state").brain_enabled);
    assert!(
        message.contains("session"),
        "user must be told the brain inherits the session model; got: {message}"
    );
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
    // Memory V2 §8.1 (P0.2): the request now carries a BrainTarget
    // enum instead of the raw profile string. The Vivling has the
    // "vivling-spark" profile pinned, so the dispatcher should see
    // `Profile("vivling-spark")` and not fall back to SessionDefault.
    assert_eq!(
        request.brain_target,
        crate::vivling::BrainTarget::Profile("vivling-spark".to_string())
    );
    assert!(request.prompt_context.contains("User message:\nciao bello"));
    assert!(request.prompt_context.contains("Live state contract:"));
}

#[test]
fn brain_runtime_error_persists_actionable_text() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let err = "Vivling brain request failed before a reply: auth missing. Check auth, provider, model, or disable the brain with `/vivling brain off`.";

    vivling
        .mark_brain_runtime_error(err)
        .expect("mark brain error");

    let state = vivling.state.as_ref().expect("state");
    let stored = state.brain_last_error.as_deref().expect("brain error");
    assert!(stored.contains("Check auth, provider, model"), "{stored}");
    assert!(stored.contains("/vivling brain off"), "{stored}");
    assert_eq!(state.last_message.as_deref(), Some(stored));
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
    use super::super::brain_context::BrainPromptKind;
    use super::super::brain_context::compose_brain_prompt;

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

/// Smoke test for the production hot path that broke in 0.130.0:
/// a `Vivling::record_turn_completed` call must indicize the new capsule into
/// the per-Vivling MSA collection on disk. The earlier `relevant_memory_*`
/// test bypasses the wrapper and calls `MsaIndex` directly, so it cannot
/// catch a regression where `self.msa` is `None` or the closure-snapshot of
/// `msa`/`vivling_id` becomes stale.
///
/// We override `vivling.msa` to point at an isolated tempdir so the test
/// does not pollute the user's `~/.local/state/mcp-msa-rs/`.
#[test]
fn record_turn_completed_indexes_into_msa() {
    use std::sync::Arc;

    let codex_home = TempDir::new().expect("codex_home tempdir");
    let msa_storage = TempDir::new().expect("msa storage tempdir");

    let mut vivling = configured_vivling(codex_home.path());
    // Replace the default-storage MSA with an isolated one so the test does
    // not depend on (or write to) the user's HOME.
    vivling.msa = Some(Arc::new(VivlingMsa::open_for_tests(msa_storage.path())));

    let _ = vivling
        .command(VivlingAction::Hatch, codex_home.path())
        .expect("hatch vivling");
    let vivling_id = vivling
        .state
        .as_ref()
        .map(|s| s.vivling_id.clone())
        .expect("hatched state");

    let before = vivling
        .state
        .as_ref()
        .map(|s| s.work_memory.len())
        .unwrap_or(0);
    vivling
        .record_turn_completed(Some("smoke turn for msa indexing"))
        .expect("record_turn_completed should succeed");
    let after = vivling
        .state
        .as_ref()
        .map(|s| s.work_memory.len())
        .unwrap_or(0);
    assert!(
        after > before,
        "work_memory should grow after record_turn_completed (before={before}, after={after})"
    );

    assert_msa_collection_has_tantivy_shard(msa_storage.path(), &vivling_id);
}

#[test]
fn record_turn_completed_indexes_into_msa_when_work_memory_saturated() {
    use crate::vivling::model::MAX_WORK_MEMORY_ENTRIES;
    use std::sync::Arc;

    let codex_home = TempDir::new().expect("codex_home tempdir");
    let msa_storage = TempDir::new().expect("msa storage tempdir");

    let mut vivling = configured_vivling(codex_home.path());
    vivling.msa = Some(Arc::new(VivlingMsa::open_for_tests(msa_storage.path())));

    let _ = vivling
        .command(VivlingAction::Hatch, codex_home.path())
        .expect("hatch vivling");
    let vivling_id = vivling
        .state
        .as_ref()
        .map(|s| s.vivling_id.clone())
        .expect("hatched state");
    vivling.state.as_mut().expect("state").work_memory = saturated_memory_entries();

    vivling
        .record_turn_completed(Some("saturated turn for msa indexing"))
        .expect("record_turn_completed should succeed");

    let state = vivling.state.as_ref().expect("state");
    assert_eq!(
        state.work_memory.len(),
        MAX_WORK_MEMORY_ENTRIES,
        "work_memory should remain capped after record_turn_completed"
    );
    assert!(
        state
            .work_memory
            .last()
            .expect("last memory")
            .summary
            .contains("saturated turn for msa indexing")
    );

    assert_msa_collection_has_tantivy_shard(msa_storage.path(), &vivling_id);
}

#[test]
fn record_loop_event_indexes_into_msa() {
    use std::sync::Arc;

    let codex_home = TempDir::new().expect("codex_home tempdir");
    let msa_storage = TempDir::new().expect("msa storage tempdir");

    let mut vivling = configured_vivling(codex_home.path());
    vivling.msa = Some(Arc::new(VivlingMsa::open_for_tests(msa_storage.path())));

    let _ = vivling
        .command(VivlingAction::Hatch, codex_home.path())
        .expect("hatch vivling");
    let vivling_id = vivling
        .state
        .as_ref()
        .map(|s| s.vivling_id.clone())
        .expect("hatched state");

    let before = vivling
        .state
        .as_ref()
        .map(|s| s.work_memory.len())
        .unwrap_or(0);
    vivling
        .record_loop_event(VivlingLoopEvent {
            kind: VivlingLoopEventKind::Runtime,
            source: VivlingLoopEventSource::Agent,
            action: "trigger".to_string(),
            label: "msa-smoke".to_string(),
            runtime_state: Some("running".to_string()),
            last_status: Some("submitted".to_string()),
            goal: Some("cover loop msa indexing".to_string()),
        })
        .expect("record_loop_event should succeed");
    let after = vivling
        .state
        .as_ref()
        .map(|s| s.work_memory.len())
        .unwrap_or(0);
    assert!(
        after > before,
        "work_memory should grow after record_loop_event (before={before}, after={after})"
    );

    assert_msa_collection_has_tantivy_shard(msa_storage.path(), &vivling_id);
}

#[test]
fn record_loop_event_indexes_into_msa_when_work_memory_saturated() {
    use crate::vivling::model::MAX_WORK_MEMORY_ENTRIES;
    use std::sync::Arc;

    let codex_home = TempDir::new().expect("codex_home tempdir");
    let msa_storage = TempDir::new().expect("msa storage tempdir");

    let mut vivling = configured_vivling(codex_home.path());
    vivling.msa = Some(Arc::new(VivlingMsa::open_for_tests(msa_storage.path())));

    let _ = vivling
        .command(VivlingAction::Hatch, codex_home.path())
        .expect("hatch vivling");
    let vivling_id = vivling
        .state
        .as_ref()
        .map(|s| s.vivling_id.clone())
        .expect("hatched state");
    vivling.state.as_mut().expect("state").work_memory = saturated_memory_entries();

    vivling
        .record_loop_event(VivlingLoopEvent {
            kind: VivlingLoopEventKind::Runtime,
            source: VivlingLoopEventSource::Agent,
            action: "trigger".to_string(),
            label: "msa-saturated".to_string(),
            runtime_state: Some("running".to_string()),
            last_status: Some("submitted".to_string()),
            goal: Some("cover saturated loop msa indexing".to_string()),
        })
        .expect("record_loop_event should succeed");

    let state = vivling.state.as_ref().expect("state");
    assert_eq!(
        state.work_memory.len(),
        MAX_WORK_MEMORY_ENTRIES,
        "work_memory should remain capped after record_loop_event"
    );
    assert!(
        state
            .work_memory
            .last()
            .expect("last memory")
            .summary
            .contains("msa-saturated")
    );

    assert_msa_collection_has_tantivy_shard(msa_storage.path(), &vivling_id);
}

fn saturated_memory_entries() -> Vec<crate::vivling::model::VivlingWorkMemoryEntry> {
    use crate::vivling::model::MAX_WORK_MEMORY_ENTRIES;
    use crate::vivling::model::VivlingWorkMemoryEntry;
    use crate::vivling::model::WorkArchetype;
    use chrono::Duration;

    (0..MAX_WORK_MEMORY_ENTRIES)
        .map(|index| VivlingWorkMemoryEntry {
            kind: "preexisting".to_string(),
            summary: format!("preexisting saturated memory {index}"),
            archetype: WorkArchetype::Builder,
            weight: 1,
            created_at: Utc::now() - Duration::seconds(index as i64),
        })
        .collect()
}

fn assert_msa_collection_has_tantivy_shard(msa_storage: &std::path::Path, vivling_id: &str) {
    let collection_dir = msa_storage.join(format!("vivling::{vivling_id}"));
    assert!(
        collection_dir.is_dir(),
        "MSA collection directory should exist at {}",
        collection_dir.display()
    );
    let entries: Vec<String> = std::fs::read_dir(&collection_dir)
        .expect("list collection dir")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let has_tantivy_shard = entries
        .iter()
        .any(|name| name.ends_with(".term") || name.ends_with(".store") || name.ends_with(".idx"));
    assert!(
        has_tantivy_shard,
        "expected tantivy shard files (.term/.store/.idx) in {}, got: {entries:?}",
        collection_dir.display()
    );
}

#[test]
fn status_loop_owner_ready_with_brain_enabled_and_no_profile() {
    // V2 §8.1 (P0.2): the status surface must report loop-owner
    // readiness based on adult + brain_enabled only, mirroring
    // `active_loop_owner_identity`. A pinned profile is no longer
    // a prerequisite — SessionDefault is a valid target.
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let _ = vivling
        .command(VivlingAction::PromoteAdult, temp.path())
        .expect("promote adult");
    let _ = vivling
        .set_brain_enabled_with_guidance(true)
        .expect("enable brain without profile");

    let status = vivling.status().expect("status");
    assert!(
        status.contains("loop owner ready"),
        "adult + brain_enabled with no profile must show loop owner ready; got: {status}"
    );

    let help = vivling.help_message();
    assert!(
        help.contains("loop-owner eligible: yes"),
        "help must mark loop-owner eligible: yes; got: {help}"
    );

    // active_loop_owner_identity must agree.
    let identity = vivling.active_loop_owner_identity();
    assert!(
        identity.is_ok(),
        "active_loop_owner_identity must accept brain_profile None; got: {identity:?}"
    );
}
