use std::collections::BTreeMap;
use std::sync::Arc;

use codex_features::Feature;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_mcp::ToolInfo;
use codex_model_provider::create_model_provider;
use codex_model_provider_info::AMAZON_BEDROCK_PROVIDER_ID;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::config_types::WebSearchMode;
use codex_protocol::dynamic_tools::DynamicToolSpec;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::WebSearchToolType;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_tools::DiscoverablePluginInfo;
use codex_tools::DiscoverableTool;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolExposure;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use pretty_assertions::assert_eq;
use serde_json::json;

use crate::session::tests::make_session_and_context;
use crate::session::turn_context::TurnContext;
use crate::tools::handlers::multi_agents_spec::MULTI_AGENT_V1_NAMESPACE;
use crate::tools::router::ToolRouter;
use crate::tools::router::ToolRouterParams;

#[derive(Default)]
struct ToolPlanInputs {
    mcp_tools: Option<Vec<ToolInfo>>,
    deferred_mcp_tools: Option<Vec<ToolInfo>>,
    discoverable_tools: Option<Vec<DiscoverableTool>>,
    dynamic_tools: Vec<DynamicToolSpec>,
}

struct ToolPlanProbe {
    visible_specs: Vec<ToolSpec>,
    visible_names: Vec<String>,
    namespace_functions: BTreeMap<String, Vec<String>>,
    registered_names: Vec<String>,
    exposures: BTreeMap<String, ToolExposure>,
}

impl ToolPlanProbe {
    fn from_router(router: ToolRouter) -> Self {
        let visible_specs = router.model_visible_specs();
        let visible_names = visible_specs
            .iter()
            .map(|spec| spec.name().to_string())
            .collect::<Vec<_>>();
        let namespace_functions = visible_specs
            .iter()
            .filter_map(|spec| match spec {
                ToolSpec::Namespace(namespace) => Some((
                    namespace.name.clone(),
                    namespace
                        .tools
                        .iter()
                        .map(|tool| match tool {
                            ResponsesApiNamespaceTool::Function(tool) => tool.name.clone(),
                        })
                        .collect::<Vec<_>>(),
                )),
                ToolSpec::Function(_)
                | ToolSpec::ToolSearch { .. }
                | ToolSpec::ImageGeneration { .. }
                | ToolSpec::WebSearch { .. }
                | ToolSpec::Freeform(_) => None,
            })
            .collect::<BTreeMap<_, _>>();
        let registered_tool_names = router.registered_tool_names_for_test();
        let registered_names = registered_tool_names
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let exposures = registered_tool_names
            .iter()
            .filter_map(|name| {
                router
                    .tool_exposure_for_test(name)
                    .map(|exposure| (name.to_string(), exposure))
            })
            .collect::<BTreeMap<_, _>>();

        Self {
            visible_specs,
            visible_names,
            namespace_functions,
            registered_names,
            exposures,
        }
    }

    fn assert_visible_contains(&self, expected: &[&str]) {
        for name in expected {
            assert!(
                self.visible_names.iter().any(|visible| visible == name),
                "expected visible tool `{name}` in {:?}",
                self.visible_names
            );
        }
    }

    fn assert_visible_lacks(&self, expected_absent: &[&str]) {
        for name in expected_absent {
            assert!(
                !self.visible_names.iter().any(|visible| visible == name),
                "expected visible tool `{name}` to be absent from {:?}",
                self.visible_names
            );
        }
    }

    fn assert_registered_contains(&self, expected: &[&str]) {
        for name in expected {
            assert!(
                self.registered_names
                    .iter()
                    .any(|registered| registered == name),
                "expected registered tool `{name}` in {:?}",
                self.registered_names
            );
        }
    }

    fn assert_registered_lacks(&self, expected_absent: &[&str]) {
        for name in expected_absent {
            assert!(
                !self
                    .registered_names
                    .iter()
                    .any(|registered| registered == name),
                "expected registered tool `{name}` to be absent from {:?}",
                self.registered_names
            );
        }
    }

    fn namespace_function_names(&self, namespace: &str) -> &[String] {
        self.namespace_functions
            .get(namespace)
            .map_or(&[], Vec::as_slice)
    }

    fn visible_spec(&self, name: &str) -> &ToolSpec {
        self.visible_specs
            .iter()
            .find(|spec| spec.name() == name)
            .unwrap_or_else(|| panic!("expected visible spec `{name}` in {:?}", self.visible_names))
    }

    fn exposure(&self, name: &str) -> ToolExposure {
        *self
            .exposures
            .get(name)
            .unwrap_or_else(|| panic!("expected registered tool `{name}`"))
    }
}

async fn probe_with(
    configure_turn: impl FnOnce(&mut TurnContext),
    inputs: ToolPlanInputs,
) -> ToolPlanProbe {
    let (_session, mut turn) = make_session_and_context().await;
    configure_turn(&mut turn);
    let router = ToolRouter::from_turn_context(
        &turn,
        ToolRouterParams {
            mcp_tools: inputs.mcp_tools,
            deferred_mcp_tools: inputs.deferred_mcp_tools,
            discoverable_tools: inputs.discoverable_tools,
            extension_tool_executors: Vec::new(),
            dynamic_tools: inputs.dynamic_tools.as_slice(),
        },
    );
    ToolPlanProbe::from_router(router)
}

async fn probe(configure_turn: impl FnOnce(&mut TurnContext)) -> ToolPlanProbe {
    probe_with(configure_turn, ToolPlanInputs::default()).await
}

fn set_feature(turn: &mut TurnContext, feature: Feature, enabled: bool) {
    if enabled {
        turn.features
            .enable(feature)
            .expect("test feature should be enableable");
    } else {
        turn.features
            .disable(feature)
            .expect("test feature should be disableable");
    }

    let mut config = (*turn.config).clone();
    if enabled {
        config
            .features
            .enable(feature)
            .expect("test feature should be enableable in config");
    } else {
        config
            .features
            .disable(feature)
            .expect("test feature should be disableable in config");
    }
    turn.config = Arc::new(config);
}

fn set_features(turn: &mut TurnContext, features: &[Feature]) {
    for feature in features {
        set_feature(turn, *feature, /*enabled*/ true);
    }
}

fn update_config(turn: &mut TurnContext, update: impl FnOnce(&mut crate::config::Config)) {
    let mut config = (*turn.config).clone();
    update(&mut config);
    turn.config = Arc::new(config);
}

fn set_web_search_mode(turn: &mut TurnContext, mode: WebSearchMode) {
    update_config(turn, |config| {
        config
            .web_search_mode
            .set(mode)
            .expect("test web search mode should be accepted");
    });
}

fn use_chatgpt_auth(turn: &mut TurnContext) {
    turn.auth_manager = Some(AuthManager::from_auth_for_testing(
        CodexAuth::create_dummy_chatgpt_auth_for_testing(),
    ));
    turn.provider = create_model_provider(
        turn.config.model_provider.clone(),
        turn.auth_manager.clone(),
    );
}

fn use_bedrock_provider(turn: &mut TurnContext) {
    let provider_info = ModelProviderInfo::create_amazon_bedrock_provider(/*aws*/ None);
    update_config(turn, |config| {
        config.model_provider_id = AMAZON_BEDROCK_PROVIDER_ID.to_string();
        config.model_provider = provider_info.clone();
    });
    turn.provider = create_model_provider(provider_info, turn.auth_manager.clone());
}

fn duplicate_primary_environment(turn: &mut TurnContext) {
    let mut second_environment = turn.environments.turn_environments[0].clone();
    second_environment.environment_id = "secondary".to_string();
    turn.environments.turn_environments.push(second_environment);
}

#[test]
fn goal_tools_require_goals_feature() {
    let model_info = model_info();
    let available_models = Vec::new();
    let mut features = Features::with_defaults();
    features.disable(Feature::Goals);
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        &[],
    );
    assert_lacks_tool_name(&tools, "get_goal");
    assert_lacks_tool_name(&tools, "create_goal");
    assert_lacks_tool_name(&tools, "update_goal");

    features.enable(Feature::Goals);
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        &[],
    );
    assert_contains_tool_names(&tools, &["get_goal", "create_goal", "update_goal"]);
}

fn mcp_tool(server: &str, namespace: &str, name: &str) -> ToolInfo {
    ToolInfo {
        server_name: server.to_string(),
        supports_parallel_tool_calls: false,
        server_origin: None,
        callable_name: name.to_string(),
        callable_namespace: namespace.to_string(),
        namespace_description: Some(format!("Tools from {server}.")),
        tool: rmcp::model::Tool {
            name: name.to_string().into(),
            title: None,
            description: Some(format!("{name} test tool").into()),
            input_schema: Arc::new(rmcp::model::object(json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false,
            }))),
            output_schema: None,
            annotations: None,
            execution: None,
            icons: None,
            meta: None,
        },
        connector_id: None,
        connector_name: None,
        plugin_display_names: Vec::new(),
    }
}

