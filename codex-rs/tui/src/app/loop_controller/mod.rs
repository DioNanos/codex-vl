//! codex-vl loop_controller (iter B1..B7 split): facade module.
//!
//! The custom Vivling/loop runtime is decomposed into focused
//! sub-modules to reduce blast radius on upstream merges. This file
//! keeps only the `impl App` public surface (`pub(super)` methods)
//! plus two small private helpers (`record_vivling_loop_job`,
//! `record_vivling_loop_runtime`) that the sub-modules call back
//! into via Rust's default child-sub-module visibility. Most
//! `pub(super)` methods delegate one-line to a sibling sub-module;
//! `loop_state_runtime` is the small boundary that exposes the
//! process-owned state handle without reopening SQLite. The 8 focused
//! unit tests that used to live here have been moved next to the code
//! they cover (`parsing`, `formatting`, `manage_tool`); cross-boundary
//! state-handle tests remain attached to this facade.
//!
//! Sub-modules now isolated:
//! - `types` — `LoopActionOutcome`, `LoopCommandSource`.
//! - `parsing` — input parsers + `ManageLoopsToolArgs` +
//!   `is_manage_loops_dynamic_tool`.
//! - `formatting` — `LOOP_STATUS_*` constants + narrating helpers +
//!   JSON payload builders.
//! - `state` — `loop_now_ms`, `loop_state_error`.
//! - `events` — `refresh_jobs`, `handle_reload`.
//! - `jobs` — `run_command_request` (Add/Update/List/Show/Enable/
//!   Disable/Remove/Trigger/Owner CRUD dispatcher).
//! - `ticks` — `handle_tick` (former `App::handle_loop_tick` body) +
//!   `process_submission` (former `App::process_loop_submission`,
//!   includes Vivling owner-kind branch + main-path
//!   `submit_loop_prompt`) + the `loop_submission_status` helper
//!   (private, only ticks consumes it).
//! - `vivling_delegation` — `handle_loop_tick_finished` (Vivling brain
//!   reply consumer with follow-up `LoopCommandRequest`),
//!   `tick_action_request` (internal helper), and the tokio spawn
//!   helpers `run_assist` / `run_loop_tick` that call into
//!   `crate::app::vivling_background::*`.
//! - `manage_tool` — `resolve_app_server_request` (former
//!   `App::resolve_manage_loops_app_server_request` body) +
//!   `execute_dynamic_tool` + `loop_action_outcome_to_app_server_response`
//!   (both private, used only inside this sub-module).

mod formatting;
mod parsing;
mod state;
mod types;

mod events;
mod jobs;
mod manage_tool;
mod notify;
mod rearm;
pub(crate) mod summary;
mod ticks;
mod vivling_delegation;

#[cfg(test)]
#[path = "loop_controller_tests.rs"]
mod tests;

use super::*;
use crate::vivling::VivlingLoopEventKind;
use crate::vivling::VivlingLoopEventSource;
use crate::vl::events::LoopCommandRequest;

use self::formatting::canonical_last_status;
use self::formatting::loop_runtime_state;
use self::types::LoopCommandScope;
use self::types::LoopCommandSource;
use self::types::ManagedToolCallSource;

// codex-vl: re-exported for the app-server event router, which must route
// `manage_loops` dynamic tool calls to this module before the upstream
// DynamicToolCall handling.
pub(super) use self::parsing::is_manage_loops_dynamic_tool;

/// bootstrap hook for `startup_orchestration`: starts the bounded
/// queue and the separate consumer task; the replay of undelivered pending
/// rows runs inside it, once per process start.
pub(crate) fn start_loop_summary_worker(state_db: &std::sync::Arc<codex_state::StateRuntime>) {
    notify::start_worker(state_db);
}

