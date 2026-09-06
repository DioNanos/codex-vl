use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::ConnectionRequestId;
use codex_app_server_protocol::IdentityBinding;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadGoal;
use codex_app_server_protocol::ThreadHistoryBuilder;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadSettings;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnError;
use codex_core::CodexThread;
use codex_core::ThreadConfigSnapshot;
use codex_file_watcher::WatchRegistration;
use codex_protocol::ThreadId;
#[cfg(test)]
use codex_protocol::config_types::MultiAgentMode;
use codex_protocol::items::AgentMessageContent as CoreAgentMessageContent;
use codex_protocol::items::TurnItem as CoreTurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::protocol::EventMsg;
use codex_rollout::RolloutItem;
use codex_rollout::state_db::StateDbHandle;
use codex_utils_path_uri::LegacyAppPathString;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::Weak;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tracing::error;

type PendingInterruptQueue = Vec<ConnectionRequestId>;

pub(crate) struct PendingThreadResumeRequest {
    pub(crate) request_id: ConnectionRequestId,
    pub(crate) history_items: Vec<RolloutItem>,
    /// Usage attribution already resolved while cold-loading a paginated child.
    pub(crate) cold_resume_token_usage_turn_id: Option<String>,
    pub(crate) config_snapshot: ThreadConfigSnapshot,
    pub(crate) instruction_sources: Vec<LegacyAppPathString>,
    pub(crate) thread_summary: codex_app_server_protocol::Thread,
    pub(crate) emit_thread_goal_update: bool,
    pub(crate) thread_goal_state_db: Option<StateDbHandle>,
    pub(crate) include_turns: bool,
    pub(crate) initial_turns_page:
        Option<codex_app_server_protocol::ThreadResumeInitialTurnsPageParams>,
    pub(crate) paginated_turns: Option<Vec<Turn>>,
    pub(crate) paginated_initial_turns_page: Option<codex_app_server_protocol::TurnsPage>,
    pub(crate) paginated_initial_turns_page_with_active_slot:
        Option<codex_app_server_protocol::TurnsPage>,
    pub(crate) resume_cursor_store: Option<Arc<dyn codex_thread_store::ThreadStore>>,
    pub(crate) redact_resume_payloads: bool,
}

// ThreadListenerCommand is used to perform operations in the context of the thread listener, for serialization purposes.
pub(crate) enum ThreadListenerCommand {
    // SendThreadResumeResponse is used to resume an already running thread by sending the thread's history to the client and atomically subscribing for new updates.
    SendThreadResumeResponse(Box<PendingThreadResumeRequest>),
    // EmitThreadGoalUpdated is used to order goal updates with running-thread resume responses and goal clears.
    EmitThreadGoalUpdated {
        turn_id: Option<String>,
        goal: ThreadGoal,
    },
    // EmitThreadQueueChanged orders durable queue updates with thread notifications.
    EmitThreadQueueChanged,
    // EmitWarning is used to order extension warnings with other thread notifications.
    EmitWarning {
        message: String,
    },
    // EmitThreadGoalCleared is used to order app-server goal clears with running-thread resume responses.
    EmitThreadGoalCleared,
    // EmitThreadGoalSnapshot is used to read and emit the latest goal state in the listener order.
    EmitThreadGoalSnapshot {
        state_db: StateDbHandle,
    },
    // ResolveServerRequest is used to notify the client that the request has been resolved.
    // It is executed in the thread listener's context to ensure that the resolved notification is ordered with regard to the request itself.
    ResolveServerRequest {
        request_id: RequestId,
        completion_tx: oneshot::Sender<()>,
    },
}

/// Per-conversation accumulation of the latest states e.g. error message while a turn runs.
#[derive(Default, Clone)]
pub(crate) struct TurnSummary {
    pub(crate) started_at: Option<i64>,
    pub(crate) command_execution_started: HashSet<String>,
    pub(crate) last_error: Option<TurnError>,
    pub(crate) last_agent_message: Option<ThreadItem>,
}

