//! Custom `AppEvent` payloads used by codex-vl extensions (loop jobs,
//! Vivling brain assist, etc.).
//!
//! These are wrapped in a single `AppEvent::Vl(VlEvent)` variant at the
//! `app_event` level to keep the upstream enum minimally patched.

use codex_protocol::ThreadId;

use super::sidebar::VivlingLogKind;
use super::suggestions::VivlingLoopSuggestion;
use crate::vivling::VivlingAssistRequest;
use crate::vivling::VivlingBrainProfileRequest;
use crate::vivling::VivlingBrainRequestKind;
use crate::vivling::VivlingExpressionRequest;
use crate::vivling::VivlingExpressionResult;
use crate::vivling::VivlingLoopTickRequest;
use crate::vivling::VivlingLoopTickResult;

/// User-facing request for one of the `/loop ...` subcommands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoopCommandRequest {
    Add {
        label: String,
        interval_seconds: i64,
        prompt_text: String,
        goal_text: Option<String>,
        auto_remove_on_completion: Option<bool>,
    },
    Update {
        label: String,
        interval_seconds: Option<i64>,
        prompt_text: Option<String>,
        goal_text: Option<Option<String>>,
        auto_remove_on_completion: Option<bool>,
        enabled: Option<bool>,
    },
    List,
    Show {
        label: String,
    },
    Enable {
        label: String,
    },
    Disable {
        label: String,
    },
    Remove {
        label: String,
    },
    Trigger {
        label: String,
    },
    OwnerShow,
    OwnerSetMain,
    OwnerSetVivling,
    /// FASE5 5A — apply a pending suggestion by id (user-confirmed via `/loop apply`).
    Apply { suggestion_id: String },
    /// FASE5 5A — dismiss a pending suggestion by id (user-confirmed via `/loop dismiss`).
    Dismiss { suggestion_id: String },
}

/// Aggregated codex-vl app events. Dispatching goes through a single
/// `AppEvent::Vl(VlEvent)` at the upstream level.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub(crate) enum VlEvent {
    /// Execute a local `/loop` command for a resumable primary thread.
    LoopCommand {
        thread_id: ThreadId,
        request: LoopCommandRequest,
    },
    /// Reload loop jobs from local state for the active primary thread.
    ReloadLoopJobs { thread_id: ThreadId },
    /// Fired by a local timer when a loop job reaches its next run time.
    LoopTick { thread_id: ThreadId, job_id: String },
    /// Persist or assign the brain profile for the active Vivling.
    PersistVivlingBrainProfile { request: VivlingBrainProfileRequest },
    /// Start a background one-shot assist request for the active Vivling brain.
    RunVivlingAssist { request: VivlingAssistRequest },
    /// Result of a Vivling brain request.
    VivlingAssistFinished {
        vivling_id: String,
        kind: VivlingBrainRequestKind,
        result: Result<String, String>,
    },
    /// Start a background one-shot loop tick request for the current Vivling loop owner.
    RunVivlingLoopTick {
        thread_id: ThreadId,
        job_id: String,
        request: VivlingLoopTickRequest,
    },
    /// Result of a Vivling-managed loop tick.
    VivlingLoopTickFinished {
        thread_id: ThreadId,
        job_id: String,
        result: Result<VivlingLoopTickResult, String>,
    },
    /// Memory V2 Step 12.B.D.2 — start a background Expression LLM
    /// dispatch (CRT live phrase + proactive). The request must
    /// already have been reserved by
    /// `maybe_dispatch_expression_refresh`; the caller is responsible
    /// for `save_state` before emitting this event so the daily
    /// counter increments survive a crash between reservation and
    /// dispatch. Emitter sites land in Step 12.B.D.3
    /// (`/vivling crt-brain` + post-turn refresh hook).
    #[allow(dead_code)]
    RunVivlingExpression { request: VivlingExpressionRequest },
    /// Memory V2 Step 12.B.D.2 — async reply for an Expression
    /// dispatch. `vivling_id` identifies the Vivling whose runtime
    /// cache should receive the validated phrases.
    VivlingExpressionFinished {
        vivling_id: String,
        result: Result<VivlingExpressionResult, String>,
    },
    /// Push a message into the Vivling sidebar log.
    SidebarPushMessage {
        kind: VivlingLogKind,
        text: String,
        vivling_id: Option<String>,
    },
    /// FASE5 5A — a gated loop suggestion is ready to surface to the user.
    /// Stored in the in-session context bus; the user applies it with
    /// `/loop apply <id>` or discards it with `/loop dismiss <id>`. No
    /// automatic action is ever taken from this event alone.
    SuggestionReady { suggestion: VivlingLoopSuggestion },
    /// FASE5 5A — user confirmed `/loop apply <id>`: map the suggestion to
    /// a safe LoopCommandRequest and route it (only non-destructive kinds).
    ApplyLoopSuggestion { suggestion_id: String },
    /// FASE5 5A — user confirmed `/loop dismiss <id>`: drop the suggestion.
    DismissLoopSuggestion { suggestion_id: String },
    /// FASE5 5A — worker turn snapshot for the volatile context bus.
    /// `blockers` only ever carries explicit signals (never invented).
    ContextBusTurn {
        summary: String,
        active_loops: Vec<String>,
        blockers: Vec<String>,
    },
}