impl App {
    fn record_vivling_loop_job(
        &mut self,
        action: &str,
        label: &str,
        job: Option<&codex_state::ThreadLoopJob>,
        source: LoopCommandSource,
    ) {
        let runtime_state = job.map(loop_runtime_state);
        let last_status =
            job.and_then(|job| canonical_last_status(job).as_deref().map(str::to_string));
        let goal = job.and_then(|job| job.goal_text.as_deref());
        self.chat_widget.record_vivling_loop_event(
            VivlingLoopEventKind::Config,
            match &source {
                LoopCommandSource::User => VivlingLoopEventSource::User,
                LoopCommandSource::Agent | LoopCommandSource::Managed(_) => {
                    VivlingLoopEventSource::Agent
                }
            },
            action,
            label,
            runtime_state,
            last_status.as_deref(),
            goal,
        );
    }

    fn record_vivling_loop_runtime(
        &mut self,
        label: &str,
        runtime_state: Option<&str>,
        last_status: Option<&str>,
        goal: Option<&str>,
        created_by: &str,
    ) {
        let source = if created_by == "user" {
            VivlingLoopEventSource::User
        } else {
            VivlingLoopEventSource::Agent
        };
        self.chat_widget.record_vivling_loop_event(
            VivlingLoopEventKind::Runtime,
            source,
            "run",
            label,
            runtime_state,
            last_status,
            goal,
        );
    }