#[derive(Default)]
pub(crate) struct ThreadState {
    pub(crate) pending_interrupts: PendingInterruptQueue,
    pub(crate) pending_rollbacks: Option<ConnectionRequestId>,
    pub(crate) turn_summary: TurnSummary,
    pub(crate) last_terminal_turn_id: Option<String>,
    /// Lets an internal runtime replacement wait until the old listener has processed Core's
    /// `ShutdownComplete` event before that listener is superseded.
    shutdown_drain_waiter: Option<oneshot::Sender<()>>,
    pub(crate) cancel_tx: Option<oneshot::Sender<()>>,
    pub(crate) experimental_raw_events: bool,
    pub(crate) listener_generation: u64,
    last_thread_settings: Option<ThreadSettings>,
    listener_command_tx: Option<mpsc::UnboundedSender<ThreadListenerCommand>>,
    current_turn_history: ThreadHistoryBuilder,
    listener_thread: Option<Weak<CodexThread>>,
    watch_registration: WatchRegistration,
}

impl ThreadState {
    pub(crate) fn listener_matches(&self, conversation: &Arc<CodexThread>) -> bool {
        self.listener_thread
            .as_ref()
            .and_then(Weak::upgrade)
            .is_some_and(|existing| Arc::ptr_eq(&existing, conversation))
    }

    pub(crate) fn set_listener(
        &mut self,
        cancel_tx: oneshot::Sender<()>,
        conversation: &Arc<CodexThread>,
        watch_registration: WatchRegistration,
        thread_settings_baseline: ThreadSettings,
    ) -> (mpsc::UnboundedReceiver<ThreadListenerCommand>, u64) {
        if let Some(previous) = self.cancel_tx.replace(cancel_tx) {
            let _ = previous.send(());
        }
        self.listener_generation = self.listener_generation.wrapping_add(1);
        self.last_thread_settings = Some(thread_settings_baseline);
        let (listener_command_tx, listener_command_rx) = mpsc::unbounded_channel();
        self.listener_command_tx = Some(listener_command_tx);
        self.listener_thread = Some(Arc::downgrade(conversation));
        self.watch_registration = watch_registration;
        (listener_command_rx, self.listener_generation)
    }

    pub(crate) fn clear_listener(&mut self) {
        if let Some(cancel_tx) = self.cancel_tx.take() {
            let _ = cancel_tx.send(());
        }
        self.shutdown_drain_waiter = None;
        self.listener_command_tx = None;
        self.current_turn_history.reset();
        self.listener_thread = None;
        self.watch_registration = WatchRegistration::default();
    }

    pub(crate) fn set_experimental_raw_events(&mut self, enabled: bool) {
        self.experimental_raw_events = enabled;
    }

    pub(crate) fn listener_command_tx(
        &self,
    ) -> Option<mpsc::UnboundedSender<ThreadListenerCommand>> {
        self.listener_command_tx.clone()
    }

    pub(crate) fn active_turn_snapshot(&self) -> Option<Turn> {
        self.current_turn_history.active_turn_snapshot()
    }

    pub(crate) fn register_shutdown_drain_waiter(&mut self) -> oneshot::Receiver<()> {
        let (completion_tx, completion_rx) = oneshot::channel();
        self.shutdown_drain_waiter = Some(completion_tx);
        completion_rx
    }

    pub(crate) fn take_shutdown_drain_waiter(&mut self) -> Option<oneshot::Sender<()>> {
        self.shutdown_drain_waiter.take()
    }

