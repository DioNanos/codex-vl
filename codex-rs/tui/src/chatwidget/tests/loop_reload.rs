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
        turn_id: chat.turn_lifecycle.last_turn_id.clone(),
        reason: codex_protocol::protocol::TurnAbortReason::Interrupted,
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

/// (4) Watchdog diagnostico del latch: con soglia zero un pending-start
/// stantio fa scattare il warn una sola volta (debounce), e lo stato di
/// avviso viene ripulito quando il latch torna giù. Nessun auto-clear: il
/// watchdog è solo diagnosi, il latch lo sblocca solo il core.
#[tokio::test]
async fn a_stale_pending_start_warns_in_pre_draw_tick() {
    let (mut chat, _app_event_rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;

    chat.input_queue.user_turn_pending_start = true;
    chat.input_queue.user_turn_pending_since =
        Some(std::time::Instant::now() - std::time::Duration::from_secs(60));

    chat.check_pending_start_watchdog_with(std::time::Duration::from_secs(1));
    assert!(
        chat.input_queue.pending_start_warned,
        "un pending-start stantio deve far scattare l'avviso"
    );

    // Debounce: il secondo tick non ri-arma l'avviso.
    chat.pre_draw_tick();
    assert!(chat.input_queue.pending_start_warned);

    // Il latch scende: lo stato di avviso viene ripulito.
    chat.input_queue.user_turn_pending_start = false;
    chat.input_queue.user_turn_pending_since = None;
    chat.input_queue.pending_start_warned = false;
    chat.check_pending_start_watchdog_with(std::time::Duration::from_secs(1));
    assert!(!chat.input_queue.pending_start_warned);
}
