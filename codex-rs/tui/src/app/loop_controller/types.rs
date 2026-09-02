//! codex-vl loop_controller: shared types used across sub-modules.
//!
//! Kept internal to the `app::loop_controller` module: `pub(super)`
//! visibility limits these to the parent module tree only.

#[derive(Debug)]
pub(super) struct LoopActionOutcome {
    pub(super) success: bool,
    pub(super) message: String,
    pub(super) payload: serde_json::Value,
}

use crate::vl::events::LoopCommandScope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LoopCommandSource {
    User,
    Agent,
    /// An agent call made while one managed tick is in flight.  The scope is
    /// issued by the TUI and is never accepted from tool arguments.
    Managed(LoopCommandScope),
}