    pub(crate) fn track_current_turn_event(&mut self, event_turn_id: &str, event: &EventMsg) {
        if let EventMsg::TurnStarted(payload) = event {
            self.turn_summary.started_at = payload.started_at;
        }
        if let EventMsg::ItemCompleted(payload) = event
            && let CoreTurnItem::AgentMessage(item) = &payload.item
            && matches!(item.phase, Some(MessagePhase::FinalAnswer) | None)
            && item.content.iter().any(|content| {
                matches!(content, CoreAgentMessageContent::Text { text } if !text.trim().is_empty())
            })
        {
            self.turn_summary.last_agent_message =
                Some(ThreadItem::from(CoreTurnItem::AgentMessage(item.clone())));
        }
        self.current_turn_history.handle_event(event);
        if matches!(event, EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_)) {
            self.last_terminal_turn_id = Some(event_turn_id.to_string());
            if !self.current_turn_history.has_active_turn() {
                self.current_turn_history.reset();
            }
        }
    }

    pub(crate) fn note_thread_settings(&mut self, thread_settings: ThreadSettings) -> bool {
        let changed = self.last_thread_settings.as_ref() != Some(&thread_settings);
        self.last_thread_settings = Some(thread_settings);
        changed
    }
}

pub(crate) async fn resolve_server_request_on_thread_listener(
    thread_state: &Arc<Mutex<ThreadState>>,
    request_id: RequestId,
) {
    let (completion_tx, completion_rx) = oneshot::channel();
    let listener_command_tx = {
        let state = thread_state.lock().await;
        state.listener_command_tx()
    };
    let Some(listener_command_tx) = listener_command_tx else {
        error!("failed to remove pending client request: thread listener is not running");
        return;
    };

    if listener_command_tx
        .send(ThreadListenerCommand::ResolveServerRequest {
            request_id,
            completion_tx,
        })
        .is_err()
    {
        error!(
            "failed to remove pending client request: thread listener command channel is closed"
        );
        return;
    }

    if let Err(err) = completion_rx.await {
        error!("failed to remove pending client request: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_server_protocol::ApprovalsReviewer;
    use codex_app_server_protocol::AskForApproval;
    use codex_app_server_protocol::SandboxPolicy;
    use codex_protocol::config_types::CollaborationMode;
    use codex_protocol::config_types::ModeKind;
    use codex_protocol::config_types::Settings;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;

    #[test]
    fn note_thread_settings_reports_only_effective_changes() {
        let mut state = ThreadState::default();
        let initial = thread_settings("mock-model");
        let updated = thread_settings("mock-model-2");

        let results = vec![
            state.note_thread_settings(initial.clone()),
            state.note_thread_settings(initial),
            state.note_thread_settings(updated.clone()),
            state.note_thread_settings(updated),
        ];

        assert_eq!(results, vec![true, false, true, false]);
    }

    #[tokio::test]
    async fn unreadable_persisted_bindings_fail_closed() {
        let home = tempfile::tempdir().expect("create persistence home");
        std::fs::write(
            home.path().join("thread-identity-bindings.json"),
            b"not-json",
        )
        .expect("write corrupt persistence sidecar");
        let manager = ThreadStateManager::with_persistence(home.path());
        let thread_id =
            ThreadId::from_string("01a076e5-35d2-7b80-af68-d5778b6591a1").expect("thread id");

        assert_eq!(
            manager
                .authorize_thread_access(thread_id, ConnectionId(1))
                .await,
            Err("thread identity binding persistence unavailable")
        );
    }

    #[test]
    fn failed_binding_persistence_returns_error() {
        let home = tempfile::tempdir().expect("create persistence home");
        let parent_file = home.path().join("not-a-directory");
        std::fs::write(&parent_file, b"file").expect("write parent file");
        let sidecar = parent_file.join("thread-identity-bindings.json");

        assert_eq!(
            persist_bindings(Some(&sidecar), &HashMap::new()),
            Err("failed to persist thread identity bindings")
        );
    }

    fn thread_settings(model: &str) -> ThreadSettings {
        ThreadSettings {
            cwd: AbsolutePathBuf::from_absolute_path("/tmp").expect("absolute path"),
            approval_policy: AskForApproval::OnRequest,
            approvals_reviewer: ApprovalsReviewer::User,
            sandbox_policy: SandboxPolicy::ReadOnly {
                network_access: false,
            },
            active_permission_profile: None,
            model: model.to_string(),
            model_provider: "mock_provider".to_string(),
            service_tier: None,
            effort: None,
            summary: None,
            collaboration_mode: CollaborationMode {
                mode: ModeKind::Default,
                settings: Settings {
                    model: model.to_string(),
                    reasoning_effort: None,
                    developer_instructions: None,
                },
            },
            multi_agent_mode: MultiAgentMode::ExplicitRequestOnly,
            personality: None,
        }
    }
}

struct ThreadEntry {
    state: Arc<Mutex<ThreadState>>,
    connection_ids: HashSet<ConnectionId>,
    has_connections_watcher: watch::Sender<bool>,
    owner_binding: Option<ThreadBinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistedThreadBinding {
    owner_instance_id: String,
    cell_id: String,
    tmux_session: String,
    incarnation_id: String,
}

impl From<&ThreadBinding> for PersistedThreadBinding {
    fn from(binding: &ThreadBinding) -> Self {
        Self {
            owner_instance_id: binding.owner_instance_id.clone(),
            cell_id: binding.cell_id.clone(),
            tmux_session: binding.tmux_session.clone(),
            incarnation_id: binding.incarnation_id.clone(),
        }
    }
}

impl PersistedThreadBinding {
    fn same_owner(&self, other: &ThreadBinding) -> bool {
        self.owner_instance_id == other.owner_instance_id
            && self.cell_id == other.cell_id
            && self.tmux_session == other.tmux_session
    }

    fn into_runtime(self) -> ThreadBinding {
        ThreadBinding {
            owner_instance_id: self.owner_instance_id,
            cell_id: self.cell_id,
            tmux_session: self.tmux_session,
            incarnation_id: self.incarnation_id,
            connection_id: ConnectionId(0),
            binding_id: String::new(),
        }
    }
}

impl Default for ThreadEntry {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(ThreadState::default())),
            connection_ids: HashSet::new(),
            has_connections_watcher: watch::channel(false).0,
            owner_binding: None,
        }
    }
}

