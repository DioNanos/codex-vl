//! Lifecycle voice tone hint used by relational care-effects modulation.
//!
//! `vl/lifecycle` is a generic layer and must NOT import `crate::vivling::*`.
//! The boundary adapter in `*_ext.rs` translates `vivling::BondTone` to this
//! enum, so the lifecycle layer can pick the right thought/proactive pool
//! without ever knowing the bond domain exists.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum LifecycleVoiceTone {
    /// Safe fallback used when no Vivling is hatched or the adapter has not
    /// resolved a tone yet — matches the most professional pool.
    #[default]
    Neutral,
    /// Default warmth — preserves the pool that shipped before care-effects.
    Warm,
    /// Warm, contextual, allusive — used at high bond levels.
    Familiar,
}