fn invalid_mcp_tool(server: &str, namespace: &str, name: &str) -> ToolInfo {
    let mut tool = mcp_tool(server, namespace, name);
    tool.tool.input_schema = Arc::new(rmcp::model::object(json!({
        "type": "null",
    })));
    tool
}

fn dynamic_tool(namespace: Option<&str>, name: &str, defer_loading: bool) -> DynamicToolSpec {
    DynamicToolSpec {
        namespace: namespace.map(str::to_string),
        name: name.to_string(),
        description: format!("{name} dynamic tool"),
        input_schema: json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false,
        }),
        defer_loading,
    }
}

fn discoverable_plugin(id: &str, name: &str) -> DiscoverableTool {
    DiscoverablePluginInfo {
        id: id.to_string(),
        name: name.to_string(),
        description: Some(format!("{name} plugin")),
        has_skills: false,
        mcp_server_names: Vec::new(),
        app_connector_ids: Vec::new(),
    }
    .into()
}

fn has_parameter(spec: &ToolSpec, parameter_name: &str) -> bool {
    serde_json::to_value(spec)
        .expect("tool spec should serialize")
        .pointer(&format!("/parameters/properties/{parameter_name}"))
        .is_some()
}

fn apply_patch_accepts_environment_id(spec: &ToolSpec) -> bool {
    match spec {
        ToolSpec::Freeform(tool) if tool.name == "apply_patch" => {
            tool.format.definition.contains("Environment ID")
        }
        _ => false,
    }
}

#[tokio::test]
async fn shell_family_registers_visible_unified_exec_and_hidden_legacy_shell() {
    let plan = probe(|turn| {
        set_features(turn, &[Feature::ShellTool, Feature::UnifiedExec]);
        set_feature(turn, Feature::ShellZshFork, /*enabled*/ false);
        turn.model_info.shell_type = ConfigShellToolType::ShellCommand;
    })
    .await;

    plan.assert_visible_contains(&["exec_command", "write_stdin"]);
    plan.assert_visible_lacks(&["shell_command"]);
    plan.assert_registered_contains(&["exec_command", "write_stdin", "shell_command"]);
    assert_eq!(plan.exposure("shell_command"), ToolExposure::Hidden);
}

#[tokio::test]
async fn environment_count_controls_environment_backed_tools() {
    let no_environment = probe(|turn| {
        turn.environments.turn_environments.clear();
        set_feature(turn, Feature::ShellTool, /*enabled*/ true);
        turn.model_info.apply_patch_tool_type = Some(ApplyPatchToolType::Freeform);
    })
    .await;
    no_environment.assert_visible_lacks(&[
        "shell_command",
        "exec_command",
        "apply_patch",
        "view_image",
    ]);
    no_environment.assert_registered_lacks(&[
        "shell_command",
        "exec_command",
        "apply_patch",
        "view_image",
    ]);

    let multiple_environments = probe(|turn| {
        duplicate_primary_environment(turn);
        set_feature(turn, Feature::ShellTool, /*enabled*/ true);
        set_feature(turn, Feature::UnifiedExec, /*enabled*/ true);
        turn.model_info.apply_patch_tool_type = Some(ApplyPatchToolType::Freeform);
    })
    .await;
    multiple_environments.assert_visible_contains(&["exec_command", "apply_patch", "view_image"]);
    assert!(has_parameter(
        multiple_environments.visible_spec("exec_command"),
        "environment_id"
    ));
    assert!(apply_patch_accepts_environment_id(
        multiple_environments.visible_spec("apply_patch")
    ));
    assert!(has_parameter(
        multiple_environments.visible_spec("view_image"),
        "environment_id"
    ));
}

#[tokio::test]
async fn host_context_gates_goal_and_agent_job_tools() {
    let feature_disabled = probe(|turn| {
        set_feature(turn, Feature::Goals, /*enabled*/ false);
        turn.goal_tools_supported = true;
    })
    .await;
    feature_disabled.assert_visible_lacks(&["get_goal", "create_goal", "update_goal"]);

    let host_disabled = probe(|turn| {
        set_feature(turn, Feature::Goals, /*enabled*/ true);
        turn.goal_tools_supported = false;
    })
    .await;
    host_disabled.assert_visible_lacks(&["get_goal", "create_goal", "update_goal"]);

    let enabled = probe(|turn| {
        set_feature(turn, Feature::Goals, /*enabled*/ true);
        turn.goal_tools_supported = true;
    })
    .await;
    enabled.assert_visible_contains(&["get_goal", "create_goal", "update_goal"]);

    let review_thread = probe(|turn| {
        set_feature(turn, Feature::Goals, /*enabled*/ true);
        turn.goal_tools_supported = true;
        turn.session_source = SessionSource::SubAgent(SubAgentSource::Review);
    })
    .await;
    review_thread.assert_visible_lacks(&["get_goal", "create_goal", "update_goal"]);

    let normal_agent_job = probe(|turn| {
        set_feature(turn, Feature::SpawnCsv, /*enabled*/ true);
    })
    .await;
    normal_agent_job.assert_visible_contains(&["spawn_agents_on_csv"]);
    normal_agent_job.assert_visible_lacks(&["report_agent_job_result"]);

    let worker_agent_job = probe(|turn| {
        set_feature(turn, Feature::SpawnCsv, /*enabled*/ true);
        turn.session_source =
            SessionSource::SubAgent(SubAgentSource::Other("agent_job:42".to_string()));
    })
    .await;
    worker_agent_job.assert_visible_contains(&["spawn_agents_on_csv", "report_agent_job_result"]);
}

#[tokio::test]
async fn mcp_and_tool_search_follow_direct_and_deferred_tool_exposure() {
    let direct_mcp = probe_with(
        |_| {},
        ToolPlanInputs {
            mcp_tools: Some(vec![mcp_tool("direct", "mcp__direct__", "lookup")]),
            ..ToolPlanInputs::default()
        },
    )
    .await;
    direct_mcp.assert_visible_contains(&[
        "list_mcp_resources",
        "list_mcp_resource_templates",
        "read_mcp_resource",
    ]);
    assert_eq!(
        direct_mcp.namespace_function_names("mcp__direct__"),
        &["lookup".to_string()]
    );

    let searchable_mcp = ToolPlanInputs {
        deferred_mcp_tools: Some(vec![mcp_tool("searchable", "mcp__searchable__", "lookup")]),
        ..ToolPlanInputs::default()
    };

    let missing_model_capability = probe_with(
        |turn| {
            turn.model_info.supports_search_tool = false;
        },
        ToolPlanInputs {
            deferred_mcp_tools: searchable_mcp.deferred_mcp_tools.clone(),
            ..ToolPlanInputs::default()
        },
    )
    .await;
    missing_model_capability.assert_visible_lacks(&["tool_search"]);

    let missing_deferred_tools = probe(|turn| {
        set_feature(turn, Feature::Collab, /*enabled*/ false);
        turn.model_info.supports_search_tool = true;
    })
    .await;
    missing_deferred_tools.assert_visible_lacks(&["tool_search"]);
    missing_deferred_tools.assert_visible_lacks(&[
        "list_mcp_resources",
        "list_mcp_resource_templates",
        "read_mcp_resource",
    ]);

    let missing_namespace_capability = probe_with(
        |turn| {
            turn.model_info.supports_search_tool = true;
            use_bedrock_provider(turn);
        },
        ToolPlanInputs {
            deferred_mcp_tools: searchable_mcp.deferred_mcp_tools.clone(),
            ..ToolPlanInputs::default()
        },
    )
    .await;
    missing_namespace_capability.assert_visible_lacks(&["tool_search"]);

    let enabled = probe_with(
        |turn| {
            turn.model_info.supports_search_tool = true;
        },
        searchable_mcp,
    )
    .await;
    enabled.assert_visible_contains(&["tool_search"]);
    enabled.assert_registered_contains(&["tool_search", "mcp__searchable__lookup"]);
}

#[tokio::test]
async fn invalid_mcp_tools_are_not_registered() {
    let plan = probe_with(
        |_| {},
        ToolPlanInputs {
            mcp_tools: Some(vec![invalid_mcp_tool(
                "invalid",
                "mcp__invalid__",
                "lookup",
            )]),
            ..ToolPlanInputs::default()
        },
    )
    .await;

    plan.assert_visible_lacks(&["mcp__invalid__"]);
    plan.assert_registered_lacks(&["mcp__invalid__lookup"]);
}

