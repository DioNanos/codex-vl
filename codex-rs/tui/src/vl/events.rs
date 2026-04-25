//! Custom `AppEvent` payloads used by codex-vl extensions (loop jobs,
//! Vivling brain assist, etc.).
//!
//! These are wrapped in a single `AppEvent::Vl(VlEvent)` variant at the
//! `app_event` level to keep the upstream enum minimally patched.

use codex_protocol::ThreadId;

use crate::vivling::VivlingAssistRequest;
use crate::vivling::VivlingBrainProfileRequest;
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
    /// Result of a Vivling brain assist request.
    VivlingAssistFinished {
        vivling_id: String,
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
}
