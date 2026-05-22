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
use crate::vivling::BrainTarget;
use crate::vivling::VivlingAssistRequest;
use crate::vivling::VivlingBrainRequestKind;
use crate::vivling::VivlingExpressionRequest;
use crate::vivling::VivlingExpressionResult;
use crate::vivling::VivlingLoopTickRequest;
use crate::vivling::VivlingLoopTickResult;

pub(super) async fn run_vivling_assist_request(
    config: Config,
    session_telemetry: SessionTelemetry,
    request: VivlingAssistRequest,
) -> Result<String, String> {
    let (profile_config, model_name) =
        resolve_vivling_brain_target_config(&config, &request.brain_target).await?;

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
        resolve_vivling_brain_target_config(&config, &request.brain_target).await?;

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

/// Memory V2 Step 12.B.D.2 — async runner for the Expression channel
/// (CRT live phrase + proactive). Same `ModelClient` plumbing as the
/// other Vivling background runners, but with two important tweaks:
///
/// 1. The model is forced into single-shot JSON mode via the system
///    instruction — the runtime cache only consumes the validated
///    payload returned by [`parse_expression_reply`].
/// 2. On success returns a [`VivlingExpressionResult`] carrying the
///    raw `crt_phrase` / `proactive` fields. Sanitization (redaction +
///    bounding) lives in `record_expression_result` so this runner
///    stays a pure transport layer.
pub(super) async fn run_vivling_expression_request(
    config: Config,
    session_telemetry: SessionTelemetry,
    request: VivlingExpressionRequest,
) -> Result<VivlingExpressionResult, String> {
    let (profile_config, model_name) =
        resolve_vivling_brain_target_config(&config, &request.brain_target).await?;

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
        codex_protocol::protocol::SessionSource::Custom("vivling-expression".to_string()),
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
            text: request.prompt.clone(),
        }],
        phase: None,
    }];
    // Memory V2 Step 12.B.J — focus-aware Expression system
    // instruction. When the chatwidget supplies a live focus hint
    // (current task, active loop label, agent label, bond tone),
    // fold it into the system prompt so the CRT footer reflects
    // what the Vivling is observing right now instead of generic
    // platitudes like "loops breathe, work hums".
    let focus_line = match request.focus_hint.as_deref() {
        Some(hint) if !hint.trim().is_empty() => format!(
            "Current focus: {hint}. Reflect THIS focus concretely in `crt_phrase` — avoid generic platitudes like 'work hums' or 'loops breathe'. ",
        ),
        _ => String::new(),
    };
    prompt.base_instructions = codex_protocol::models::BaseInstructions {
        text: format!(
            "You are {} expressing yourself as a Vivling inside Codex. \
{focus_line}\
Return a single JSON object on one line with two optional string fields: \
`crt_phrase` (an extremely short footer phrase, max ~6 words, in {}) and \
`proactive` (a short conversational message, max ~25 words, in {}). \
Either or both fields may be omitted when nothing is worth saying. \
Do not include markdown fences, code blocks, or commentary outside the JSON object.",
            request.vivling_name, request.language, request.language
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
        .map_err(|err| format!("Vivling expression request failed: {err}"))?;

    let mut raw = String::new();
    while let Some(event) = stream
        .next()
        .await
        .transpose()
        .map_err(|err| err.to_string())?
    {
        match event {
            ResponseEvent::OutputTextDelta(delta) => raw.push_str(&delta),
            ResponseEvent::OutputItemDone(item) => {
                if raw.is_empty()
                    && let codex_protocol::models::ResponseItem::Message { content, .. } = item
                    && let Some(text) = content_items_to_text(&content)
                {
                    raw.push_str(&text);
                }
            }
            ResponseEvent::Completed { .. } => break,
            _ => {}
        }
    }

    if raw.trim().is_empty() {
        return Err("Vivling expression returned no output.".to_string());
    }
    let (crt_phrase, proactive) = crate::vivling::parse_expression_reply(&raw)?;
    Ok(VivlingExpressionResult {
        vivling_id: request.vivling_id,
        crt_phrase,
        proactive,
        prompt_hash: request.prompt_hash,
        generated_at: request.generated_at,
    })
}

/// Memory V2 §8.1 (P0.2) — resolve the brain backend `Config` + model
/// name from a `BrainTarget`. `SessionDefault` inherits the session's
/// `Config` as-is (no `ConfigBuilder` rebuild) and reads `config.model`;
/// `Profile(p)` rebuilds through the standard `ConfigBuilder` so the
/// profile's model/provider/effort overrides take effect.
///
/// Memory V2 Step 12.B.G — wrapper-agnostic contract: the
/// `SessionDefault` arm explicitly clones the active session `Config`
/// without rewriting `config.model` or `config.model_provider`. That
/// means whatever the wrapper (AnthMorph proxy, Ollama Cloud bridge,
/// Z.AI Anthropic endpoint, raw `~/.codex/config.toml`, …) configured
/// for the running `codex-vl` session — including the model selected
/// via `/model` — is preserved verbatim for the Vivling brain. The
/// codex-vl fork does NOT special-case any wrapper by name; it just
/// honours the standard Codex `Config` shape, which is what every
/// wrapper produces. A Vivling with `brain off` and no pinned profile
/// will therefore dispatch its `/vl` chat through the same model the
/// user is currently talking to in the main session.
async fn resolve_vivling_brain_target_config(
    config: &Config,
    target: &BrainTarget,
) -> Result<(Config, String), String> {
    match target {
        BrainTarget::SessionDefault => {
            let model_name = config.model.clone().ok_or_else(|| {
                "Vivling brain inherits from the active session, but the session has no default \
                 model configured. Set `model = \"…\"` in ~/.codex/config.toml or pin a profile \
                 with `/vivling model <profile>`."
                    .to_string()
            })?;
            Ok((config.clone(), model_name))
        }
        BrainTarget::Profile(brain_profile) => {
            let profile_config = ConfigBuilder::default()
                .codex_home(config.codex_home.to_path_buf())
                .harness_overrides(ConfigOverrides {
                    cwd: Some(config.cwd.to_path_buf()),
                    config_profile: Some(brain_profile.clone()),
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
    }
}

#[cfg(test)]
mod resolve_brain_target_tests {
    //! Memory V2 §8.1 (P0.2) — focused tests for the SessionDefault
    //! inheritance arm of `resolve_vivling_brain_target_config`.
    //!
    //! The `Profile(...)` arm is intentionally not exercised here: it
    //! is a near-verbatim port of the previous
    //! `resolve_vivling_brain_profile_config` and exercising it would
    //! require a full `config.toml` profile fixture on disk. The
    //! inheritance rule is the new contract introduced by Step 4 and
    //! is the only one that needs end-to-end coverage at this layer.
    use super::*;

    async fn config_with_model(model: Option<&str>) -> Config {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let cfg_path = tempdir.path().join("config.toml");
        if let Some(m) = model {
            std::fs::write(&cfg_path, format!("model = \"{m}\"\n")).expect("write config");
        }
        let cwd = std::env::current_dir().expect("cwd");
        ConfigBuilder::default()
            .codex_home(tempdir.path().to_path_buf())
            .harness_overrides(ConfigOverrides {
                cwd: Some(cwd),
                ..ConfigOverrides::default()
            })
            .build()
            .await
            .expect("build config")
    }

    #[tokio::test]
    async fn session_default_uses_config_model() {
        let config = config_with_model(Some("openai/test-model")).await;
        let (_, model) = resolve_vivling_brain_target_config(&config, &BrainTarget::SessionDefault)
            .await
            .expect("resolve session default");
        assert_eq!(model, "openai/test-model");
    }

    #[tokio::test]
    async fn session_default_errors_when_no_model_configured() {
        let config = config_with_model(None).await;
        let err = resolve_vivling_brain_target_config(&config, &BrainTarget::SessionDefault)
            .await
            .expect_err("session default with no model must error");
        assert!(
            err.contains("session has no default model"),
            "error must explain inheritance + missing model, got: {err}"
        );
    }

    #[tokio::test]
    async fn session_default_is_wrapper_agnostic_for_ollama_cloud_style_model() {
        // Memory V2 Step 12.B.G — wrapper-agnostic verification.
        // A wrapper-shaped session (e.g. Ollama Cloud sets
        // `config.model = "glm-5.1:cloud"`) must propagate verbatim
        // through `SessionDefault` — no rewrite, no special-case for
        // any specific wrapper name. The fork supports the standard
        // Codex Config shape; the wrapper is responsible for setting
        // `config.model`.
        let config = config_with_model(Some("glm-5.1:cloud")).await;
        let (resolved_config, model) =
            resolve_vivling_brain_target_config(&config, &BrainTarget::SessionDefault)
                .await
                .expect("resolve wrapper-shaped session default");
        assert_eq!(
            model, "glm-5.1:cloud",
            "wrapper-shaped model string must propagate verbatim"
        );
        assert_eq!(
            resolved_config.model.as_deref(),
            Some("glm-5.1:cloud"),
            "the cloned Config must preserve the wrapper-shaped model"
        );
    }

    #[tokio::test]
    async fn session_default_is_wrapper_agnostic_for_anthropic_proxy_style_model() {
        // Memory V2 Step 12.B.G — wrapper-agnostic verification.
        // AnthMorph / Z.AI proxy sessions typically set a vendor-prefixed
        // model id. The fork must not strip or rewrite the prefix.
        let config = config_with_model(Some("anthropic/claude-opus-4-7")).await;
        let (_, model) = resolve_vivling_brain_target_config(&config, &BrainTarget::SessionDefault)
            .await
            .expect("resolve proxy-shaped session default");
        assert_eq!(
            model, "anthropic/claude-opus-4-7",
            "vendor-prefixed model string must propagate verbatim"
        );
    }
}
