//! Shared core crate for Vivling Memory V2.
//!
//! Hosts model types, safety primitives (file lock + atomic write + backup),
//! deterministic path helpers, and secret redaction. Imported by both
//! `codex-tui` (Vivling runtime) and `codex-vivling-memory-agent` (sleep-time
//! batch worker), without creating a dependency cycle.
//!
//! Step 1.A: only `paths`, `safety`, `redaction` are populated. `VivlingState`
//! model types remain in `codex-tui::vivling::model` and will move here in
//! Step 1.B once the safety primitives are validated.

pub mod paths;
pub mod redaction;
pub mod safety;
