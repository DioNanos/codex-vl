use std::sync::Arc;

use codex_utils_absolute_path::test_support::PathExt;
use tempfile::tempdir;

use super::super::AppRunControl;
use super::super::test_support::make_test_app;
use super::super::tests::make_test_app_with_channels;
use crate::app_event::AppEvent;
use crate::history_cell::HistoryCell;
use crate::vl::VlEvent;
use crate::vl::events::LoopCommandRequest;

#[tokio::test]
async fn loop_state_runtime_reuses_process_state_db_handle() {
    let mut app = make_test_app().await;
    let codex_home = tempdir().expect("temporary Codex home should be created");
    let state_db = codex_state::StateRuntime::init(
        codex_state::SqliteConfig::new_for_testing(codex_home.path().abs()),
        "test-provider".to_string(),
    )
    .await
    .expect("state database should initialize once");
    app.state_db = Some(state_db.clone());

    let first = app
        .loop_state_runtime()
        .await
        .expect("loop controller should receive the process handle");
    let second = app
        .loop_state_runtime()
        .await
        .expect("loop controller should keep reusing the process handle");

    assert!(Arc::ptr_eq(&state_db, &first));
    assert!(Arc::ptr_eq(&first, &second));
}

#[tokio::test]
async fn reload_loop_jobs_state_failure_is_non_fatal() {
    let mut app = make_test_app().await;
    let thread_id = codex_protocol::ThreadId::new();
    let cwd = app.config.cwd.to_path_buf();
    app.chat_widget
        .handle_thread_session(super::super::tests::test_thread_session(thread_id, cwd));
    app.primary_thread_id = Some(thread_id);
    app.state_db = None;

    let control = app
        .handle_vl_event(VlEvent::ReloadLoopJobs { thread_id })
        .await
        .expect("a best-effort loop refresh must not terminate the TUI");

    assert!(matches!(control, AppRunControl::Continue));
}

fn history_cell_text(cell: &Arc<dyn HistoryCell>) -> String {
    cell.display_lines(/*width*/ 120)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn app_for_loop_owner_slash_test() -> anyhow::Result<(
    super::super::App,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    codex_protocol::ThreadId,
    tempfile::TempDir,
)> {
    let (mut app, events, _ops) = make_test_app_with_channels().await;
    let codex_home = tempdir()?;
    let state_db = codex_state::StateRuntime::init(
        codex_state::SqliteConfig::new_for_testing(codex_home.path().abs()),
        "test-provider".to_string(),
    )
    .await?;
    app.state_db = Some(state_db);
    let thread_id = codex_protocol::ThreadId::new();
    app.primary_thread_id = Some(thread_id);
    app.active_thread_id = Some(thread_id);
    app.chat_widget.set_agent_turn_running_for_tests(true);
    Ok((app, events, thread_id, codex_home))
}

#[tokio::test]
async fn loop_owner_brain_off_during_turn_reports_error_and_continues() -> anyhow::Result<()> {
    let (mut app, mut events, thread_id, _codex_home) = app_for_loop_owner_slash_test().await?;
    app.chat_widget
        .prepare_vivling_adult_for_tests()
        .map_err(|err| anyhow::anyhow!(err))?;

    let control = app
        .handle_vl_event(VlEvent::LoopCommand {
            thread_id,
            request: LoopCommandRequest::OwnerSetVivling,
        })
        .await?;

    assert!(matches!(control, AppRunControl::Continue));
    assert!(app.chat_widget.is_agent_turn_running());
    let cell = events
        .try_recv()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let AppEvent::InsertHistoryCell(cell) = cell else {
        anyhow::bail!("expected the Vivling brain error history cell, got {cell:?}");
    };
    let text = history_cell_text(&cell);
    assert!(
        text.contains("Enable the Vivling brain first with `/vivling brain on`"),
        "expected the brain-off guidance, got: {text}"
    );
    assert!(
        !text.contains("turn_aborted"),
        "the slash error must not abort the active turn: {text}"
    );
    assert!(
        events.try_recv().is_err(),
        "the command must not enqueue a turn completion/interruption event"
    );
    Ok(())
}

#[tokio::test]
async fn loop_owner_brain_on_during_turn_remains_a_positive_control() -> anyhow::Result<()> {
    let (mut app, _events, thread_id, _codex_home) = app_for_loop_owner_slash_test().await?;
    app.chat_widget
        .prepare_vivling_management_gate_for_tests()
        .map_err(|err| anyhow::anyhow!(err))?;

    let control = app
        .handle_vl_event(VlEvent::LoopCommand {
            thread_id,
            request: LoopCommandRequest::OwnerSetVivling,
        })
        .await?;

    assert!(matches!(control, AppRunControl::Continue));
    assert!(app.chat_widget.is_agent_turn_running());
    let owner = app
        .state_db
        .as_ref()
        .expect("test state database")
        .get_thread_loop_owner(thread_id)
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    assert_eq!(
        owner.as_ref().map(|owner| owner.owner_kind.as_str()),
        Some(codex_state::THREAD_LOOP_OWNER_KIND_VIVLING)
    );
    Ok(())
}

#[tokio::test]
async fn loop_delegate_brain_off_during_turn_reports_error_and_continues() -> anyhow::Result<()> {
    let (mut app, mut events, thread_id, _codex_home) = app_for_loop_owner_slash_test().await?;
    app.chat_widget
        .prepare_vivling_adult_for_tests()
        .map_err(|err| anyhow::anyhow!(err))?;
    app.state_db
        .as_ref()
        .expect("test state database")
        .create_or_replace_thread_loop_job(codex_state::ThreadLoopJobCreateParams {
            id: "job-delegate-brain-off".to_string(),
            thread_id,
            label: "x".to_string(),
            prompt_text: "check x".to_string(),
            goal_text: Some("check x".to_string()),
            interval_seconds: 60,
            enabled: true,
            run_policy: "queue_one".to_string(),
            auto_remove_on_completion: true,
            created_by: "user".to_string(),
            next_run_ms: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await?;

    let outcome = super::jobs::run_command_request(
        &mut app,
        thread_id,
        LoopCommandRequest::Delegate {
            label: "x".to_string(),
            owner_kind: "vivling".to_string(),
        },
        super::types::LoopCommandSource::User,
    )
    .await?;
    assert!(!outcome.success);
    assert!(
        outcome
            .message
            .contains("Enable the Vivling brain first with `/vivling brain on`")
    );

    let control = app
        .handle_vl_event(VlEvent::LoopCommand {
            thread_id,
            request: LoopCommandRequest::Delegate {
                label: "x".to_string(),
                owner_kind: "vivling".to_string(),
            },
        })
        .await?;

    assert!(matches!(control, AppRunControl::Continue));
    assert!(app.chat_widget.is_agent_turn_running());
    let cell = events
        .try_recv()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let AppEvent::InsertHistoryCell(cell) = cell else {
        anyhow::bail!("expected the Delegate brain error history cell, got {cell:?}");
    };
    let text = history_cell_text(&cell);
    assert!(
        text.contains("Enable the Vivling brain first with `/vivling brain on`"),
        "expected the brain-off guidance, got: {text}"
    );
    assert!(
        !text.contains("turn_aborted"),
        "the Delegate precondition error must not abort the active turn: {text}"
    );
    assert!(
        app.state_db
            .as_ref()
            .expect("test state database")
            .get_loop_delegation(thread_id, "job-delegate-brain-off")
            .await?
            .is_none(),
        "brain-off Delegate must not persist a delegation"
    );
    Ok(())
}
