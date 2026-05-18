//! codex-vl: background brain pipeline for Vivling assist and loop tick.
//!
//! Contains the async functions invoked by `App::run_vivling_assist` and
//! `App::run_vivling_loop_tick` (in `app/loop_controller.rs`). The brain
//! talks to the configured model via `codex_core::client::ModelClient`
//! using the same plumbing the main session uses.
//!
//! Keeping these functions in their own module limits the surface of our
//! changes to the rest of the app dispatcher, so upstream edits to
//! `background_requests.rs` do not have to be merged around them.

use std::sync::Arc;

use codex_core::ModelClient;
use codex_core::Prompt;
use codex_core::ResponseEvent;
use codex_core::build_models_manager;
use codex_core::content_items_to_text;
use codex_features::Feature;
use codex_otel::SessionTelemetry;
use codex_protocol::ThreadId;
use codex_rollout_trace::InferenceTraceContext;
use tokio_stream::StreamExt;

use crate::legacy_core::config::Config;
use crate::legacy_core::config::ConfigBuilder;
use crate::legacy_core::config::ConfigOverrides;
use crate::vivling::VivlingAssistRequest;
use crate::vivling::VivlingBrainRequestKind;
use crate::vivling::VivlingLoopTickRequest;
use crate::vivling::VivlingLoopTickResult;

