use std::sync::Arc;

use codex_extension_api::ExtensionEventSink;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadGoalClearedEvent;
use codex_protocol::protocol::ThreadGoal;
use codex_protocol::protocol::ThreadGoalUpdatedEvent;

#[derive(Clone)]
pub(crate) struct GoalEventEmitter {
    sink: Arc<dyn ExtensionEventSink>,
}

impl GoalEventEmitter {
    pub(crate) fn new(sink: Arc<dyn ExtensionEventSink>) -> Self {
        Self { sink }
    }

    pub(crate) fn thread_goal_updated(
        &self,
        event_id: impl Into<String>,
        turn_id: Option<String>,
        goal: ThreadGoal,
    ) {
        self.sink.emit(Event {
            id: event_id.into(),
            msg: EventMsg::ThreadGoalUpdated(ThreadGoalUpdatedEvent {
                thread_id: goal.thread_id,
                turn_id,
                goal,
            }),
        });
    }

    /// codex-vl: completing a goal clears it from the thread (fork
    /// semantics, ported from core `goals.rs` after upstream moved the
    /// goal runtime into this extension). TUI/app-server listeners for
    /// `ThreadGoalCleared` predate the move and stay unchanged.
    pub(crate) fn thread_goal_cleared(
        &self,
        event_id: impl Into<String>,
        thread_id: codex_protocol::ThreadId,
        turn_id: Option<String>,
    ) {
        self.sink.emit(Event {
            id: event_id.into(),
            msg: EventMsg::ThreadGoalCleared(ThreadGoalClearedEvent {
                thread_id,
                turn_id,
            }),
        });
    }
}
