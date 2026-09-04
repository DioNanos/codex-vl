use super::*;

#[tokio::test]
async fn turn_aborted_without_turn_complete_reconciles_running_state_and_queue() {
    let (mut chat, mut app_event_rx, mut op_rx) = make_chatwidget_manual(None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.on_task_started();
    chat.input_queue
        .queued_user_messages
        .push_back(UserMessage::from("queued after abort").into());
    chat.input_queue
        .queued_user_message_history_records
        .push_back(UserMessageHistoryRecord::UserMessageText);

    chat.handle_turn_aborted_event(codex_protocol::protocol::TurnAbortedEvent {
        turn_id: chat.turn_lifecycle.last_turn_id.clone(),
        reason: codex_protocol::protocol::TurnAbortReason::Interrupted,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    });

    assert!(!chat.turn_lifecycle.agent_turn_running);
    assert!(!chat.bottom_pane.is_task_running());
    assert!(chat.input_queue.queued_user_messages.is_empty());
    assert!(chat.input_queue.pending_steers.is_empty());
    assert_no_submit_op(&mut op_rx);
    assert!(
        !app_event_rx
            .try_iter()
            .any(|event| matches!(event, AppEvent::Vl(_)))
    );
}
