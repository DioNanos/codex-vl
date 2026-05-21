//! Pure Vivling model data types.
//!
//! Hosts the serde-friendly enums and structs that describe a Vivling without
//! pulling in TUI/runtime dependencies. Imported by both `codex-tui` and
//! `codex-vivling-memory-agent`.
//!
//! Step 1.B scope: only the strictly pure submodules move here. Types with a
//! TUI/runtime coupling (`VivlingState` itself, `state_*` impl blocks,
//! `lineage` impls on `VivlingState`) remain in `codex-tui::vivling::model`
//! and are scheduled for a later step once their coupling is broken.

pub mod constants;
pub mod gene;
pub mod text_utils;
pub mod types;

pub use constants::*;
pub use gene::*;
pub use text_utils::*;
pub use types::*;