impl ThreadEntry {
    fn update_has_connections(&self) {
        let _ = self.has_connections_watcher.send_if_modified(|current| {
            let prev = *current;
            *current = !self.connection_ids.is_empty();
            prev != *current
        });
    }
}

#[derive(Default)]
struct ThreadStateManagerInner {
    live_connections: HashMap<ConnectionId, ConnectionCapabilities>,
    threads: HashMap<ThreadId, ThreadEntry>,
    thread_ids_by_connection: HashMap<ConnectionId, HashSet<ThreadId>>,
    persisted_bindings: HashMap<ThreadId, PersistedThreadBinding>,
    persistence_error: Option<Arc<str>>,
}

#[derive(Clone, Default)]
pub(crate) struct ConnectionCapabilities {
    pub(crate) request_attestation: bool,
    pub(crate) identity_binding: Option<IdentityBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ThreadBinding {
    pub(crate) owner_instance_id: String,
    pub(crate) cell_id: String,
    pub(crate) tmux_session: String,
    pub(crate) incarnation_id: String,
    pub(crate) connection_id: ConnectionId,
    pub(crate) binding_id: String,
}

impl ThreadBinding {
    fn from_connection(connection_id: ConnectionId, binding: &IdentityBinding) -> Self {
        Self {
            owner_instance_id: binding.claims.owner_instance_id.clone(),
            cell_id: binding.claims.cell_id.clone(),
            tmux_session: binding.claims.tmux_session.clone(),
            incarnation_id: binding.claims.incarnation_id.clone(),
            connection_id,
            binding_id: binding.binding_id.clone(),
        }
    }

