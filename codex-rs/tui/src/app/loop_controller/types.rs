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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LoopCommandSource {
    User,
    Agent,
}