#[tokio::test]
async fn request_plugin_install_requires_all_discovery_features_and_discoverable_tools() {
    let discoverable_tools = Some(vec![discoverable_plugin("github", "GitHub")]);
    for disabled_feature in [Feature::ToolSuggest, Feature::Apps, Feature::Plugins] {
        let plan = probe_with(
            |turn| {
                set_features(
                    turn,
                    &[Feature::ToolSuggest, Feature::Apps, Feature::Plugins],
                );
                set_feature(turn, disabled_feature, /*enabled*/ false);
            },
            ToolPlanInputs {
                discoverable_tools: discoverable_tools.clone(),
                ..ToolPlanInputs::default()
            },
        )
        .await;
        plan.assert_visible_lacks(&[
            "list_available_plugins_to_install",
            "request_plugin_install",
        ]);
    }

    let no_candidates = probe(|turn| {
        set_features(
            turn,
            &[Feature::ToolSuggest, Feature::Apps, Feature::Plugins],
        );
    })
    .await;
    no_candidates.assert_visible_lacks(&[
        "list_available_plugins_to_install",
        "request_plugin_install",
    ]);

    let enabled = probe_with(
        |turn| {
            set_features(
                turn,
                &[Feature::ToolSuggest, Feature::Apps, Feature::Plugins],
            );
        },
        ToolPlanInputs {
            discoverable_tools,
            ..ToolPlanInputs::default()
        },
    )
    .await;
    enabled.assert_visible_contains(&[
        "list_available_plugins_to_install",
        "request_plugin_install",
    ]);
}

#[tokio::test]
async fn install_suggestion_tools_stay_visible_without_tool_search() {
    let plan = probe_with(
        |turn| {
            turn.model_info.supports_search_tool = false;
            set_features(
                turn,
                &[Feature::ToolSuggest, Feature::Apps, Feature::Plugins],
            );
        },
        ToolPlanInputs {
            discoverable_tools: Some(vec![discoverable_plugin("github", "GitHub")]),
            ..ToolPlanInputs::default()
        },
    )
    .await;

    plan.assert_visible_contains(&[
        "list_available_plugins_to_install",
        "request_plugin_install",
    ]);
    plan.assert_visible_lacks(&["tool_search"]);
}

#[tokio::test]
async fn request_plugin_install_description_defers_inventory_to_list_tool() {
    let plan = probe_with(
        |turn| {
            set_features(
                turn,
                &[Feature::ToolSuggest, Feature::Apps, Feature::Plugins],
            );
        },
        ToolPlanInputs {
            discoverable_tools: Some(vec![discoverable_plugin("github", "GitHub")]),
            ..ToolPlanInputs::default()
        },
    )
    .await;

    let ToolSpec::Function(ResponsesApiTool {
        description: list_description,
        ..
    }) = plan.visible_spec("list_available_plugins_to_install")
    else {
        panic!("expected list_available_plugins_to_install function spec");
    };
    assert!(list_description.contains(
        "Returns known plugins and connectors that can be passed to `request_plugin_install`."
    ));

    let ToolSpec::Function(ResponsesApiTool {
        description: request_description,
        ..
    }) = plan.visible_spec("request_plugin_install")
    else {
        panic!("expected request_plugin_install function spec");
    };
    assert!(request_description.contains(
        "Use this tool only after `list_available_plugins_to_install` returns a plugin or connector that exactly matches the user's explicit request."
    ));
    assert!(!request_description.contains("github"));
}

#[tokio::test]
async fn code_mode_only_exposes_code_executor_and_hides_nested_tools() {
    let input = ToolPlanInputs {
        dynamic_tools: vec![dynamic_tool(
            Some("codex_app"),
            "lookup",
            /*defer_loading*/ false,
        )],
        ..ToolPlanInputs::default()
    };
    let plain = probe_with(|_| {}, input).await;
    assert_eq!(
        plain.namespace_function_names("codex_app"),
        &["lookup".to_string()]
    );
    plain.assert_visible_lacks(&[
        codex_code_mode::PUBLIC_TOOL_NAME,
        codex_code_mode::WAIT_TOOL_NAME,
    ]);

    let code_mode_only = probe_with(
        |turn| {
            set_features(turn, &[Feature::CodeMode, Feature::CodeModeOnly]);
        },
        ToolPlanInputs {
            dynamic_tools: vec![dynamic_tool(
                Some("codex_app"),
                "lookup",
                /*defer_loading*/ false,
            )],
            ..ToolPlanInputs::default()
        },
    )
    .await;
    code_mode_only.assert_visible_contains(&[
        codex_code_mode::PUBLIC_TOOL_NAME,
        codex_code_mode::WAIT_TOOL_NAME,
    ]);
    assert_eq!(
        code_mode_only.namespace_function_names("codex_app"),
        Vec::<String>::new().as_slice()
    );
}

#[tokio::test]
async fn multi_agent_feature_selects_one_agent_tool_family() {
    let v1 = probe(|turn| {
        set_feature(turn, Feature::Collab, /*enabled*/ true);
        set_feature(turn, Feature::MultiAgentV2, /*enabled*/ false);
    })
    .await;
    v1.assert_visible_contains(&[MULTI_AGENT_V1_NAMESPACE]);
    v1.assert_visible_lacks(&[
        "spawn_agent",
        "send_input",
        "resume_agent",
        "wait_agent",
        "close_agent",
        "send_message",
        "followup_task",
        "list_agents",
    ]);
    assert_eq!(
        v1.namespace_function_names(MULTI_AGENT_V1_NAMESPACE),
        &[
            "close_agent".to_string(),
            "resume_agent".to_string(),
            "send_input".to_string(),
            "spawn_agent".to_string(),
            "wait_agent".to_string(),
        ]
    );

    let v2 = probe(|turn| {
        set_feature(turn, Feature::MultiAgentV2, /*enabled*/ true);
        update_config(turn, |config| {
            config.multi_agent_v2.max_concurrent_threads_per_session = 17;
        });
    })
    .await;
    v2.assert_visible_contains(&[
        "spawn_agent",
        "send_message",
        "followup_task",
        "wait_agent",
        "close_agent",
        "list_agents",
    ]);
    v2.assert_visible_lacks(&["send_input", "resume_agent"]);
    let spawn_agent_description = match v2.visible_spec("spawn_agent") {
        ToolSpec::Function(tool) => tool.description.as_str(),
        other => panic!("expected spawn_agent function spec, got {other:?}"),
    };
    assert!(spawn_agent_description.contains("max_concurrent_threads_per_session = 17"));

    let direct_model_only = probe(|turn| {
        set_features(
            turn,
            &[
                Feature::CodeMode,
                Feature::CodeModeOnly,
                Feature::MultiAgentV2,
            ],
        );
        update_config(turn, |config| {
            config.multi_agent_v2.non_code_mode_only = true;
        });
    })
    .await;
    direct_model_only.assert_visible_contains(&["spawn_agent", "send_message", "wait_agent"]);
    assert_eq!(
        direct_model_only.exposure("spawn_agent"),
        ToolExposure::DirectModelOnly
    );
}

#[tokio::test]
async fn v1_multi_agent_tools_defer_when_tool_search_available() {
    let plan = probe(|turn| {
        turn.model_info.supports_search_tool = true;
        set_feature(turn, Feature::Collab, /*enabled*/ true);
        set_feature(turn, Feature::MultiAgentV2, /*enabled*/ false);
    })
    .await;

    plan.assert_visible_contains(&["tool_search"]);
    plan.assert_visible_lacks(&[
        "spawn_agent",
        "send_input",
        "resume_agent",
        "wait_agent",
        "close_agent",
    ]);
    for tool_name in [
        "spawn_agent",
        "send_input",
        "resume_agent",
        "wait_agent",
        "close_agent",
    ] {
        let namespaced_tool_name = ToolName::namespaced(MULTI_AGENT_V1_NAMESPACE, tool_name);
        let namespaced_tool_name = namespaced_tool_name.to_string();
        assert!(
            plan.registered_names.contains(&namespaced_tool_name),
            "expected namespaced runtime for {tool_name}"
        );
        assert!(
            !plan
                .registered_names
                .contains(&ToolName::plain(tool_name).to_string()),
            "expected no plain runtime for deferred {tool_name}"
        );
        assert_eq!(plan.exposure(&namespaced_tool_name), ToolExposure::Deferred);
    }
    let ToolSpec::ToolSearch { description, .. } = plan.visible_spec("tool_search") else {
        panic!("expected visible tool_search spec");
    };
    assert!(description.contains("- Multi-agent tools: Spawn and manage sub-agents."));
}

#[tokio::test]
async fn multi_agent_v2_can_use_configured_tool_namespace() {
    let namespaced = probe(|turn| {
        set_feature(turn, Feature::MultiAgentV2, /*enabled*/ true);
        update_config(turn, |config| {
            config.multi_agent_v2.tool_namespace = Some("agents".to_string());
        });
    })
    .await;

    namespaced.assert_visible_contains(&["agents"]);
    for tool_name in [
        "spawn_agent",
        "send_message",
        "followup_task",
        "wait_agent",
        "close_agent",
        "list_agents",
    ] {
        namespaced.assert_visible_lacks(&[tool_name]);
        assert!(
            namespaced
                .registered_names
                .contains(&ToolName::namespaced("agents", tool_name).to_string()),
            "expected namespaced runtime for {tool_name}"
        );
        assert!(
            !namespaced
                .registered_names
                .contains(&ToolName::plain(tool_name).to_string()),
            "expected no plain runtime for {tool_name}"
        );
        assert!(
            namespaced
                .namespace_function_names("agents")
                .iter()
                .any(|name| name == tool_name),
            "expected {tool_name} in agents namespace"
        );
    }
}