    fn same_owner(&self, other: &Self) -> bool {
        self.owner_instance_id == other.owner_instance_id
            && self.cell_id == other.cell_id
            && self.tmux_session == other.tmux_session
    }
}

#[derive(Clone, Default)]
pub(crate) struct ThreadStateManager {
    state: Arc<Mutex<ThreadStateManagerInner>>,
    binding_store_path: Option<PathBuf>,
    // Extension event sinks are synchronous, so they need an await-free way to
    // enqueue work on the active per-thread listener.
    listener_commands:
        Arc<StdMutex<HashMap<ThreadId, mpsc::UnboundedSender<ThreadListenerCommand>>>>,
}

impl ThreadStateManager {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_persistence(codex_home: &Path) -> Self {
        let binding_store_path = codex_home.join("thread-identity-bindings.json");
        let (persisted_bindings, persistence_error) =
            match load_persisted_bindings(&binding_store_path) {
                Ok(bindings) => (bindings, None),
                Err(error) => (HashMap::new(), Some(Arc::<str>::from(error))),
            };
        Self {
            state: Arc::new(Mutex::new(ThreadStateManagerInner {
                persisted_bindings,
                persistence_error,
                ..Default::default()
            })),
            binding_store_path: Some(binding_store_path),
            listener_commands: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    pub(crate) async fn connection_initialized(
        &self,
        connection_id: ConnectionId,
        capabilities: ConnectionCapabilities,
    ) {
        self.state
            .lock()
            .await
            .live_connections
            .insert(connection_id, capabilities);
    }

    pub(crate) async fn connection_identity_bound(
        &self,
        connection_id: ConnectionId,
        binding: IdentityBinding,
    ) {
        if let Some(capabilities) = self
            .state
            .lock()
            .await
            .live_connections
            .get_mut(&connection_id)
        {
            capabilities.identity_binding = Some(binding);
        }
    }

    async fn connection_thread_binding(
        &self,
        connection_id: ConnectionId,
    ) -> Option<ThreadBinding> {
        let state = self.state.lock().await;
        state
            .live_connections
            .get(&connection_id)
            .and_then(|capabilities| capabilities.identity_binding.as_ref())
            .map(|binding| ThreadBinding::from_connection(connection_id, binding))
    }

    pub(crate) async fn connection_identity_binding(
        &self,
        connection_id: ConnectionId,
    ) -> Option<IdentityBinding> {
        let state = self.state.lock().await;
        state
            .live_connections
            .get(&connection_id)
            .and_then(|capabilities| capabilities.identity_binding.clone())
    }

    pub(crate) async fn authorize_thread_access(
        &self,
        thread_id: ThreadId,
        connection_id: ConnectionId,
    ) -> Result<(), &'static str> {
        let requester = self.connection_thread_binding(connection_id).await;
        let mut state = self.state.lock().await;
        if state.persistence_error.is_some() {
            return Err("thread identity binding persistence unavailable");
        }
        let thread_has_owner = state
            .threads
            .get(&thread_id)
            .and_then(|entry| entry.owner_binding.as_ref())
            .is_some()
            || state.persisted_bindings.contains_key(&thread_id);
        if !thread_has_owner {
            return Ok(());
        }
        let Some(requester) = requester else {
            return Err("identity required for bound thread");
        };
        let owner_matches = state
            .threads
            .get(&thread_id)
            .and_then(|entry| entry.owner_binding.as_ref())
            .is_some_and(|owner| owner.same_owner(&requester))
            || state
                .persisted_bindings
                .get(&thread_id)
                .is_some_and(|owner| owner.same_owner(&requester));
        if owner_matches {
            let mut persisted_bindings = state.persisted_bindings.clone();
            if let Some(entry) = state.threads.get(&thread_id)
                && entry
                    .owner_binding
                    .as_ref()
                    .is_some_and(|owner| owner.same_owner(&requester))
            {
                if let Some(entry) = state.threads.get_mut(&thread_id) {
                    entry.owner_binding = Some(requester.clone());
                }
            }
            if let Some(owner) = persisted_bindings.get(&thread_id)
                && owner.same_owner(&requester)
            {
                persisted_bindings.insert(thread_id, PersistedThreadBinding::from(&requester));
            }
            persist_bindings(self.binding_store_path.as_deref(), &persisted_bindings)?;
            state.persisted_bindings = persisted_bindings;
            Ok(())
        } else {
            Err("thread identity binding owner mismatch")
        }
    }

