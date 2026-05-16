//! codex-vl loop_controller: manage_loops dynamic-tool resolver.
//!
//! Surface consumed by the app-server side when an agent calls the
//! `manage_loops` dynamic tool (namespace `codex_app::manage_loops`
//! / flat `manage_loops`). The body of the former
//! `App::execute_manage_loops_dynamic_tool` +
//! `App::resolve_manage_loops_app_server_request` lives here as
//! `pub(super)` free fns. `mod.rs` keeps the `App::resolve_*` facade
//! signature byte-identical and delegates here.
//!
//! `loop_action_outcome_to_app_server_response` is the only consumer
//! of `LoopActionOutcome → AppServerDynamicToolCallResponse`, so it
//! lives here too (private to this module).

use codex_app_server_protocol::DynamicToolCallOutputContentItem as AppServerDynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::DynamicToolCallResponse as AppServerDynamicToolCallResponse;
use codex_app_server_protocol::RequestId;
use codex_protocol::ThreadId;

use crate::app::App;
use crate::app_server_session::AppServerSession;

use super::formatting::loop_action_failure;
use super::jobs;
use super::parsing::is_manage_loops_dynamic_tool;
use super::parsing::parse_manage_loops_tool_request;
use super::types::LoopActionOutcome;
use super::types::LoopCommandSource;

pub(super) fn loop_action_outcome_to_app_server_response(
    outcome: LoopActionOutcome,
) -> AppServerDynamicToolCallResponse {
    AppServerDynamicToolCallResponse {
        content_items: vec![AppServerDynamicToolCallOutputContentItem::InputText {
            text: outcome.payload.to_string(),
        }],
        success: outcome.success,
    }
}

async fn execute_dynamic_tool(
    app: &mut App,
    thread_id: ThreadId,
    arguments: serde_json::Value,
) -> LoopActionOutcome {
    match parse_manage_loops_tool_request(arguments) {
        Ok(request) => {
            match jobs::run_command_request(app, thread_id, request, LoopCommandSource::Agent).await
            {
                Ok(outcome) => outcome,
                Err(err) => loop_action_failure("unknown", thread_id, err.to_string()),
            }
        }
        Err(err) => loop_action_failure(
            "unknown",
            thread_id,
            format!("manage_loops arguments invalid: {err}"),
        ),
    }
}

pub(super) async fn resolve_app_server_request(
    app: &mut App,
    app_server: &AppServerSession,
    request_id: RequestId,
    params: DynamicToolCallParams,
) -> color_eyre::Result<()> {
    let thread_id = ThreadId::from_string(&params.thread_id)?;
    let outcome = if is_manage_loops_dynamic_tool(params.namespace.as_deref(), &params.tool) {
        execute_dynamic_tool(app, thread_id, params.arguments).await
    } else {
        loop_action_failure(
            "unknown",
            thread_id,
            format!(
                "Dynamic tool `{}{}` is not available in TUI yet.",
                params
                    .namespace
                    .as_deref()
                    .map(|namespace| format!("{namespace}::"))
                    .unwrap_or_default(),
                params.tool
            ),
        )
    };
    app_server
        .resolve_server_request(
            request_id,
            serde_json::to_value(loop_action_outcome_to_app_server_response(outcome))?,
        )
        .await?;
    Ok(())
}