#[tokio::test]
async fn multi_agent_v2_namespace_is_ignored_without_provider_namespace_support() {
    let plan = probe(|turn| {
        set_feature(turn, Feature::MultiAgentV2, /*enabled*/ true);
        update_config(turn, |config| {
            config.multi_agent_v2.tool_namespace = Some("agents".to_string());
        });
        use_bedrock_provider(turn);
    })
    .await;

    plan.assert_visible_contains(&["spawn_agent", "send_message", "list_agents"]);
    plan.assert_visible_lacks(&["agents"]);
    assert!(
        plan.registered_names
            .contains(&ToolName::plain("spawn_agent").to_string())
    );
    assert!(
        !plan
            .registered_names
            .contains(&ToolName::namespaced("agents", "spawn_agent").to_string())
    );
}

#[tokio::test]
async fn code_mode_only_can_expose_namespaced_multi_agent_v2_as_normal_tools() {
    let plan = probe(|turn| {
        set_features(
            turn,
            &[
                Feature::CodeMode,
                Feature::CodeModeOnly,
                Feature::MultiAgentV2,
            ],
        );
        update_config(turn, |config| {
            config.multi_agent_v2.non_code_mode_only = true;
            config.multi_agent_v2.tool_namespace = Some("agents".to_string());
        });
    })
    .await;

    assert_eq!(plan.visible_names, vec!["exec", "wait", "agents"]);
    for tool_name in [
        "spawn_agent",
        "send_message",
        "followup_task",
        "wait_agent",
        "close_agent",
        "list_agents",
    ] {
        assert!(
            plan.namespace_function_names("agents")
                .iter()
                .any(|name| name == tool_name),
            "expected {tool_name} in agents namespace"
        );
    }
}

#[tokio::test]
async fn hosted_tools_follow_provider_auth_model_and_config_gates() {
    let api_key_auth = probe(|turn| {
        set_feature(turn, Feature::ImageGeneration, /*enabled*/ true);
        turn.model_info.input_modalities = vec![InputModality::Image];
    })
    .await;
    api_key_auth.assert_visible_lacks(&["image_generation"]);

    let image_generation = probe(|turn| {
        use_chatgpt_auth(turn);
        set_feature(turn, Feature::ImageGeneration, /*enabled*/ true);
        turn.model_info.input_modalities = vec![InputModality::Image];
    })
    .await;
    image_generation.assert_visible_contains(&["image_generation"]);

    let live_web_search = probe(|turn| {
        set_web_search_mode(turn, WebSearchMode::Live);
        turn.model_info.web_search_tool_type = WebSearchToolType::TextAndImage;
    })
    .await;
    assert_eq!(
        live_web_search.visible_spec("web_search"),
        &ToolSpec::WebSearch {
            external_web_access: Some(true),
            filters: None,
            user_location: None,
            search_context_size: None,
            search_content_types: Some(vec!["text".to_string(), "image".to_string()]),
        }
    );

    let unsupported_provider = probe(|turn| {
        set_web_search_mode(turn, WebSearchMode::Live);
        use_bedrock_provider(turn);
    })
    .await;
    unsupported_provider.assert_visible_lacks(&["web_search"]);
}

#[test]
fn mcp_resource_tools_are_hidden_without_mcp_servers() {
    let model_info = model_info();
    let features = Features::with_defaults();
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        &[],
    );

    assert!(
        !tools.iter().any(|tool| matches!(
            tool.name(),
            "list_mcp_resources" | "list_mcp_resource_templates" | "read_mcp_resource"
        )),
        "MCP resource tools should be omitted when no MCP servers are configured"
    );
}

#[test]
fn mcp_resource_tools_are_included_when_mcp_servers_are_present() {
    let model_info = model_info();
    let features = Features::with_defaults();
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs(
        &tools_config,
        Some(HashMap::new()),
        /*deferred_mcp_tools*/ None,
        &[],
    );

    assert_contains_tool_names(
        &tools,
        &[
            "list_mcp_resources",
            "list_mcp_resource_templates",
            "read_mcp_resource",
        ],
    );
}

#[test]
#[ignore]
fn test_parallel_support_flags() {
    let model_info = model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::UnifiedExec);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        &[],
    );

    assert_contains_tool_names(&tools, &["exec_command", "write_stdin"]);
}

#[test]
fn test_test_model_info_includes_sync_tool() {
    let mut model_info = model_info();
    model_info.experimental_supported_tools = vec!["test_sync_tool".to_string()];
    let features = Features::with_defaults();
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        &[],
    );

    assert!(tools.iter().any(|tool| tool.name() == "test_sync_tool"));
}

#[test]
fn test_build_specs_mcp_tools_converted() {
    let model_info = model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::UnifiedExec);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Live),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs(
        &tools_config,
        Some(HashMap::from([(
            ToolName::namespaced("test_server/", "do_something_cool"),
            mcp_tool(
                "do_something_cool",
                "Do something cool",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "string_argument": { "type": "string" },
                        "number_argument": { "type": "number" },
                        "object_argument": {
                            "type": "object",
                            "properties": {
                                "string_property": { "type": "string" },
                                "number_property": { "type": "number" },
                            },
                            "required": ["string_property", "number_property"],
                            "additionalProperties": false,
                        },
                    },
                }),
            ),
        )])),
        /*deferred_mcp_tools*/ None,
        &[],
    );

    let tool = find_namespace_function_tool(&tools, "test_server/", "do_something_cool");
    assert_eq!(
        tool,
        &ResponsesApiTool {
            name: "do_something_cool".to_string(),
            parameters: JsonSchema::object(
                BTreeMap::from([
                    (
                        "string_argument".to_string(),
                        JsonSchema::string(/*description*/ None),
                    ),
                    (
                        "number_argument".to_string(),
                        JsonSchema::number(/*description*/ None),
                    ),
                    (
                        "object_argument".to_string(),
                        JsonSchema::object(
                            BTreeMap::from([
                                (
                                    "string_property".to_string(),
                                    JsonSchema::string(/*description*/ None),
                                ),
                                (
                                    "number_property".to_string(),
                                    JsonSchema::number(/*description*/ None),
                                ),
                            ]),
                            Some(vec![
                                "string_property".to_string(),
                                "number_property".to_string(),
                            ]),
                            Some(false.into()),
                        ),
                    ),
                ]),
                /*required*/ None,
                /*additional_properties*/ None
            ),
            description: "Do something cool".to_string(),
            strict: false,
            output_schema: Some(mcp_call_tool_result_output_schema(serde_json::json!({}))),
            defer_loading: None,
        }
    );
}

#[test]
fn namespace_specs_are_hidden_when_namespace_tools_are_disabled() {
    let model_info = model_info();
    let features = Features::with_defaults();
    let available_models = Vec::new();
    let mut tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    tools_config.namespace_tools = false;

    let (tools, registry) = build_specs(
        &tools_config,
        Some(HashMap::from([(
            ToolName::namespaced("mcp__sample__", "echo"),
            mcp_tool("echo", "Echo", serde_json::json!({"type": "object"})),
        )])),
        /*deferred_mcp_tools*/ None,
        &[],
    );

    assert_lacks_tool_name(&tools, "mcp__sample__");
    assert_contains_tool_names(&tools, &["mcp__sample__echo"]);
    assert!(registry.has_tool(&ToolName::namespaced("mcp__sample__", "echo")));
}

#[test]
fn namespaced_dynamic_specs_are_hidden_when_namespace_tools_are_disabled() {
    let model_info = model_info();
    let features = Features::with_defaults();
    let available_models = Vec::new();
    let mut tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    tools_config.namespace_tools = false;
    let dynamic_tools = vec![
        DynamicToolSpec {
            namespace: Some("codex_app".to_string()),
            name: "automation_update".to_string(),
            description: "Create or update automations.".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
            defer_loading: false,
        },
        DynamicToolSpec {
            namespace: None,
            name: "plain_dynamic".to_string(),
            description: "Plain dynamic tool.".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
            defer_loading: false,
        },
    ];

    let (tools, _) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        &dynamic_tools,
    );

    assert_lacks_tool_name(&tools, "codex_app");
    assert_contains_tool_names(&tools, &["plain_dynamic"]);
}