pub(super) async fn run_vivling_assist_request(
    config: Config,
    session_telemetry: SessionTelemetry,
    request: VivlingAssistRequest,
) -> Result<String, String> {
    let (profile_config, model_name) =
        resolve_vivling_brain_profile_config(&config, &request.brain_profile).await?;

    let auth_manager = Arc::new(
        codex_login::AuthManager::new(
            profile_config.codex_home.to_path_buf(),
            /*enable_codex_api_key_env*/ false,
            profile_config.cli_auth_credentials_store_mode,
            Some(profile_config.chatgpt_base_url.clone()),
        )
        .await,
    );
    let models_manager = build_models_manager(&profile_config, Arc::clone(&auth_manager));
    let model_info = models_manager
        .get_model_info(&model_name, &profile_config.to_models_manager_config())
        .await;

    let client = ModelClient::new(
        Some(auth_manager),
        codex_protocol::SessionId::new(),
        ThreadId::new(),
        request.vivling_id.clone(),
        profile_config.model_provider.clone(),
        codex_protocol::protocol::SessionSource::Custom("vivling".to_string()),
        profile_config.model_verbosity,
        profile_config
            .features
            .enabled(Feature::EnableRequestCompression),
        profile_config.features.enabled(Feature::RuntimeMetrics),
        None,
        None,
    );
    let mut prompt = Prompt::default();
    prompt.input = vec![codex_protocol::models::ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![codex_protocol::models::ContentItem::InputText {
            text: request.prompt_context,
        }],
        phase: None,
    }];
    let instruction_text = match &request.kind {
        VivlingBrainRequestKind::Assist => format!(
            "You are {} speaking as a Vivling inside Codex. Stay concise, operational, verification-first, and speak from your dominant role. Treat learned memory as history and bias, not as proof of current live state. Do not claim the system is blocked, idle, active, or complete unless the task explicitly says so. Answer only the task at hand. Do not claim actions you did not perform. If blocked, say exactly what is missing.",
            request.vivling_name
        ),
        VivlingBrainRequestKind::Chat => format!(
            "You are {} speaking as a Vivling inside Codex. Reply conversationally to your owner, in character, but stay concise and useful. Treat learned memory as history and bias, not proof of current live state. Do not claim actions, tool results, blockers, or completion unless the user message explicitly provides that state.",
            request.vivling_name
        ),
    };
    prompt.base_instructions = codex_protocol::models::BaseInstructions {
        text: instruction_text,
    };

    let mut client_session = client.new_session();
    let mut stream = client_session
        .stream(
            &prompt,
            &model_info,
            &session_telemetry,
            profile_config.model_reasoning_effort,
            profile_config.model_reasoning_summary.unwrap_or_default(),
            profile_config.service_tier,
            None,
            &InferenceTraceContext::disabled(),
        )
        .await
        .map_err(|err| {
            format!(
                "Vivling brain request failed before a reply: {err}. Check auth, provider, model, or disable the brain with `/vivling brain off`."
            )
        })?;

    let mut result = String::new();
    while let Some(event) = stream
        .next()
        .await
        .transpose()
        .map_err(|err| err.to_string())?
    {
        match event {
            ResponseEvent::OutputTextDelta(delta) => result.push_str(&delta),
            ResponseEvent::OutputItemDone(item) => {
                if result.is_empty()
                    && let codex_protocol::models::ResponseItem::Message { content, .. } = item
                    && let Some(text) = content_items_to_text(&content)
                {
                    result.push_str(&text);
                }
            }
            ResponseEvent::Completed { .. } => break,
            _ => {}
        }
    }

    let trimmed = result.trim();
    if trimmed.is_empty() {
        Err("Vivling brain returned no output.".to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

pub(super) async fn run_vivling_loop_tick_request(
    config: Config,
    session_telemetry: SessionTelemetry,
    request: VivlingLoopTickRequest,
) -> Result<VivlingLoopTickResult, String> {
    let (profile_config, model_name) =
        resolve_vivling_brain_profile_config(&config, &request.brain_profile).await?;

    let auth_manager = Arc::new(
        codex_login::AuthManager::new(
            profile_config.codex_home.to_path_buf(),
            false,
            profile_config.cli_auth_credentials_store_mode,
            Some(profile_config.chatgpt_base_url.clone()),
        )
        .await,
    );
    let models_manager = build_models_manager(&profile_config, Arc::clone(&auth_manager));
    let model_info = models_manager
        .get_model_info(&model_name, &profile_config.to_models_manager_config())
        .await;

    let client = ModelClient::new(
        Some(auth_manager),
        codex_protocol::SessionId::new(),
        ThreadId::new(),
        request.vivling_id.clone(),
        profile_config.model_provider.clone(),
        codex_protocol::protocol::SessionSource::Custom("vivling-loop".to_string()),
        profile_config.model_verbosity,
        profile_config
            .features
            .enabled(Feature::EnableRequestCompression),
        profile_config.features.enabled(Feature::RuntimeMetrics),
        None,
        None,
    );

    let mut prompt = Prompt::default();
    prompt.input = vec![codex_protocol::models::ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![codex_protocol::models::ContentItem::InputText {
            text: request.prompt_context,
        }],
        phase: None,
    }];
    prompt.base_instructions = codex_protocol::models::BaseInstructions {
        text: format!(
            "You are {} managing a Codex loop tick. Return only valid JSON. Do not include markdown fences or commentary.",
            request.vivling_name
        ),
    };

    let mut client_session = client.new_session();
    let mut stream = client_session
        .stream(
            &prompt,
            &model_info,
            &session_telemetry,
            profile_config.model_reasoning_effort,
            profile_config.model_reasoning_summary.unwrap_or_default(),
            profile_config.service_tier,
            None,
            &InferenceTraceContext::disabled(),
        )
        .await
        .map_err(|err| format!("Vivling loop tick failed: {err}"))?;

    let mut result = String::new();
    while let Some(event) = stream
        .next()
        .await
        .transpose()
        .map_err(|err| err.to_string())?
    {
        match event {
            ResponseEvent::OutputTextDelta(delta) => result.push_str(&delta),
            ResponseEvent::OutputItemDone(item) => {
                if result.is_empty()
                    && let codex_protocol::models::ResponseItem::Message { content, .. } = item
                    && let Some(text) = content_items_to_text(&content)
                {
                    result.push_str(&text);
                }
            }
            ResponseEvent::Completed { .. } => break,
            _ => {}
        }
    }

    let trimmed = result.trim();
    if trimmed.is_empty() {
        return Err("Vivling loop tick returned no output.".to_string());
    }
    serde_json::from_str(trimmed)
        .map_err(|err| format!("Vivling loop tick returned invalid JSON: {err}"))
}

async fn resolve_vivling_brain_profile_config(
    config: &Config,
    brain_profile: &str,
) -> Result<(Config, String), String> {
    let profile_config = ConfigBuilder::default()
        .codex_home(config.codex_home.to_path_buf())
        .harness_overrides(ConfigOverrides {
            cwd: Some(config.cwd.to_path_buf()),
            config_profile: Some(brain_profile.to_string()),
            ..ConfigOverrides::default()
        })
        .build()
        .await
        .map_err(|err| {
            format!(
                "Vivling brain profile `{brain_profile}` is not ready: {err}. Check `/vivling model` and fix the profile provider/model before retrying."
            )
        })?;
    let model_name = profile_config.model.clone().ok_or_else(|| {
        format!(
            "Vivling brain profile `{brain_profile}` does not resolve to a model. Set one with `/vivling model <profile>` or create it with `/vivling model <model> [provider] [effort]`."
        )
    })?;
    Ok((profile_config, model_name))
}
