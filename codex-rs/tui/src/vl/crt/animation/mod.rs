//! CRT Rich-tier animation engine.
//!
//! Stateful seam between `Vivling::render()` and the otherwise-pure
//! `vl/crt/` rendering primitives. Adds:
//!
//! - `VivlingCrtConfig`: opt-out toggles read from `~/.codex/config.toml`
//!   under `[vivling.crt]`. Stored independently of the upstream
//!   `ConfigToml` to keep merge surface zero.
//! - `CrtAnimationLedger`: tracks four discrete state changes (mode,
//!   message, insight, boot) that drive event-based transitions.
//! - `TransitionPhases`: per-frame eased snapshot consumed by the
//!   compose pipeline and the `effects` layer.
//! - `BootSequence`: the multi-row boot animation rendered into a
//!   temporarily-expanded strip on the first render of an app run.
//! - `FrameTarget`: detects the runtime context to pick a frame cadence
//!   (smooth, reduced for SSH, or off for non-TTY).

pub(crate) mod boot;
pub(crate) mod config;
pub(crate) mod frame_pacing;
pub(crate) mod ledger;
pub(crate) mod transitions;

pub(crate) use boot::BOOT_STRIP_HEIGHT;
pub(crate) use boot::render_boot_strip;
pub(crate) use config::VivlingCrtConfig;
pub(crate) use frame_pacing::FrameTarget;
pub(crate) use frame_pacing::PacingProbe;
pub(crate) use ledger::CrtAnimationLedger;
pub(crate) use transitions::TransitionPhases;