#[test]
fn test_build_specs_mcp_namespace_description_falls_back_when_missing() {
    let model_info = model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::UnifiedExec);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs(
        &tools_config,
        Some(HashMap::from([(
            ToolName::namespaced("test_server/", "do_something_cool"),
            mcp_tool(
                "do_something_cool",
                "Do something cool",
                serde_json::json!({"type": "object"}),
            ),
        )])),
        /*deferred_mcp_tools*/ None,
        &[],
    );

    let namespace_tool = find_tool(&tools, "test_server/");
    let ToolSpec::Namespace(namespace) = namespace_tool else {
        panic!("expected namespace tool");
    };
    assert_eq!(
        namespace.description,
        "Tools in the test_server/ namespace."
    );
}

#[test]
fn test_build_specs_mcp_tools_sorted_by_name() {
    let model_info = model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::UnifiedExec);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });

    let tools_map = HashMap::from([
        (
            ToolName::namespaced("test_server/", "do"),
            mcp_tool("do", "a", serde_json::json!({"type": "object"})),
        ),
        (
            ToolName::namespaced("test_server/", "something"),
            mcp_tool("something", "b", serde_json::json!({"type": "object"})),
        ),
        (
            ToolName::namespaced("test_server/", "cool"),
            mcp_tool("cool", "c", serde_json::json!({"type": "object"})),
        ),
    ]);

    let (tools, _) = build_specs(
        &tools_config,
        Some(tools_map),
        /*deferred_mcp_tools*/ None,
        &[],
    );

    assert_eq!(
        namespace_function_names(&tools, "test_server/"),
        vec![
            "cool".to_string(),
            "do".to_string(),
            "something".to_string(),
        ]
    );
}

#[test]
fn search_tool_description_lists_each_mcp_source_once() {
    let model_info = search_capable_model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::Apps);
    features.enable(Feature::ToolSearch);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });

    let (tools, registry) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        Some(vec![
            deferred_mcp_tool(
                "_create_event",
                "mcp__codex_apps__calendar",
                CODEX_APPS_MCP_SERVER_NAME,
                Some("Calendar"),
                Some("Plan events and manage your calendar."),
            ),
            deferred_mcp_tool(
                "_list_events",
                "mcp__codex_apps__calendar",
                CODEX_APPS_MCP_SERVER_NAME,
                Some("Calendar"),
                Some("Plan events and manage your calendar."),
            ),
            deferred_mcp_tool(
                "_search_threads",
                "mcp__codex_apps__gmail",
                CODEX_APPS_MCP_SERVER_NAME,
                Some("Gmail"),
                Some("Find and summarize email threads."),
            ),
            deferred_mcp_tool(
                "echo",
                "mcp__rmcp__",
                "rmcp",
                /*connector_name*/ None,
                Some("Remote memory tools."),
            ),
        ]),
        &[],
    );

    let search_tool = find_tool(&tools, TOOL_SEARCH_TOOL_NAME);
    let ToolSpec::ToolSearch { description, .. } = search_tool else {
        panic!("expected tool_search tool");
    };
    let description = description.as_str();
    assert!(description.contains("- Calendar: Plan events and manage your calendar."));
    assert!(description.contains("- Gmail: Find and summarize email threads."));
    assert_eq!(
        description
            .matches("- Calendar: Plan events and manage your calendar.")
            .count(),
        1
    );
    assert!(description.contains("- rmcp: Remote memory tools."));
    assert!(!description.contains("mcp__rmcp__echo"));

    assert!(registry.has_tool(&ToolName::namespaced(
        "mcp__codex_apps__calendar",
        "_create_event",
    )));
    assert!(registry.has_tool(&ToolName::namespaced("mcp__rmcp__", "echo")));
}

#[test]
fn search_tool_requires_model_capability_and_enabled_feature() {
    let model_info = search_capable_model_info();
    let deferred_mcp_tools = Some(vec![deferred_mcp_tool(
        "_create_event",
        "mcp__codex_apps__calendar",
        CODEX_APPS_MCP_SERVER_NAME,
        Some("Calendar"),
        /*description*/ None,
    )]);

    let features = Features::with_defaults();
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &ModelInfo {
            supports_search_tool: false,
            ..model_info.clone()
        },
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        deferred_mcp_tools.clone(),
        &[],
    );
    assert_lacks_tool_name(&tools, TOOL_SEARCH_TOOL_NAME);

    let mut features_without_tool_search = Features::with_defaults();
    features_without_tool_search.disable(Feature::ToolSearch);
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features_without_tool_search,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        deferred_mcp_tools.clone(),
        &[],
    );
    assert_lacks_tool_name(&tools, TOOL_SEARCH_TOOL_NAME);

    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        deferred_mcp_tools,
        &[],
    );
    assert_contains_tool_names(&tools, &[TOOL_SEARCH_TOOL_NAME]);
}

#[test]
fn no_search_tool_when_namespaces_disabled() {
    let model_info = search_capable_model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::ToolSearch);
    let available_models = Vec::new();
    let mut tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    tools_config.namespace_tools = false;

    let (tools, registry) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        Some(vec![deferred_mcp_tool(
            "_create_event",
            "mcp__codex_apps__calendar",
            CODEX_APPS_MCP_SERVER_NAME,
            Some("Calendar"),
            Some("Plan events and manage your calendar."),
        )]),
        &[],
    );

    assert_lacks_tool_name(&tools, TOOL_SEARCH_TOOL_NAME);
    assert!(!registry.has_tool(&ToolName::plain(TOOL_SEARCH_TOOL_NAME)));
}

#[test]
fn search_tool_registers_for_deferred_dynamic_tools() {
    let model_info = search_capable_model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::ToolSearch);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let dynamic_tools = vec![
        DynamicToolSpec {
            namespace: Some("codex_app".to_string()),
            name: "automation_update".to_string(),
            description: "Create, update, view, or delete recurring automations.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "mode": { "type": "string" },
                },
            }),
            defer_loading: true,
        },
        DynamicToolSpec {
            namespace: Some("codex_app".to_string()),
            name: "automation_list".to_string(),
            description: "List recurring automations.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
            }),
            defer_loading: true,
        },
    ];

    let (tools, registry) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        &dynamic_tools,
    );

    let search_tool = find_tool(&tools, TOOL_SEARCH_TOOL_NAME);
    let ToolSpec::ToolSearch { description, .. } = search_tool else {
        panic!("expected tool_search tool");
    };
    assert!(description.contains("- Dynamic tools: Tools provided by the current Codex thread."));
    assert_contains_tool_names(&tools, &[TOOL_SEARCH_TOOL_NAME]);
    assert_lacks_tool_name(&tools, "codex_app");
    assert!(registry.has_tool(&ToolName::plain(TOOL_SEARCH_TOOL_NAME)));
    assert!(registry.has_tool(&ToolName::namespaced("codex_app", "automation_update")));
    assert!(registry.has_tool(&ToolName::namespaced("codex_app", "automation_list")));
}

#[test]
fn dynamic_tools_register_flat_and_namespaced_manage_loops_aliases() {
    let model_info = search_capable_model_info();
    let features = Features::with_defaults();
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let input_schema = json!({
        "type": "object",
        "properties": {
            "action": { "type": "string" },
        },
        "required": ["action"],
        "additionalProperties": false,
    });
    let dynamic_tools = vec![
        DynamicToolSpec {
            namespace: None,
            name: "manage_loops".to_string(),
            description: "Manage local recurring loop jobs.".to_string(),
            input_schema: input_schema.clone(),
            defer_loading: false,
        },
        DynamicToolSpec {
            namespace: Some("codex_app".to_string()),
            name: "manage_loops".to_string(),
            description: "Manage local recurring loop jobs.".to_string(),
            input_schema,
            defer_loading: false,
        },
    ];

    let (tools, registry) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        &dynamic_tools,
    );

    let flat_tool = find_tool(&tools, "manage_loops");
    let ToolSpec::Function(ResponsesApiTool { parameters, .. }) = flat_tool else {
        panic!("expected flat manage_loops function tool");
    };
    assert_eq!(
        parameters.required.as_ref(),
        Some(&vec!["action".to_string()])
    );
    assert_eq!(
        namespace_function_names(&tools, "codex_app"),
        vec!["manage_loops".to_string()]
    );
    assert!(registry.has_tool(&ToolName::plain("manage_loops")));
    assert!(registry.has_tool(&ToolName::namespaced("codex_app", "manage_loops")));
}

#[test]
fn search_tool_is_hidden_for_deferred_dynamic_tools_when_namespace_tools_are_disabled() {
    let model_info = search_capable_model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::ToolSearch);
    let available_models = Vec::new();
    let mut tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    tools_config.namespace_tools = false;
    let dynamic_tools = vec![
        DynamicToolSpec {
            namespace: Some("codex_app".to_string()),
            name: "automation_update".to_string(),
            description: "Create or update automations.".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
            defer_loading: true,
        },
        DynamicToolSpec {
            namespace: None,
            name: "plain_dynamic".to_string(),
            description: "Plain dynamic tool.".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
            defer_loading: true,
        },
    ];

    let (tools, registry) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        &dynamic_tools,
    );

    assert_lacks_tool_name(&tools, TOOL_SEARCH_TOOL_NAME);
    assert_lacks_tool_name(&tools, "codex_app");
    assert_lacks_tool_name(&tools, "plain_dynamic");
    assert!(!registry.has_tool(&ToolName::plain(TOOL_SEARCH_TOOL_NAME)));
    assert!(registry.has_tool(&ToolName::namespaced("codex_app", "automation_update")));
    assert!(registry.has_tool(&ToolName::plain("plain_dynamic")));
}

