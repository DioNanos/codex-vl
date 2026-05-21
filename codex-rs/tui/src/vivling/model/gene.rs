//! Re-export shim for `VivlingGeneVector` and gene-related helpers.
//!
//! Definitions live in `codex-vivling-core::model::gene` (Step 1.B). Logic
//! that operates on `VivlingState` continues to live in `state_*.rs` /
//! `lineage.rs` here in `codex-tui` because `VivlingState` itself still
//! references `crate::vivling::VivlingBond` and has not been moved yet.
pub(crate) use codex_vivling_core::model::gene::*;
