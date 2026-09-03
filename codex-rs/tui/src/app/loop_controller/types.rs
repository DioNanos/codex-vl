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

pub(super) use crate::vl::events::LoopCommandScope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LoopCommandSource {
    User,
    Agent,
    /// An agent call made while one managed tick is in flight.  The scope is
    /// issued by the TUI and is never accepted from tool arguments.
    Managed(LoopCommandScope),
}

/// the three caller-identity cases for an agent `manage_loops`
/// DynamicToolCall, kept distinct in the type: collapsing «no scope» and
/// «ambiguous» into one `Option::None` made the managed gate reject ordinary agent calls
/// (`resolve_tool_call_source`). The managed-tick completion path uses a
/// separate, stricter resolver (`resolve_managed_tick_source`) that can
/// never yield `Agent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ManagedToolCallSource {
    /// No active server-issued scope on the thread: an ordinary agent call
    /// (pre-governance behaviour, normal permissions).
    OrdinaryAgent,
    /// Exactly one managed tick in flight: the managed-tick allowlist governs.
    Single(LoopCommandSource),
    /// Two or more scopes on the thread: caller identity is ambiguous —
    /// fail closed.
    Ambiguous,
}