#[test]
fn request_plugin_install_is_not_registered_without_feature_flag() {
    let model_info = search_capable_model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::ToolSearch);
    features.enable(Feature::Apps);
    features.enable(Feature::Plugins);
    features.disable(Feature::ToolSuggest);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs_with_inputs_for_test(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        Some(vec![discoverable_connector(
            "connector_2128aebfecb84f64a069897515042a44",
            "Google Calendar",
            "Plan events and schedules.",
        )]),
        /*extension_tool_executors*/ &[],
        &[],
    );

    assert!(
        !tools
            .iter()
            .any(|tool| tool.name() == REQUEST_PLUGIN_INSTALL_TOOL_NAME)
    );
}

#[test]
fn request_plugin_install_can_be_registered_without_search_tool() {
    let model_info = ModelInfo {
        supports_search_tool: false,
        ..search_capable_model_info()
    };
    let mut features = Features::with_defaults();
    features.enable(Feature::Apps);
    features.enable(Feature::Plugins);
    features.enable(Feature::ToolSuggest);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });
    let (tools, _) = build_specs_with_inputs_for_test(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        Some(vec![discoverable_connector(
            "connector_2128aebfecb84f64a069897515042a44",
            "Google Calendar",
            "Plan events and schedules.",
        )]),
        /*extension_tool_executors*/ &[],
        &[],
    );

    assert_contains_tool_names(&tools, &[REQUEST_PLUGIN_INSTALL_TOOL_NAME]);
    let request_plugin_install = find_tool(&tools, REQUEST_PLUGIN_INSTALL_TOOL_NAME);
    assert_lacks_tool_name(&tools, TOOL_SEARCH_TOOL_NAME);

    let ToolSpec::Function(ResponsesApiTool { description, .. }) = request_plugin_install else {
        panic!("expected function tool");
    };
    assert!(description.contains(
        "Use this tool only to ask the user to install one known plugin or connector from the list below. The list contains known candidates that are not currently installed."
    ));
    assert!(description.contains(
        "`tool_search` is not available, or it has already been called and did not find or make the requested tool callable."
    ));
}

#[test]
fn request_plugin_install_description_lists_discoverable_tools() {
    let model_info = search_capable_model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::Apps);
    features.enable(Feature::Plugins);
    features.enable(Feature::ToolSearch);
    features.enable(Feature::ToolSuggest);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });

    let discoverable_tools = vec![
        discoverable_connector(
            "connector_2128aebfecb84f64a069897515042a44",
            "Google Calendar",
            "Plan events and schedules.",
        ),
        discoverable_connector(
            "connector_68df038e0ba48191908c8434991bbac2",
            "Gmail",
            "Find and summarize email threads.",
        ),
        DiscoverableTool::Plugin(Box::new(DiscoverablePluginInfo {
            id: "sample@test".to_string(),
            name: "Sample Plugin".to_string(),
            description: None,
            has_skills: true,
            mcp_server_names: vec!["sample-docs".to_string()],
            app_connector_ids: vec!["connector_sample".to_string()],
        })),
    ];

    let (tools, registry) = build_specs_with_inputs_for_test(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        Some(discoverable_tools),
        /*extension_tool_executors*/ &[],
        &[],
    );
    assert!(registry.has_tool(&ToolName::plain(REQUEST_PLUGIN_INSTALL_TOOL_NAME)));

    let request_plugin_install = find_tool(&tools, REQUEST_PLUGIN_INSTALL_TOOL_NAME);
    let ToolSpec::Function(ResponsesApiTool {
        description,
        parameters,
        ..
    }) = request_plugin_install
    else {
        panic!("expected function tool");
    };
    assert!(description.contains(
        "Use this tool only to ask the user to install one known plugin or connector from the list below. The list contains known candidates that are not currently installed."
    ));
    assert!(description.contains("Google Calendar"));
    assert!(description.contains("Gmail"));
    assert!(description.contains("Sample Plugin"));
    assert!(description.contains("Plan events and schedules."));
    assert!(description.contains("Find and summarize email threads."));
    assert!(description.contains("id: `sample@test`, type: plugin, action: install"));
    assert!(description.contains("`action_type`: `install`"));
    assert!(
        description.contains("skills; MCP servers: sample-docs; app connectors: connector_sample")
    );
    assert!(
        description.contains(
            "The user explicitly asks to use a specific plugin or connector that is not already available in the current context or active `tools` list."
        )
    );
    assert!(description.contains(
        "`tool_search` is not available, or it has already been called and did not find or make the requested tool callable."
    ));
    assert!(description.contains(
        "The plugin or connector is one of the known installable plugins or connectors listed below. Only ask to install plugins or connectors from this list."
    ));
    assert!(description.contains(
        "Do not use this tool for adjacent capabilities, broad recommendations, or tools that merely seem useful."
    ));
    assert!(description.contains("IMPORTANT: DO NOT call this tool in parallel with other tools."));
    assert!(description.contains(
        "If current active tools aren't relevant and `tool_search` is available, only call this tool after `tool_search` has already been tried and found no relevant tool."
    ));
    assert!(!description.contains("targeted lookup"));
    assert!(!description.contains("broad or speculative searches"));
    assert!(description.contains("Only proceed when one listed plugin or connector exactly fits."));
    assert!(description.contains(
        "If we found both connectors and plugins to install, use plugins first, only use connectors if the corresponding plugin is installed but the connector is not."
    ));
    assert!(!description.contains("{{discoverable_tools}}"));
    assert!(!description.contains("tool_search fails to find a good match"));
    let (_, required) = expect_object_schema(parameters);
    assert_eq!(
        required,
        Some(&vec![
            "tool_type".to_string(),
            "action_type".to_string(),
            "tool_id".to_string(),
            "suggest_reason".to_string(),
        ])
    );
}

#[test]
fn code_mode_augments_mcp_tool_descriptions_with_namespaced_sample() {
    let model_info = model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::CodeMode);
    features.enable(Feature::CodeModeOnly);
    features.enable(Feature::UnifiedExec);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });

    let (tools, _) = build_specs(
        &tools_config,
        Some(HashMap::from([(
            ToolName::namespaced("mcp__sample__", "echo"),
            mcp_tool(
                "echo",
                "Echo text",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": {"type": "string"}
                    },
                    "required": ["message"],
                    "additionalProperties": false
                }),
            ),
        )])),
        /*deferred_mcp_tools*/ None,
        &[],
    );

    let ToolSpec::Freeform(FreeformTool { description, .. }) = find_tool(&tools, "exec") else {
        panic!("expected freeform tool");
    };

    assert!(description.contains(
        r#"### `mcp__sample__echo`
Echo text

exec tool declaration:
```ts
declare const tools: { mcp__sample__echo(args: { message: string; }): Promise<CallToolResult>; };
```"#
    ));
}

#[test]
fn code_mode_preserves_nullable_and_literal_mcp_input_shapes() {
    let model_info = model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::CodeMode);
    features.enable(Feature::UnifiedExec);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });

    let (tools, _) = build_specs(
        &tools_config,
        Some(HashMap::from([(
            ToolName::namespaced("mcp__sample__", "fn"),
            mcp_tool(
                "fn",
                "Sample fn",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "open": {
                            "anyOf": [
                                {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "ref_id": {"type": "string"},
                                            "lineno": {"anyOf": [{"type": "integer"}, {"type": "null"}]}
                                        },
                                        "required": ["ref_id"],
                                        "additionalProperties": false
                                    }
                                },
                                {"type": "null"}
                            ]
                        },
                        "tagged_list": {
                            "anyOf": [
                                {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "kind": {"type": "const", "const": "tagged"},
                                            "variant": {"type": "enum", "enum": ["alpha", "beta"]},
                                            "scope": {"type": "enum", "enum": ["one", "two"]}
                                        },
                                        "required": ["kind", "variant", "scope"]
                                    }
                                },
                                {"type": "null"}
                            ]
                        },
                        "response_length": {"type": "enum", "enum": ["short", "medium", "long"]}
                    },
                    "additionalProperties": false
                }),
            ),
        )])),
        /*deferred_mcp_tools*/ None,
        &[],
    );

    let ResponsesApiTool { description, .. } =
        find_namespace_function_tool(&tools, "mcp__sample__", "fn");

    assert!(description.contains(
        r#"exec tool declaration:
```ts
declare const tools: { mcp__sample__fn(args: { open?: Array<{ lineno?: number | null; ref_id: string; }> | null; response_length?: "short" | "medium" | "long"; tagged_list?: Array<{ kind: "tagged"; scope: "one" | "two"; variant: "alpha" | "beta"; }> | null; }): Promise<CallToolResult>; };
```"#
    ));
}

