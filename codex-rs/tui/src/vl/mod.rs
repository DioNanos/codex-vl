//! codex-vl: container for custom extensions layered on top of upstream Codex.
//!
//! Everything under this module is a codex-vl-only addition. Keeping the
//! wiring in one place (a single mod in `lib.rs`, a single `AppEvent::Vl`
//! variant, dedicated extension structs for widgets) makes merges with
//! upstream predictable: conflicts stay confined to this tree instead of
//! spreading across every upstream file.

pub(crate) mod events;

pub(crate) use events::LoopCommandRequest;
pub(crate) use events::VlEvent;
