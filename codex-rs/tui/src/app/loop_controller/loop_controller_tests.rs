use std::sync::Arc;

use tempfile::tempdir;

use super::super::AppRunControl;
use super::super::test_support::make_test_app;
use crate::vl::VlEvent;

#[tokio::test]
async fn loop_state_runtime_reuses_process_state_db_handle() {
    let mut app = make_test_app().await;
    let codex_home = tempdir().expect("temporary Codex home should be created");
    let state_db = codex_state::StateRuntime::init(
        codex_home.path().to_path_buf(),
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