#[test]
fn code_mode_augments_builtin_tool_descriptions_with_typed_sample() {
    let model_info = model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::CodeMode);
    features.enable(Feature::UnifiedExec);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });

    let (tools, _) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        &[],
    );
    let ToolSpec::Function(ResponsesApiTool { description, .. }) =
        find_tool(&tools, VIEW_IMAGE_TOOL_NAME)
    else {
        panic!("expected function tool");
    };

    assert_eq!(
        description,
        "View a local image from the filesystem (only use if given a full filepath by the user, and the image isn't already attached to the thread context within <image ...> tags).\n\nexec tool declaration:\n```ts\ndeclare const tools: { view_image(args: {\n  // Local filesystem path to an image file\n  path: string;\n}): Promise<{\n  // Image detail hint returned by view_image. Returns `high` for default resized behavior or `original` when original resolution is preserved.\n  detail: \"high\" | \"original\";\n  // Data URL for the loaded image.\n  image_url: string;\n}>; };\n```"
    );
}

#[test]
fn code_mode_only_exec_description_includes_full_nested_tool_details() {
    let model_info = model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::CodeMode);
    features.enable(Feature::CodeModeOnly);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });

    let (tools, _) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        &[],
    );
    let ToolSpec::Freeform(FreeformTool { description, .. }) = find_tool(&tools, "exec") else {
        panic!("expected freeform tool");
    };

    assert!(!description.contains("Enabled nested tools:"));
    assert!(!description.contains("Nested tool reference:"));
    assert!(description.starts_with("Run JavaScript code to orchestrate/compose tool calls"));
    assert!(!description.contains("do not attempt to use any other tools directly"));
    assert!(description.contains("### `update_plan`"));
    assert!(description.contains("### `view_image`"));
}

#[test]
fn code_mode_only_exec_description_includes_extension_tool_details() {
    let model_info = model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::CodeMode);
    features.enable(Feature::CodeModeOnly);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });

    let extension_tool_executors = vec![extension_tool_executor(
        "extension_echo",
        "Echoes arguments through an extension tool.",
    )];
    let (tools, _) = build_specs_with_inputs_for_test(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        /*discoverable_tools*/ None,
        &extension_tool_executors,
        &[],
    );
    let ToolSpec::Freeform(FreeformTool { description, .. }) = find_tool(&tools, "exec") else {
        panic!("expected freeform tool");
    };

    assert!(description.contains("### `extension_echo`"));
    assert!(description.contains("Echoes arguments through an extension tool."));
}

#[test]
fn code_mode_exec_description_omits_nested_tool_details_when_not_code_mode_only() {
    let model_info = model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::CodeMode);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });

    let (tools, _) = build_specs(
        &tools_config,
        /*mcp_tools*/ None,
        /*deferred_mcp_tools*/ None,
        &[],
    );
    let ToolSpec::Freeform(FreeformTool { description, .. }) = find_tool(&tools, "exec") else {
        panic!("expected freeform tool");
    };

    assert!(!description.starts_with(
        "Use `exec/wait` tool to run all other tools, do not attempt to use any other tools directly"
    ));
    assert!(!description.contains("### `update_plan`"));
    assert!(!description.contains("### `view_image`"));
}

fn model_info() -> ModelInfo {
    serde_json::from_value(json!({
        "slug": "gpt-5-codex",
        "display_name": "GPT-5 Codex",
        "description": null,
        "supported_reasoning_levels": [],
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 1,
        "availability_nux": null,
        "upgrade": null,
        "base_instructions": "base",
        "model_messages": null,
        "supports_reasoning_summaries": false,
        "default_reasoning_summary": "auto",
        "support_verbosity": false,
        "default_verbosity": null,
        "apply_patch_tool_type": "freeform",
        "truncation_policy": {
            "mode": "bytes",
            "limit": 10000
        },
        "supports_parallel_tool_calls": false,
        "supports_image_detail_original": false,
        "context_window": null,
        "auto_compact_token_limit": null,
        "effective_context_window_percent": 95,
        "experimental_supported_tools": [],
        "input_modalities": ["text", "image"],
        "supports_search_tool": false
    }))
    .expect("deserialize test model")
}

fn search_capable_model_info() -> ModelInfo {
    ModelInfo {
        supports_search_tool: true,
        ..model_info()
    }
}

fn build_specs(
    config: &ToolsConfig,
    mcp_tools: Option<HashMap<ToolName, rmcp::model::Tool>>,
    deferred_mcp_tools: Option<Vec<ToolInfo>>,
    dynamic_tools: &[DynamicToolSpec],
) -> (Vec<ToolSpec>, ToolRegistry) {
    build_specs_with_inputs_for_test(
        config,
        mcp_tools,
        deferred_mcp_tools,
        /*discoverable_tools*/ None,
        /*extension_tool_executors*/ &[],
        dynamic_tools,
    )
}

fn build_specs_with_inputs_for_test(
    config: &ToolsConfig,
    mcp_tools: Option<HashMap<ToolName, rmcp::model::Tool>>,
    deferred_mcp_tools: Option<Vec<ToolInfo>>,
    discoverable_tools: Option<Vec<DiscoverableTool>>,
    extension_tool_executors: &[Arc<dyn ToolExecutor<ExtensionToolCall>>],
    dynamic_tools: &[DynamicToolSpec],
) -> (Vec<ToolSpec>, ToolRegistry) {
    let mcp_tool_inputs = mcp_tools.as_ref().map(|mcp_tools| {
        mcp_tools
            .iter()
            .map(|(name, tool)| tool_info_from_parts(name, tool.clone()))
            .collect::<Vec<_>>()
    });
    let params = ToolRegistryBuildParams {
        mcp_tools: mcp_tool_inputs.as_deref(),
        deferred_mcp_tools: deferred_mcp_tools.as_deref(),
        discoverable_tools: discoverable_tools.as_deref(),
        extension_tool_executors,
        dynamic_tools,
        default_agent_type_description: DEFAULT_AGENT_TYPE_DESCRIPTION,
        wait_agent_timeouts: wait_agent_timeout_options(),
    };
    let mut executors = collect_tool_executors(config, params);
    append_tool_search_executor(config, &mut executors);
    prepend_code_mode_executors(config, &mut executors);
    build_model_visible_specs_and_registry(config, executors, hosted_model_tool_specs(config))
}

fn mcp_tool(name: &str, description: &str, input_schema: serde_json::Value) -> rmcp::model::Tool {
    rmcp::model::Tool {
        name: name.to_string().into(),
        title: None,
        description: Some(description.to_string().into()),
        input_schema: std::sync::Arc::new(rmcp::model::object(input_schema)),
        output_schema: None,
        annotations: None,
        execution: None,
        icons: None,
        meta: None,
    }
}

fn tool_info_from_parts(name: &ToolName, tool: rmcp::model::Tool) -> ToolInfo {
    ToolInfo {
        server_name: server_name_from_tool_name(name),
        supports_parallel_tool_calls: false,
        server_origin: None,
        callable_name: name.name.clone(),
        callable_namespace: name.namespace.clone().unwrap_or_default(),
        namespace_description: None,
        tool,
        connector_id: None,
        connector_name: None,
        plugin_display_names: Vec::new(),
    }
}

fn server_name_from_tool_name(name: &ToolName) -> String {
    name.namespace
        .as_deref()
        .and_then(|namespace| {
            namespace
                .strip_prefix("mcp__")
                .and_then(|suffix| suffix.strip_suffix("__"))
        })
        .unwrap_or_else(|| name.namespace.as_deref().unwrap_or("test_server"))
        .to_string()
}

#[test]
fn code_mode_augments_mcp_tool_descriptions_with_structured_output_sample() {
    let model_info = model_info();
    let mut features = Features::with_defaults();
    features.enable(Feature::CodeMode);
    features.enable(Feature::CodeModeOnly);
    features.enable(Feature::UnifiedExec);
    let available_models = Vec::new();
    let tools_config = ToolsConfig::new(&ToolsConfigParams {
        model_info: &model_info,
        available_models: &available_models,
        features: &features,
        image_generation_tool_auth_allowed: true,
        web_search_mode: Some(WebSearchMode::Cached),
        session_source: SessionSource::Cli,
        permission_profile: &PermissionProfile::Disabled,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
    });

    let mut tool = mcp_tool(
        "echo",
        "Echo text",
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {"type": "string"}
            },
            "required": ["message"],
            "additionalProperties": false
        }),
    );
    tool.output_schema = Some(std::sync::Arc::new(rmcp::model::object(
        serde_json::json!({
            "type": "object",
            "properties": {
                "echo": {"type": "string"},
                "env": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "null"}
                    ]
                }
            },
            "required": ["echo", "env"],
            "additionalProperties": false
        }),
    )));

    let (tools, _) = build_specs(
        &tools_config,
        Some(HashMap::from([(
            ToolName::namespaced("mcp__sample__", "echo"),
            tool,
        )])),
        /*deferred_mcp_tools*/ None,
        &[],
    );

    let ToolSpec::Freeform(FreeformTool { description, .. }) = find_tool(&tools, "exec") else {
        panic!("expected freeform tool");
    };

    assert!(description.contains(
        r#"### `mcp__sample__echo`
Echo text

exec tool declaration:
```ts
declare const tools: { mcp__sample__echo(args: { message: string; }): Promise<CallToolResult<{ echo: string; env: string | null; }>>; };
```"#
    ));
}

