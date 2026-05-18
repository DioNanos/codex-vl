//! codex-vl loop_controller (iter B1..B7 split): facade module.
//!
//! The custom Vivling/loop runtime is decomposed into focused
//! sub-modules to reduce blast radius on upstream merges. This file
//! keeps only the `impl App` public surface (`pub(super)` methods)
//! plus two small private helpers (`record_vivling_loop_job`,
//! `record_vivling_loop_runtime`) that the sub-modules call back
//! into via Rust's default child-sub-module visibility. Every
//! pub(super) method delegates one-line to a sibling sub-module
//! function; no behaviour lives in this file. The 8 unit tests that
//! used to live here have been moved next to the code they cover
//! (`parsing`, `formatting`, `manage_tool`).
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
mod ticks;
mod vivling_delegation;

use super::*;
use crate::vivling::VivlingLoopEventKind;
use crate::vivling::VivlingLoopEventSource;
use crate::vl::events::LoopCommandRequest;

use self::formatting::canonical_last_status;
use self::formatting::loop_runtime_state;
use self::state::loop_state_error;
use self::types::LoopCommandSource;

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
            match source {
                LoopCommandSource::User => VivlingLoopEventSource::User,
                LoopCommandSource::Agent => VivlingLoopEventSource::Agent,
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
        codex_state::StateRuntime::init(
            self.config.codex_home.to_path_buf(),
            self.config.model_provider_id.clone(),
        )
        .await
        .map_err(loop_state_error)
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
        result: Result<crate::vivling::VivlingLoopTickResult, String>,
    ) -> color_eyre::Result<()> {
        vivling_delegation::handle_loop_tick_finished(self, thread_id, job_id, result).await
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
    pub(super) fn run_vivling_assist(&mut self, request: crate::vivling::VivlingAssistRequest) {
        vivling_delegation::run_assist(self, request);
    }

    /// codex-vl: dispatch a Vivling-managed loop tick.
    pub(super) fn run_vivling_loop_tick(
        &mut self,
        thread_id: ThreadId,
        job_id: String,
        request: crate::vivling::VivlingLoopTickRequest,
    ) {
        vivling_delegation::run_loop_tick(self, thread_id, job_id, request);
    }
}
