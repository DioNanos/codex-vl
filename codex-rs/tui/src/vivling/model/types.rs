//! Re-export shim for Vivling pure data types.
//!
//! The actual definitions now live in `codex-vivling-core::model::types`
//! (Step 1.B). Call sites inside `codex-tui` continue to use
//! `super::types::*` via this shim so no churn is forced in the same commit.
pub(crate) use codex_vivling_core::model::types::*;
