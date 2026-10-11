use super::*;
use crate::app_command::AppCommand as Op;
use crate::vl::VlEvent;

/// Drain the event channel and report whether a loop-job reload was requested.
fn drain_reload(events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> bool {
    let mut reloaded = false;
    while let Ok(event) = events.try_recv() {
        if let AppEvent::Vl(VlEvent::ReloadLoopJobs { .. }) = event {
            reloaded = true;
        }
    }
    reloaded
}

fn aborted_interrupt_event(chat: &ChatWidget) -> codex_protocol::protocol::TurnAbortedEvent {
    codex_protocol::protocol::TurnAbortedEvent {
        root_turn_id: None,
        turn_id: chat.turn_lifecycle.last_turn_id.clone(),
        reason: codex_protocol::protocol::TurnAbortReason::Interrupted,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

/// (i) An interrupted turn never reaches `on_task_complete`, which used to be
/// the only `ReloadLoopJobs` emitter: a loop tick that landed in
/// `pending_busy` during the turn then stayed unscheduled with no live timer.
/// The interrupt boundary must carry the same wakeup, with the same guards.
#[tokio::test]
async fn an_interrupted_turn_reloads_pending_loop_jobs() {
    let (mut chat, mut app_event_rx, _op_rx) =
        make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());

    chat.handle_turn_aborted_event(aborted_interrupt_event(&chat));

    assert!(
        drain_reload(&mut app_event_rx),
        "an interrupted turn must re-arm pending loop ticks"
    );
}

/// (i) The side-conversation guard copied from `on_task_complete`: a reload is
/// a main-thread wakeup and must not fire from side-conversation teardown.
#[tokio::test]
async fn an_interrupted_turn_does_not_reload_for_side_conversations() {
    let (mut chat, mut app_event_rx, _op_rx) =
        make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.active_side_conversation = true;

    chat.handle_turn_aborted_event(aborted_interrupt_event(&chat));

    assert!(
        !drain_reload(&mut app_event_rx),
        "side conversations must not trigger the main-thread loop reload"
    );
}

/// (ii) With a multi-message queue the reload is DEFERRED, not lost: each
/// `TurnComplete` that opens the next queued turn skips it, and the last one
/// — with the queue finally empty — carries it.
#[tokio::test]
async fn a_multi_message_queue_defers_loop_reload_to_the_last_turn() {
    let (mut chat, mut app_event_rx, mut op_rx) =
        make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());

    chat.submit_user_message(UserMessage::from("first"));
    chat.on_task_started();
    // Scarto l'op del primo submit: conto solo gli ops del drain.
    while op_rx.try_recv().is_ok() {}
    chat.queue_user_message(UserMessage::from("queued-1"));
    chat.queue_user_message(UserMessage::from("queued-2"));

    // First completion: the drain opens the next queued turn, so the loop
    // reload is intentionally skipped (a reload here would hit BlockedUserTurn).
    chat.on_task_complete(
        /*last_agent_message*/ None, /*duration_ms*/ None, /*from_replay*/ false,
    );
    let mut submitted = 0;
    while let Ok(op) = op_rx.try_recv() {
        if let Op::UserTurn { .. } = op {
            submitted += 1;
        }
    }
    assert_eq!(submitted, 1, "the drain must open exactly one queued turn");
    assert!(
        !drain_reload(&mut app_event_rx),
        "the reload must be deferred while queued turns are still opening"
    );

    // Second completion: queued-2 opens, so the reload is deferred once more.
    chat.on_task_complete(
        /*last_agent_message*/ None, /*duration_ms*/ None, /*from_replay*/ false,
    );
    let mut submitted = 0;
    while let Ok(op) = op_rx.try_recv() {
        if let Op::UserTurn { .. } = op {
            submitted += 1;
        }
    }
    assert_eq!(submitted, 1, "the second queued turn opens");
    assert!(
        !drain_reload(&mut app_event_rx),
        "still deferred: one queued turn remains"
    );

    // Third completion: the queue is empty, so this boundary carries the reload.
    chat.on_task_complete(
        /*last_agent_message*/ None, /*duration_ms*/ None, /*from_replay*/ false,
    );
    assert!(
        drain_reload(&mut app_event_rx),
        "the last turn completion must carry the loop reload"
    );
}

/// (4) Pending-start watchdog below the auto-clear timeout: past the warn
/// threshold the warning fires once (debounce) and the footer hint appears,
/// but the latch itself stays high until the core lowers it on the normal
/// path, and the warned state resets with it.
#[tokio::test]
async fn a_stale_pending_start_warns_in_pre_draw_tick() {
    let (mut chat, _app_event_rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;

    chat.input_queue.user_turn_pending_start = true;
    // 45 s: past the warn threshold, still below the 60 s auto-clear timeout.
    chat.input_queue.user_turn_pending_since =
        Some(std::time::Instant::now() - std::time::Duration::from_secs(45));

    chat.check_pending_start_watchdog_with(std::time::Duration::from_secs(30));
    assert!(
        chat.input_queue.pending_start_warned,
        "un pending-start stantio deve far scattare l'avviso"
    );
    assert!(
        chat.input_queue.user_turn_pending_start,
        "below the auto-clear timeout the watchdog only diagnoses, it does not unlock"
    );

    // Debounce: the second tick does not re-arm the warning.
    chat.pre_draw_tick();
    assert!(chat.input_queue.pending_start_warned);
    assert!(chat.input_queue.user_turn_pending_start);

    // The latch goes down: the warned state is reset.
    chat.input_queue.user_turn_pending_start = false;
    chat.input_queue.user_turn_pending_since = None;
    chat.input_queue.pending_start_warned = false;
    chat.check_pending_start_watchdog_with(std::time::Duration::from_secs(30));
    assert!(!chat.input_queue.pending_start_warned);
}

/// (4) Auto-clear of the orphan latch: past the auto-clear timeout with no
/// `TaskStarted`, the TUI clears the latch by itself, warns once on the same
/// target, and lets the queue flow. The message that raised the latch is not
/// re-sent: the drain dispatches exactly the NEXT queued message, once.
#[tokio::test]
async fn a_stale_pending_start_auto_clears_and_dispatches_one_queued_message() {
    let (mut chat, _app_event_rx, mut op_rx) =
        make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());

    // Orphan latch: the message that raised it is already dispatched, two
    // more are queued behind it.
    chat.input_queue.user_turn_pending_start = true;
    chat.queue_user_message(UserMessage::from("queued-1"));
    chat.queue_user_message(UserMessage::from("queued-2"));
    chat.input_queue.user_turn_pending_since =
        Some(std::time::Instant::now() - std::time::Duration::from_secs(70));

    chat.check_pending_start_watchdog_with(std::time::Duration::from_secs(30));

    assert!(
        chat.input_queue
            .user_turn_pending_since
            .is_some_and(|since| since.elapsed() < std::time::Duration::from_secs(10)),
        "the auto-clear ran: the drain re-armed a fresh pending start for the next message"
    );
    let mut submitted = 0;
    while let Ok(op) = op_rx.try_recv() {
        if let Op::UserTurn { .. } = op {
            submitted += 1;
        }
    }
    assert_eq!(
        submitted, 1,
        "the auto-clear must dispatch exactly one queued message, never the pending one"
    );
    assert_eq!(
        chat.input_queue.queued_user_messages.len(),
        1,
        "only the first queued message left the queue"
    );

    // The re-armed pending start is fresh: a second tick must not dispatch
    // anything else inside the same episode.
    chat.check_pending_start_watchdog_with(std::time::Duration::from_secs(30));
    let mut resubmitted = 0;
    while let Ok(op) = op_rx.try_recv() {
        if let Op::UserTurn { .. } = op {
            resubmitted += 1;
        }
    }
    assert_eq!(resubmitted, 0, "no double-send inside the same episode");
}

/// (4) Never auto-clear while a turn is actually running: once the core
/// reported the turn start the normal path owns the latch, the watchdog must
/// not touch it, and nothing may be dispatched behind the running turn.
#[tokio::test]
async fn an_auto_clear_never_fires_while_a_turn_is_running() {
    let (mut chat, _app_event_rx, mut op_rx) =
        make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());

    chat.submit_user_message(UserMessage::from("first"));
    chat.on_task_started();
    while op_rx.try_recv().is_ok() {}

    // Pathological overlap kept for the guard's sake: a stale pending start
    // recorded while the turn is demonstrably running.
    chat.input_queue.user_turn_pending_start = true;
    chat.input_queue.user_turn_pending_since =
        Some(std::time::Instant::now() - std::time::Duration::from_secs(70));
    chat.queue_user_message(UserMessage::from("queued-1"));

    chat.check_pending_start_watchdog_with(std::time::Duration::from_secs(30));

    assert!(
        chat.input_queue.user_turn_pending_start,
        "a running turn is the core's business: the watchdog must not clear the latch"
    );
    assert_eq!(
        chat.input_queue.queued_user_messages.len(),
        1,
        "nothing may be dispatched while the turn is running"
    );
    let mut submitted = 0;
    while let Ok(op) = op_rx.try_recv() {
        if let Op::UserTurn { .. } = op {
            submitted += 1;
        }
    }
    assert_eq!(submitted, 0);
}