    pub(crate) async fn bind_new_thread(
        &self,
        thread_id: ThreadId,
        connection_id: ConnectionId,
    ) -> Result<(), &'static str> {
        let Some(binding) = self.connection_thread_binding(connection_id).await else {
            return Ok(());
        };
        let mut state = self.state.lock().await;
        if state.persistence_error.is_some() {
            return Err("thread identity binding persistence unavailable");
        }
        let persisted_binding = PersistedThreadBinding::from(&binding);
        let mut persisted_bindings = state.persisted_bindings.clone();
        persisted_bindings.insert(thread_id, persisted_binding);
        persist_bindings(self.binding_store_path.as_deref(), &persisted_bindings)?;
        let entry = state.threads.entry(thread_id).or_default();
        entry.owner_binding = Some(binding);
        state.persisted_bindings = persisted_bindings;
        Ok(())
    }

    pub(crate) async fn bind_forked_thread(
        &self,
        source_thread_id: ThreadId,
        forked_thread_id: ThreadId,
        connection_id: ConnectionId,
    ) -> Result<(), &'static str> {
        self.authorize_thread_access(source_thread_id, connection_id)
            .await?;
        let owner_binding = {
            let state = self.state.lock().await;
            state
                .threads
                .get(&source_thread_id)
                .and_then(|entry| entry.owner_binding.clone())
                .or_else(|| {
                    state
                        .persisted_bindings
                        .get(&source_thread_id)
                        .cloned()
                        .map(PersistedThreadBinding::into_runtime)
                })
        };
        if let Some(owner_binding) = owner_binding {
            let mut state = self.state.lock().await;
            if state.persistence_error.is_some() {
                return Err("thread identity binding persistence unavailable");
            }
            let mut persisted_bindings = state.persisted_bindings.clone();
            persisted_bindings.insert(
                forked_thread_id,
                PersistedThreadBinding::from(&owner_binding),
            );
            persist_bindings(self.binding_store_path.as_deref(), &persisted_bindings)?;
            state
                .threads
                .entry(forked_thread_id)
                .or_default()
                .owner_binding = Some(owner_binding.clone());
            state.persisted_bindings = persisted_bindings;
        } else {
            self.bind_new_thread(forked_thread_id, connection_id)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn first_attestation_capable_connection_for_thread(
        &self,
        thread_id: ThreadId,
    ) -> Option<ConnectionId> {
        let state = self.state.lock().await;
        state
            .threads
            .get(&thread_id)?
            .connection_ids
            .iter()
            .filter_map(|connection_id| {
                state
                    .live_connections
                    .get(connection_id)?
                    .request_attestation
                    .then_some(*connection_id)
            })
            .min_by_key(|connection_id| connection_id.0)
    }

    pub(crate) async fn wait_for_thread_subscriber(&self, thread_id: ThreadId) {
        let mut has_connections = {
            let mut state = self.state.lock().await;
            state
                .threads
                .entry(thread_id)
                .or_default()
                .has_connections_watcher
                .subscribe()
        };
        while !*has_connections.borrow_and_update() {
            if has_connections.changed().await.is_err() {
                break;
            }
        }
    }

    pub(crate) async fn subscribed_connection_ids(&self, thread_id: ThreadId) -> Vec<ConnectionId> {
        let state = self.state.lock().await;
        state
            .threads
            .get(&thread_id)
            .map(|thread_entry| thread_entry.connection_ids.iter().copied().collect())
            .unwrap_or_default()
    }

    pub(crate) async fn thread_state(&self, thread_id: ThreadId) -> Arc<Mutex<ThreadState>> {
        let mut state = self.state.lock().await;
        state.threads.entry(thread_id).or_default().state.clone()
    }

    pub(crate) fn current_listener_command_tx(
        &self,
        thread_id: ThreadId,
    ) -> Option<mpsc::UnboundedSender<ThreadListenerCommand>> {
        self.listener_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&thread_id)
            .cloned()
    }