fn discoverable_connector(id: &str, name: &str, description: &str) -> DiscoverableTool {
    let slug = name.replace(' ', "-").to_lowercase();
    DiscoverableTool::Connector(Box::new(AppInfo {
        id: id.to_string(),
        name: name.to_string(),
        description: Some(description.to_string()),
        logo_url: None,
        logo_url_dark: None,
        distribution_channel: None,
        branding: None,
        app_metadata: None,
        labels: None,
        install_url: Some(format!("https://chatgpt.com/apps/{slug}/{id}")),
        is_accessible: false,
        is_enabled: true,
        plugin_display_names: Vec::new(),
    }))
}

fn deferred_mcp_tool(
    tool_name: &str,
    tool_namespace: &str,
    server_name: &str,
    connector_name: Option<&str>,
    description: Option<&str>,
) -> ToolInfo {
    ToolInfo {
        server_name: server_name.to_string(),
        supports_parallel_tool_calls: false,
        server_origin: None,
        callable_name: tool_name.to_string(),
        callable_namespace: tool_namespace.to_string(),
        namespace_description: description.map(str::to_string),
        tool: mcp_tool(
            tool_name,
            description.unwrap_or("Deferred MCP tool"),
            json!({}),
        ),
        connector_id: None,
        connector_name: connector_name.map(str::to_string),
        plugin_display_names: Vec::new(),
    }
}

fn assert_contains_tool_names(tools: &[ToolSpec], expected_subset: &[&str]) {
    use std::collections::HashSet;

    let mut names = HashSet::new();
    let mut duplicates = Vec::new();
    for name in tools.iter().map(ToolSpec::name) {
        if !names.insert(name) {
            duplicates.push(name);
        }
    }
    assert!(
        duplicates.is_empty(),
        "duplicate tool entries detected: {duplicates:?}"
    );
    for expected in expected_subset {
        assert!(
            names.contains(expected),
            "expected tool {expected} to be present; had: {names:?}"
        );
    }
}

fn assert_lacks_tool_name(tools: &[ToolSpec], expected_absent: &str) {
    let names = tools.iter().map(ToolSpec::name).collect::<Vec<_>>();
    assert!(
        !names.contains(&expected_absent),
        "expected tool {expected_absent} to be absent; had: {names:?}"
    );
}

fn request_user_input_tool_spec(available_modes: &[ModeKind]) -> ToolSpec {
    create_request_user_input_tool(request_user_input_tool_description(available_modes))
}

fn spawn_agent_tool_options(config: &ToolsConfig) -> SpawnAgentToolOptions {
    SpawnAgentToolOptions {
        available_models: config.available_models.clone(),
        agent_type_description: agent_type_description(config, DEFAULT_AGENT_TYPE_DESCRIPTION),
        hide_agent_type_model_reasoning: config.hide_spawn_agent_metadata,
        include_usage_hint: config.spawn_agent_usage_hint,
        usage_hint_text: config.spawn_agent_usage_hint_text.clone(),
        max_concurrent_threads_per_session: config.max_concurrent_threads_per_session,
    }
}

fn wait_agent_timeout_options() -> WaitAgentTimeoutOptions {
    WaitAgentTimeoutOptions {
        default_timeout_ms: DEFAULT_WAIT_TIMEOUT_MS,
        min_timeout_ms: MIN_WAIT_TIMEOUT_MS,
        max_timeout_ms: MAX_WAIT_TIMEOUT_MS,
    }
}

fn find_tool<'a>(tools: &'a [ToolSpec], expected_name: &str) -> &'a ToolSpec {
    tools
        .iter()
        .find(|tool| tool.name() == expected_name)
        .unwrap_or_else(|| panic!("expected tool {expected_name}"))
}

fn assert_namespace_contains_function(
    tools: &[ToolSpec],
    expected_namespace: &str,
    expected_name: &str,
) {
    let namespace_tool = find_tool(tools, expected_namespace);
    let ToolSpec::Namespace(namespace) = namespace_tool else {
        panic!("expected namespace tool {expected_namespace}");
    };
    assert!(
        namespace.tools.iter().any(|tool| {
            matches!(tool, ResponsesApiNamespaceTool::Function(tool) if tool.name == expected_name)
        }),
        "expected tool {expected_name} in namespace {expected_namespace}"
    );
}

fn assert_process_tool_environment_id(
    tools: &[ToolSpec],
    expected_name: &str,
    expected_present: bool,
) {
    let tool = find_tool(tools, expected_name);
    let ToolSpec::Function(ResponsesApiTool { parameters, .. }) = tool else {
        panic!("expected function tool {expected_name}");
    };
    let (properties, _) = expect_object_schema(parameters);
    assert_eq!(
        properties.contains_key("environment_id"),
        expected_present,
        "{expected_name} environment_id parameter presence"
    );
}

fn assert_apply_patch_environment_id(tools: &[ToolSpec], expected_present: bool) {
    let tool = find_tool(tools, "apply_patch");
    let ToolSpec::Freeform(FreeformTool { format, .. }) = tool else {
        panic!("expected freeform apply_patch tool");
    };
    assert_eq!(
        format.definition.contains("environment_id?"),
        expected_present,
        "apply_patch environment_id grammar presence"
    );
}

fn find_namespace_function_tool<'a>(
    tools: &'a [ToolSpec],
    expected_namespace: &str,
    expected_name: &str,
) -> &'a ResponsesApiTool {
    let namespace_tool = find_tool(tools, expected_namespace);
    let ToolSpec::Namespace(namespace) = namespace_tool else {
        panic!("expected namespace tool {expected_namespace}");
    };
    namespace
        .tools
        .iter()
        .find_map(|tool| match tool {
            ResponsesApiNamespaceTool::Function(tool) if tool.name == expected_name => Some(tool),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected tool {expected_namespace}{expected_name} in namespace"))
}

fn namespace_function_names(tools: &[ToolSpec], expected_namespace: &str) -> Vec<String> {
    let namespace_tool = find_tool(tools, expected_namespace);
    let ToolSpec::Namespace(namespace) = namespace_tool else {
        panic!("expected namespace tool {expected_namespace}");
    };
    namespace
        .tools
        .iter()
        .map(|tool| match tool {
            ResponsesApiNamespaceTool::Function(tool) => tool.name.clone(),
        })
        .collect()
}

fn expect_object_schema(
    schema: &JsonSchema,
) -> (&BTreeMap<String, JsonSchema>, Option<&Vec<String>>) {
    assert_eq!(
        schema.schema_type,
        Some(JsonSchemaType::Single(JsonSchemaPrimitiveType::Object))
    );
    let properties = schema
        .properties
        .as_ref()
        .expect("expected object properties");
    (properties, schema.required.as_ref())
}

fn expect_string_description(schema: &JsonSchema) -> &str {
    assert_eq!(
        schema.schema_type,
        Some(JsonSchemaType::Single(JsonSchemaPrimitiveType::String))
    );
    schema.description.as_deref().expect("expected description")
}

fn strip_descriptions_schema(schema: &mut JsonSchema) {
    if let Some(variants) = &mut schema.any_of {
        for variant in variants {
            strip_descriptions_schema(variant);
        }
    }
    if let Some(items) = &mut schema.items {
        strip_descriptions_schema(items);
    }
    if let Some(properties) = &mut schema.properties {
        for value in properties.values_mut() {
            strip_descriptions_schema(value);
        }
    }
    if let Some(AdditionalProperties::Schema(schema)) = &mut schema.additional_properties {
        strip_descriptions_schema(schema);
    }
    schema.description = None;
}

fn strip_descriptions_tool(spec: &mut ToolSpec) {
    match spec {
        ToolSpec::ToolSearch { parameters, .. } => strip_descriptions_schema(parameters),
        ToolSpec::Function(ResponsesApiTool { parameters, .. }) => {
            strip_descriptions_schema(parameters);
        }
        ToolSpec::Namespace(namespace) => {
            for tool in &mut namespace.tools {
                match tool {
                    ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                        parameters, ..
                    }) => {
                        strip_descriptions_schema(parameters);
                    }
                }
            }
        }
        ToolSpec::Freeform(FreeformTool { .. })
        | ToolSpec::ImageGeneration { .. }
        | ToolSpec::WebSearch { .. } => {}
    }
}