/// (4) A late `TaskStarted` after an auto-clear is harmless: the ack of the
/// original turn lands on the normal latch reset, a second ack for the
/// drained turn is absorbed, and neither re-sends anything.
#[tokio::test]
async fn a_late_task_started_after_an_auto_clear_is_harmless() {
    let (mut chat, _app_event_rx, mut op_rx) =
        make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());

    chat.input_queue.user_turn_pending_start = true;
    chat.queue_user_message(UserMessage::from("queued-1"));
    chat.input_queue.user_turn_pending_since =
        Some(std::time::Instant::now() - std::time::Duration::from_secs(70));

    chat.check_pending_start_watchdog_with(std::time::Duration::from_secs(30));

    // The auto-clear dispatched queued-1 and re-armed a fresh pending start.
    let mut submitted = 0;
    while let Ok(op) = op_rx.try_recv() {
        if let Op::UserTurn { .. } = op {
            submitted += 1;
        }
    }
    assert_eq!(submitted, 1);

    // The core finally acknowledges the original turn...
    chat.on_task_started();
    assert!(
        chat.turn_lifecycle.agent_turn_running,
        "the late ack must start the turn on the normal path"
    );
    assert!(!chat.input_queue.user_turn_pending_start);

    // ...and the drained turn's own start arrives too: both acks are
    // absorbed without re-sending anything.
    chat.on_task_started();
    assert!(chat.turn_lifecycle.agent_turn_running);
    assert!(!chat.input_queue.user_turn_pending_start);
    assert!(
        chat.input_queue.queued_user_messages.is_empty(),
        "the queue was already consumed by the drain"
    );
    let mut resubmitted = 0;
    while let Ok(op) = op_rx.try_recv() {
        if let Op::UserTurn { .. } = op {
            resubmitted += 1;
        }
    }
    assert_eq!(resubmitted, 0, "a late ack must never re-send input");
}