    pub(crate) fn register_listener_command_tx(
        &self,
        thread_id: ThreadId,
        tx: mpsc::UnboundedSender<ThreadListenerCommand>,
    ) {
        self.listener_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(thread_id, tx);
    }

    pub(crate) fn unregister_listener_command_tx(&self, thread_id: ThreadId) {
        self.listener_commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&thread_id);
    }

    pub(crate) async fn remove_thread_state(&self, thread_id: ThreadId) {
        let thread_state = {
            let mut state = self.state.lock().await;
            let thread_state = state
                .threads
                .remove(&thread_id)
                .map(|thread_entry| thread_entry.state);
            state.thread_ids_by_connection.retain(|_, thread_ids| {
                thread_ids.remove(&thread_id);
                !thread_ids.is_empty()
            });
            thread_state
        };
        self.unregister_listener_command_tx(thread_id);

        if let Some(thread_state) = thread_state {
            let mut thread_state = thread_state.lock().await;
            tracing::debug!(
                thread_id = %thread_id,
                listener_generation = thread_state.listener_generation,
                had_listener = thread_state.cancel_tx.is_some(),
                had_active_turn = thread_state.active_turn_snapshot().is_some(),
                "clearing thread listener during thread-state teardown"
            );
            thread_state.clear_listener();
        }
    }

    pub(crate) async fn clear_all_listeners(&self) {
        let thread_states = {
            let state = self.state.lock().await;
            state
                .threads
                .iter()
                .map(|(thread_id, thread_entry)| (*thread_id, thread_entry.state.clone()))
                .collect::<Vec<_>>()
        };

        for (thread_id, thread_state) in thread_states {
            self.unregister_listener_command_tx(thread_id);
            let mut thread_state = thread_state.lock().await;
            tracing::debug!(
                thread_id = %thread_id,
                listener_generation = thread_state.listener_generation,
                had_listener = thread_state.cancel_tx.is_some(),
                had_active_turn = thread_state.active_turn_snapshot().is_some(),
                "clearing thread listener during app-server shutdown"
            );
            thread_state.clear_listener();
        }
    }

    pub(crate) async fn unsubscribe_connection_from_thread(
        &self,
        thread_id: ThreadId,
        connection_id: ConnectionId,
    ) -> bool {
        {
            let mut state = self.state.lock().await;
            if !state.threads.contains_key(&thread_id) {
                return false;
            }

            if !state
                .thread_ids_by_connection
                .get(&connection_id)
                .is_some_and(|thread_ids| thread_ids.contains(&thread_id))
            {
                return false;
            }

            if let Some(thread_ids) = state.thread_ids_by_connection.get_mut(&connection_id) {
                thread_ids.remove(&thread_id);
                if thread_ids.is_empty() {
                    state.thread_ids_by_connection.remove(&connection_id);
                }
            }
            if let Some(thread_entry) = state.threads.get_mut(&thread_id) {
                thread_entry.connection_ids.remove(&connection_id);
                thread_entry.update_has_connections();
            }
        };

        true
    }

    #[cfg(test)]
    pub(crate) async fn has_subscribers(&self, thread_id: ThreadId) -> bool {
        self.state
            .lock()
            .await
            .threads
            .get(&thread_id)
            .is_some_and(|thread_entry| !thread_entry.connection_ids.is_empty())
    }

