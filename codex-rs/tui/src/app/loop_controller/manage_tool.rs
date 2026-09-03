//! codex-vl loop_controller: manage_loops dynamic-tool resolver.
//!
//! Surface consumed by the app-server side when an agent calls the
//! `manage_loops` dynamic tool (namespace `codex_app::manage_loops`
//! / flat `manage_loops`). The body of the former
//! `App::resolve_manage_loops_app_server_request` lives here as the
//! single `pub(super)` free fn `resolve_app_server_request`; the
//! former `App::execute_manage_loops_dynamic_tool` body migrated
//! alongside it as the private `execute_dynamic_tool` helper since
//! `resolve_app_server_request` is its only caller. `mod.rs` keeps
//! the `App::resolve_*` facade signature byte-identical and
//! delegates here.
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
use super::types::ManagedToolCallSource;

fn loop_action_outcome_to_app_server_response(
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
            // FIX-G — three explicit cases for the agent tool call: no
            // active scope is an ordinary agent call (pre-T5 behaviour),
            // one scope binds the T5 allowlist, two or more are ambiguous
            // and fail closed. Never collapse these again.
            let source = match app.resolve_tool_call_source(thread_id) {
                ManagedToolCallSource::OrdinaryAgent => LoopCommandSource::Agent,
                ManagedToolCallSource::Single(source) => source,
                ManagedToolCallSource::Ambiguous => {
                    return loop_action_failure(
                        "scope",
                        thread_id,
                        "manage_loops is ambiguous with more than one active server-issued loop tick scope."
                            .to_string(),
                    );
                }
            };
            match jobs::run_command_request(app, thread_id, request, source).await {
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

#[cfg(test)]
mod tests {
    use codex_app_server_protocol::DynamicToolCallOutputContentItem as AppServerDynamicToolCallOutputContentItem;
    use codex_protocol::ThreadId;

    use super::super::formatting::loop_action_success;
    use super::super::formatting::sample_job;
    use super::loop_action_outcome_to_app_server_response;

    // FIX-G regression tests -------------------------------------------------
    // The T5 regression: with `managed_loop_command_source` returning a bare
    // `Option`, «no active scope» was rejected like «ambiguous» and the
    // ordinary agent could no longer create loops. These tests pin the three
    // explicit cases plus the fail-closed tick-completion resolver.
    mod fix_g {
        use crate::app::tests::make_test_app_with_channels;
        use codex_state::SqliteConfig;
        use codex_state::StateRuntime;
        use codex_utils_absolute_path::test_support::PathExt;
        use tempfile::tempdir;

        use super::super::execute_dynamic_tool;
        use crate::app::App;
        use codex_protocol::ThreadId;

        const ADD_ARGS: &str =
            r#"{"action":"add","label":"__LABEL__","interval":"5m","prompt":"check"}"#;

        async fn app_with_state() -> anyhow::Result<(App, std::sync::Arc<StateRuntime>, ThreadId)> {
            let (mut app, _events, _ops) = make_test_app_with_channels().await;
            let codex_home = tempdir()?;
            let state_runtime = StateRuntime::init(
                SqliteConfig::new_for_testing(codex_home.path().abs()),
                "test-provider".to_string(),
            )
            .await?;
            app.state_db = Some(state_runtime.clone());
            let thread_id = ThreadId::new();
            app.primary_thread_id = Some(thread_id);
            app.active_thread_id = Some(thread_id);
            Ok((app, state_runtime, thread_id))
        }

        fn add_args(label: &str) -> serde_json::Value {
            serde_json::from_str(ADD_ARGS.replace("__LABEL__", label).as_str())
                .expect("add arguments parse")
        }

        async fn create_unscoped_job(
            app: &mut App,
            thread_id: ThreadId,
            label: &str,
        ) -> anyhow::Result<()> {
            let outcome = execute_dynamic_tool(app, thread_id, add_args(label)).await;
            assert!(
                outcome.success,
                "unscoped add must succeed: {}",
                outcome.message
            );
            Ok(())
        }

        // (a) The seam-control case: with NO active scope the ordinary agent
        // call goes through and the job is really created.
        #[tokio::test]
        async fn add_without_scope_creates_the_job() -> anyhow::Result<()> {
            let (mut app, state_runtime, thread_id) = app_with_state().await?;

            let outcome = execute_dynamic_tool(&mut app, thread_id, add_args("unscoped")).await;
            assert!(
                outcome.success,
                "unscoped agent add must be an ordinary call: {}",
                outcome.message
            );
            let created = state_runtime
                .get_thread_loop_job_by_label(thread_id, "unscoped")
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?;
            assert!(created.is_some(), "the job must exist after the add");
            Ok(())
        }

        // (b) With exactly one scope the T5 allowlist governs: allowed
        // updates go through, disallowed adds are refused by scope.
        #[tokio::test]
        async fn single_scope_applies_the_allowlist() -> anyhow::Result<()> {
            let (mut app, state_runtime, thread_id) = app_with_state().await?;
            create_unscoped_job(&mut app, thread_id, "scoped").await?;
            let job = state_runtime
                .get_thread_loop_job_by_label(thread_id, "scoped")
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?
                .expect("scoped job exists");
            app.register_managed_loop_scope(thread_id, &job.id, "scoped");

            // Allowlisted update: prompt enrichment goes through (an interval
            // change would additionally require the T5 churn gate).
            let update = execute_dynamic_tool(
                &mut app,
                thread_id,
                serde_json::json!({"action":"update","label":"scoped","prompt":"enriched"}),
            )
            .await;
            assert!(
                update.success,
                "allowlisted update must succeed: {}",
                update.message
            );

            // Outside the allowlist: a brand-new add is refused by scope.
            let refused = execute_dynamic_tool(&mut app, thread_id, add_args("other")).await;
            assert!(
                !refused.success,
                "managed-scope add must be refused by the T5 allowlist"
            );
            let other = state_runtime
                .get_thread_loop_job_by_label(thread_id, "other")
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?;
            assert!(other.is_none(), "the refused add must not create a job");
            Ok(())
        }

        // (c) Two or more scopes: caller identity is ambiguous — fail closed,
        // no job is created.
        #[tokio::test]
        async fn two_scopes_fail_closed() -> anyhow::Result<()> {
            let (mut app, state_runtime, thread_id) = app_with_state().await?;
            create_unscoped_job(&mut app, thread_id, "one").await?;
            create_unscoped_job(&mut app, thread_id, "two").await?;
            let one = state_runtime
                .get_thread_loop_job_by_label(thread_id, "one")
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?
                .expect("job one exists");
            let two = state_runtime
                .get_thread_loop_job_by_label(thread_id, "two")
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?
                .expect("job two exists");
            app.register_managed_loop_scope(thread_id, &one.id, "one");
            app.register_managed_loop_scope(thread_id, &two.id, "two");

            let attempt = execute_dynamic_tool(&mut app, thread_id, add_args("three")).await;
            assert!(
                !attempt.success,
                "an ambiguous caller identity must fail closed"
            );
            assert!(
                attempt.message.contains("ambiguous"),
                "the failure must name the ambiguity: {}",
                attempt.message
            );
            let three = state_runtime
                .get_thread_loop_job_by_label(thread_id, "three")
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?;
            assert!(three.is_none(), "the refused add must not create a job");
            Ok(())
        }

        // (d) The tick-completion resolver is bound to the exact
        // (thread_id, job_id) scope: a different or missing job never
        // resolves, so the completion path records `audit_rejected` instead
        // of executing with full agent permissions.
        #[tokio::test]
        async fn managed_tick_source_is_bound_to_the_exact_job() -> anyhow::Result<()> {
            let (mut app, state_runtime, thread_id) = app_with_state().await?;
            create_unscoped_job(&mut app, thread_id, "owner").await?;
            create_unscoped_job(&mut app, thread_id, "bystander").await?;
            let owner = state_runtime
                .get_thread_loop_job_by_label(thread_id, "owner")
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?
                .expect("owner job exists");
            let bystander = state_runtime
                .get_thread_loop_job_by_label(thread_id, "bystander")
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?
                .expect("bystander job exists");
            app.register_managed_loop_scope(thread_id, &owner.id, "owner");

            assert!(
                app.resolve_managed_tick_source(thread_id, &owner.id)
                    .is_some(),
                "the exact tick scope must resolve to Managed"
            );
            assert!(
                app.resolve_managed_tick_source(thread_id, &bystander.id)
                    .is_none(),
                "a scope of another job must never resolve (audit_rejected path)"
            );
            assert!(
                app.resolve_managed_tick_source(thread_id, "missing")
                    .is_none(),
                "a missing job must never resolve"
            );
            Ok(())
        }
    }

    #[test]
    fn app_server_response_uses_json_payload() {
        let response = loop_action_outcome_to_app_server_response(loop_action_success(
            "show",
            ThreadId::new(),
            "ok".to_string(),
            Some(&sample_job()),
            None,
        ));

        let [AppServerDynamicToolCallOutputContentItem::InputText { text }] =
            response.content_items.as_slice()
        else {
            panic!("expected text payload");
        };
        let parsed: serde_json::Value =
            serde_json::from_str(text).expect("tool response should be JSON");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["action"], "show");
        assert_eq!(parsed["job"]["label"], "forge");
    }
}
