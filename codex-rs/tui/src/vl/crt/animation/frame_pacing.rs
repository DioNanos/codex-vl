//! Frame pacing target detection.
//!
//! The TUI render path is event-driven; we ask `FrameRequester` to wake us
//! up after a target interval. The target adapts to the runtime context:
//! local truecolor terminals get smooth ~30fps, slow paths (SSH) get a
//! conservative ~20fps, and non-TTY output disables animation entirely.

use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameTarget {
    /// Local truecolor terminal: ~30fps (32ms tick).
    Smooth,
    /// SSH or otherwise constrained: ~20fps (50ms tick).
    Reduced,
    /// Non-TTY (pipe, redirect): no scheduling.
    None,
}

impl FrameTarget {
    pub(crate) fn detect(probe: PacingProbe) -> Self {
        if probe.is_non_tty {
            return Self::None;
        }
        if probe.is_remote_session {
            return Self::Reduced;
        }
        Self::Smooth
    }

    /// Tick interval. Returns None when no scheduling should occur.
    pub(crate) fn tick(self) -> Option<Duration> {
        match self {
            Self::Smooth => Some(Duration::from_millis(32)),
            Self::Reduced => Some(Duration::from_millis(50)),
            Self::None => None,
        }
    }

    /// Quick check: should the renderer schedule continuous frames?
    pub(crate) fn schedules_frames(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PacingProbe {
    pub is_non_tty: bool,
    pub is_remote_session: bool,
}

impl PacingProbe {
    pub(crate) fn from_std_env() -> Self {
        Self {
            is_non_tty: !std::io::IsTerminal::is_terminal(&std::io::stdout()),
            is_remote_session: std::env::var_os("SSH_CONNECTION").is_some()
                || std::env::var_os("SSH_CLIENT").is_some()
                || std::env::var_os("SSH_TTY").is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_tty_disables_scheduling() {
        let target = FrameTarget::detect(PacingProbe {
            is_non_tty: true,
            is_remote_session: false,
        });
        assert_eq!(target, FrameTarget::None);
        assert!(target.tick().is_none());
        assert!(!target.schedules_frames());
    }

    #[test]
    fn ssh_uses_reduced_cadence() {
        let target = FrameTarget::detect(PacingProbe {
            is_non_tty: false,
            is_remote_session: true,
        });
        assert_eq!(target, FrameTarget::Reduced);
        assert_eq!(target.tick(), Some(Duration::from_millis(50)));
    }

    #[test]
    fn local_tty_uses_smooth_cadence() {
        let target = FrameTarget::detect(PacingProbe {
            is_non_tty: false,
            is_remote_session: false,
        });
        assert_eq!(target, FrameTarget::Smooth);
        assert_eq!(target.tick(), Some(Duration::from_millis(32)));
    }

    #[test]
    fn non_tty_takes_priority_over_ssh() {
        let target = FrameTarget::detect(PacingProbe {
            is_non_tty: true,
            is_remote_session: true,
        });
        assert_eq!(target, FrameTarget::None);
    }
}