    pub(crate) async fn try_ensure_connection_subscribed(
        &self,
        thread_id: ThreadId,
        connection_id: ConnectionId,
        experimental_raw_events: bool,
    ) -> Option<Arc<Mutex<ThreadState>>> {
        let thread_state = {
            let mut state = self.state.lock().await;
            if !state.live_connections.contains_key(&connection_id) {
                return None;
            }
            state
                .thread_ids_by_connection
                .entry(connection_id)
                .or_default()
                .insert(thread_id);
            let thread_entry = state.threads.entry(thread_id).or_default();
            thread_entry.connection_ids.insert(connection_id);
            thread_entry.update_has_connections();
            thread_entry.state.clone()
        };
        {
            let mut thread_state_guard = thread_state.lock().await;
            if experimental_raw_events {
                thread_state_guard.set_experimental_raw_events(/*enabled*/ true);
            }
        }
        Some(thread_state)
    }

    pub(crate) async fn try_add_connection_to_thread(
        &self,
        thread_id: ThreadId,
        connection_id: ConnectionId,
    ) -> bool {
        let mut state = self.state.lock().await;
        if !state.live_connections.contains_key(&connection_id) {
            return false;
        }
        state
            .thread_ids_by_connection
            .entry(connection_id)
            .or_default()
            .insert(thread_id);
        let thread_entry = state.threads.entry(thread_id).or_default();
        thread_entry.connection_ids.insert(connection_id);
        thread_entry.update_has_connections();
        true
    }

    pub(crate) async fn remove_connection(&self, connection_id: ConnectionId) -> Vec<ThreadId> {
        {
            let mut state = self.state.lock().await;
            state.live_connections.remove(&connection_id);
            let thread_ids = state
                .thread_ids_by_connection
                .remove(&connection_id)
                .unwrap_or_default();
            for thread_id in &thread_ids {
                if let Some(thread_entry) = state.threads.get_mut(thread_id) {
                    thread_entry.connection_ids.remove(&connection_id);
                    thread_entry.update_has_connections();
                }
            }
            thread_ids
                .into_iter()
                .filter(|thread_id| {
                    state
                        .threads
                        .get(thread_id)
                        .is_some_and(|thread_entry| thread_entry.connection_ids.is_empty())
                })
                .collect::<Vec<_>>()
        }
    }

    pub(crate) async fn subscribe_to_has_connections(
        &self,
        thread_id: ThreadId,
    ) -> Option<watch::Receiver<bool>> {
        let state = self.state.lock().await;
        state
            .threads
            .get(&thread_id)
            .map(|thread_entry| thread_entry.has_connections_watcher.subscribe())
    }
}

fn load_persisted_bindings(
    path: &Path,
) -> Result<HashMap<ThreadId, PersistedThreadBinding>, String> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => {
            return Err(format!(
                "failed to read thread identity bindings at {}: {error}",
                path.display()
            ));
        }
    };
    let raw = serde_json::from_str::<HashMap<String, PersistedThreadBinding>>(&contents).map_err(
        |error| {
            format!(
                "failed to decode thread identity bindings at {}: {error}",
                path.display()
            )
        },
    )?;
    raw.into_iter()
        .map(|(thread_id, binding)| {
            ThreadId::from_string(&thread_id)
                .map(|thread_id| (thread_id, binding))
                .map_err(|error| {
                    format!(
                        "invalid thread id in identity bindings at {}: {error}",
                        path.display()
                    )
                })
        })
        .collect()
}

fn persist_bindings(
    path: Option<&Path>,
    bindings: &HashMap<ThreadId, PersistedThreadBinding>,
) -> Result<(), &'static str> {
    let Some(path) = path else { return Ok(()) };
    let raw = bindings
        .iter()
        .map(|(thread_id, binding)| (thread_id.to_string(), binding))
        .collect::<HashMap<_, _>>();
    let contents = serde_json::to_vec_pretty(&raw).map_err(|error| {
        tracing::error!(path = %path.display(), %error, "failed to encode thread identity bindings");
        "failed to persist thread identity bindings"
    })?;
    let temporary_path = path.with_extension("json.tmp");
    std::fs::write(&temporary_path, contents)
        .and_then(|()| std::fs::rename(&temporary_path, path))
        .map_err(|error| {
            tracing::error!(path = %path.display(), %error, "failed to persist thread identity bindings");
            "failed to persist thread identity bindings"
        })
}
