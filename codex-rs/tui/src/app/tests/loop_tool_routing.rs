//! codex-vl: routing of `manage_loops` dynamic tool calls in the TUI
//! app-server event handler.
//!
//! Mutation-sensitive: the fork branch in
//! `App::handle_server_request_event` must stay BEFORE the upstream
//! `DynamicToolCall` handling, whose embedded gate rejects every call
//! with "TUI dynamic tools require an active external task". These
//! tests fail if the branch order is restored to the post-merge state
//! or if the fork routing is dropped.

use super::*;

use codex_app_server_protocol::DynamicToolCallOutputContentItem;
use codex_app_server_protocol::DynamicToolCallParams;
use codex_app_server_protocol::DynamicToolCallResponse;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use serde_json::json;

fn manage_loops_add_request(request_id: AppServerRequestId, thread_id: &ThreadId) -> ServerRequest {
    ServerRequest::DynamicToolCall {
        request_id,
        params: DynamicToolCallParams {
            thread_id: thread_id.to_string(),
            turn_id: ThreadId::new().to_string(),
            call_id: "ml-add-1".to_string(),
            namespace: Some("codex_app".to_string()),
            tool: "manage_loops".to_string(),
            arguments: json!({
                "action": "add",
                "label": "gate-fix-probe",
                "interval": "1h",
                "prompt": "noop",
            }),
        },
    }
}

fn non_manage_loops_request(
    request_id: AppServerRequestId,
    namespace: Option<&str>,
    tool: &str,
) -> ServerRequest {
    ServerRequest::DynamicToolCall {
        request_id,
        params: DynamicToolCallParams {
            thread_id: ThreadId::new().to_string(),
            turn_id: ThreadId::new().to_string(),
            call_id: format!("route-{tool}"),
            namespace: namespace.map(str::to_string),
            tool: tool.to_string(),
            arguments: json!({}),
        },
    }
}

fn failure_text(response: &DynamicToolCallResponse) -> String {
    response
        .content_items
        .iter()
        .filter_map(|item| match item {
            DynamicToolCallOutputContentItem::InputText { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn manage_loops_dynamic_tool_is_served_by_the_loop_controller_in_embedded_mode() -> Result<()>
{
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let app_server =
        crate::start_embedded_app_server_for_picker(app.chat_widget.config_ref()).await?;

    // Loop persistence needs the process state database handle.
    let codex_home = tempdir().expect("temporary Codex home should be created");
    let state_db = codex_state::StateRuntime::init(
        SqliteConfig::new_for_testing(codex_home.path().abs()),
        "test-provider".to_string(),
    )
    .await
    .expect("state database should initialize");
    app.state_db = Some(state_db);

    let thread_id = ThreadId::new();
    app.primary_thread_id = Some(thread_id);
    app.active_thread_id = Some(thread_id);

    app.handle_server_request_event(
        &app_server,
        manage_loops_add_request(AppServerRequestId::Integer(4242), &thread_id),
    )
    .await;

    // Mutation-sensitive: if the upstream DynamicToolCall branch runs before
    // the fork branch, the embedded gate rejects the call with a failure
    // event instead of routing it to the loop controller.
    assert!(
        events.try_recv().is_err(),
        "the embedded gate rejected manage_loops: the fork routing is no longer \
         before the upstream DynamicToolCall handling"
    );

    // The loop controller executed the command (the job is persisted).
    let runtime = app.loop_state_runtime().await?;
    let jobs = runtime
        .list_thread_loop_jobs(thread_id)
        .await
        .expect("loop jobs should be listable");
    assert!(
        jobs.iter().any(|job| job.label == "gate-fix-probe"),
        "manage_loops did not reach the loop controller: jobs = {jobs:?}"
    );
    Ok(())
}

#[tokio::test]
async fn upstream_gate_still_rejects_non_manage_loops_dynamic_tools_in_embedded_mode() -> Result<()>
{
    let (mut app, mut events, _ops) = make_test_app_with_channels().await;
    let app_server =
        crate::start_embedded_app_server_for_picker(app.chat_widget.config_ref()).await?;

    // Upstream delegation tool (fork_thread): the upstream gate must stay
    // intact (approval-gated MCP server message via the delegation path).
    app.handle_server_request_event(
        &app_server,
        non_manage_loops_request(
            AppServerRequestId::Integer(4243),
            Some("codex_tui"),
            "fork_thread",
        ),
    )
    .await;
    assert_matches!(
        events.try_recv(),
        Ok(AppEvent::DynamicToolCallCompleted { response, .. })
            if !response.success
                && failure_text(&response).contains("approval-gated MCP server")
    );

    // Generic non-manage_loops tool: the embedded "active external task"
    // rejection must stay intact.
    app.handle_server_request_event(
        &app_server,
        non_manage_loops_request(
            AppServerRequestId::Integer(4244),
            Some("codex_app"),
            "some_other_tool",
        ),
    )
    .await;
    assert_matches!(
        events.try_recv(),
        Ok(AppEvent::DynamicToolCallCompleted { response, .. })
            if !response.success
                && failure_text(&response).contains("require an active external task")
    );
    Ok(())
}