    pub(super) async fn loop_state_runtime(
        &self,
    ) -> color_eyre::Result<std::sync::Arc<codex_state::StateRuntime>> {
        self.state_db.clone().ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "loop jobs are unavailable because the process state database is not initialized"
            )
        })
    }

    pub(super) fn register_managed_loop_scope(
        &mut self,
        thread_id: ThreadId,
        job_id: &str,
        label: &str,
    ) -> LoopCommandScope {
        self.managed_loop_scopes
            .retain(|scope| scope.thread_id != thread_id || scope.job_id != job_id);
        let scope = LoopCommandScope {
            instance_id: uuid::Uuid::new_v4().to_string(),
            thread_id,
            job_id: job_id.to_string(),
            label: label.to_string(),
        };
        self.managed_loop_scopes.push(scope.clone());
        tracing::debug!(
            target: "codex_vl::loop_management",
            instance_id = %scope.instance_id,
            job_id,
            "issued managed loop command scope"
        );
        scope
    }

    pub(super) fn clear_managed_loop_scope(&mut self, thread_id: ThreadId, job_id: &str) {
        self.managed_loop_scopes
            .retain(|scope| scope.thread_id != thread_id || scope.job_id != job_id);
    }

    /// resolver for the agent `manage_loops` DynamicToolCall ONLY
    /// (`manage_tool.rs`). The managed-tick completion path has its own,
    /// stricter resolver (`resolve_managed_tick_source`) that can never
    /// yield `Agent`.
    pub(super) fn resolve_tool_call_source(&self, thread_id: ThreadId) -> ManagedToolCallSource {
        let mut scopes = self
            .managed_loop_scopes
            .iter()
            .filter(|scope| scope.thread_id == thread_id);
        let Some(scope) = scopes.next() else {
            // No active scope: an ordinary agent call (pre-governance behaviour).
            return ManagedToolCallSource::OrdinaryAgent;
        };
        if scopes.next().is_some() {
            // More than one child tick on a thread has no unambiguous caller
            // identity in DynamicToolCallParams. Reject it rather than guessing.
            return ManagedToolCallSource::Ambiguous;
        }
        ManagedToolCallSource::Single(LoopCommandSource::Managed(scope.clone()))
    }

    /// resolver for the managed-tick completion path ONLY
    /// (`vivling_delegation.rs`): fail-closed and bound to the exact
    /// (thread_id, job_id) scope of the finishing tick. Never yields
    /// `Agent`: the caller executes the structured tick action on `Some`
    /// or records `audit_rejected` on `None`.
    pub(super) fn resolve_managed_tick_source(
        &self,
        thread_id: ThreadId,
        job_id: &str,
    ) -> Option<LoopCommandSource> {
        self.managed_loop_scopes
            .iter()
            .find(|scope| scope.thread_id == thread_id && scope.job_id == job_id)
            .map(|scope| LoopCommandSource::Managed(scope.clone()))
    }

    pub(super) async fn refresh_loop_jobs(
        &mut self,
        thread_id: ThreadId,
    ) -> color_eyre::Result<()> {
        events::refresh_jobs(self, thread_id).await
    }

    pub(super) async fn apply_loop_command_request(
        &mut self,
        thread_id: ThreadId,
        request: LoopCommandRequest,
        source: bool,
        emit_ui_feedback: bool,
    ) -> color_eyre::Result<()> {
        let source = if source {
            LoopCommandSource::Agent
        } else {
            LoopCommandSource::User
        };
        let outcome = jobs::run_command_request(self, thread_id, request, source).await?;
        if emit_ui_feedback {
            if outcome.success {
                self.chat_widget
                    .add_info_message(outcome.message, /*hint*/ None);
            } else {
                self.chat_widget.add_error_message(outcome.message);
            }
        }
        Ok(())
    }

    pub(super) async fn handle_reload_loop_jobs(
        &mut self,
        thread_id: ThreadId,
    ) -> color_eyre::Result<()> {
        events::handle_reload(self, thread_id).await
    }

    pub(super) async fn handle_loop_tick(
        &mut self,
        thread_id: ThreadId,
        job_id: String,
    ) -> color_eyre::Result<()> {
        ticks::handle_tick(self, thread_id, job_id).await
    }

    pub(super) async fn handle_vivling_loop_tick_finished(
        &mut self,
        thread_id: ThreadId,
        job_id: String,
        occurrence_ms: Option<i64>,
        started_ms: i64,
        result: Result<crate::vivling::VivlingLoopTickResult, String>,
        resolution: crate::vl::delegated_loops::EffectiveLoopOwner,
    ) -> color_eyre::Result<()> {
        vivling_delegation::handle_loop_tick_finished(
            self,
            thread_id,
            job_id,
            occurrence_ms,
            started_ms,
            result,
            resolution,
        )
        .await
    }

    pub(super) async fn resolve_manage_loops_app_server_request(
        &mut self,
        app_server: &AppServerSession,
        request_id: codex_app_server_protocol::RequestId,
        params: codex_app_server_protocol::DynamicToolCallParams,
    ) -> color_eyre::Result<()> {
        manage_tool::resolve_app_server_request(self, app_server, request_id, params).await
    }

    /// codex-vl: dispatch a Vivling brain assist request.
    pub(super) fn run_vivling_assist(
        &mut self,
        thread_id: ThreadId,
        request: crate::vivling::VivlingAssistRequest,
    ) {
        vivling_delegation::run_assist(self, thread_id, request);
    }

    /// codex-vl: dispatch a Vivling-managed loop tick.
    pub(super) fn run_vivling_loop_tick(
        &mut self,
        thread_id: ThreadId,
        job_id: String,
        occurrence_ms: Option<i64>,
        started_ms: i64,
        request: crate::vivling::VivlingLoopTickRequest,
        runner_model: Option<String>,
        resolution: crate::vl::delegated_loops::EffectiveLoopOwner,
    ) {
        vivling_delegation::run_loop_tick(
            self,
            thread_id,
            job_id,
            occurrence_ms,
            started_ms,
            request,
            runner_model,
            resolution,
        );
    }

    /// Memory V2 Step 12.B.D.2 — dispatch a Vivling Expression LLM
    /// request. Spawns the background runner and forwards the result
    /// via [`VlEvent::VivlingExpressionFinished`]. Reservation +
    /// `save_state` must already have happened on the main thread.
    pub(super) fn run_vivling_expression(
        &mut self,
        request: crate::vivling::VivlingExpressionRequest,
    ) {
        vivling_delegation::run_expression(self, request);
    }
}
