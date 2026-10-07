//! Step 12.C — Vivling lifecycle: dispatch phase (FSM) plus the ExpressionKind type.
//!
//! The FSM models ONLY the mutually exclusive phase of the runtime wrapper.
//! The `expression_in_flight` gate is an ORTHOGONAL axis (a field on the wrapper,
//! Task 3): an async expression dispatch can stay in flight while the
//! Vivling is TaskRunning. Runtime-only: never serialized, no schema bump.
//! Animation pacing and the one-shot latch stay out (see plan §scoping).

use std::time::Instant;

/// Step 12.D consumerà le varianti non-`Crt` per il dispatch kind-aware;
/// in 12.C il gate usa solo la presenza (hardcoded `Crt`).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExpressionKind {
    Assist,
    LoopTick,
    Crt,
    Bootstrap,
}

#[derive(Clone, Debug, Default)]
pub(crate) enum VivlingLifecyclePhase {
    /// Pre-configure: nessun codex_home.
    #[default]
    Unavailable,
    /// Configured, nessun turno worker in corso.
    Idle,
    /// Un turno worker è in esecuzione.
    TaskRunning {
        /// Step 12.D: durata del turno per observability.
        #[allow(dead_code)]
        since: Instant,
    },
}

impl VivlingLifecyclePhase {
    /// Step 12.D: etichetta observability (`state.kind()` in 1 stringa).
    #[allow(dead_code)]
    pub(crate) fn kind_label(&self) -> &'static str {
        match self {
            VivlingLifecyclePhase::Unavailable => "unavailable",
            VivlingLifecyclePhase::Idle => "idle",
            VivlingLifecyclePhase::TaskRunning { .. } => "task_running",
        }
    }

    pub(crate) fn is_task_running(&self) -> bool {
        matches!(self, VivlingLifecyclePhase::TaskRunning { .. })
    }

    /// Unavailable -> Idle (idempotente: non retrocede da TaskRunning).
    pub(crate) fn set_available(&mut self) {
        if matches!(self, VivlingLifecyclePhase::Unavailable) {
            *self = VivlingLifecyclePhase::Idle;
        }
    }

    /// Idle -> TaskRunning. false se non in Idle (best-effort, no panic).
    pub(crate) fn begin_task(&mut self, now: Instant) -> bool {
        if matches!(self, VivlingLifecyclePhase::Idle) {
            *self = VivlingLifecyclePhase::TaskRunning { since: now };
            true
        } else {
            false
        }
    }

    /// TaskRunning -> Idle. false (no-op) se non in TaskRunning.
    pub(crate) fn end_task(&mut self) -> bool {
        if self.is_task_running() {
            *self = VivlingLifecyclePhase::Idle;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn default_is_unavailable() {
        let p = VivlingLifecyclePhase::default();
        assert_eq!(p.kind_label(), "unavailable");
        assert!(!p.is_task_running());
    }

    #[test]
    fn configure_moves_unavailable_to_idle_idempotent() {
        let mut p = VivlingLifecyclePhase::Unavailable;
        p.set_available();
        assert_eq!(p.kind_label(), "idle");
        let now = Instant::now();
        p.begin_task(now);
        p.set_available(); // non retrocede da TaskRunning
        assert_eq!(p.kind_label(), "task_running");
    }

    #[test]
    fn task_cycle() {
        let now = Instant::now();
        let mut p = VivlingLifecyclePhase::Idle;
        assert!(p.begin_task(now));
        assert!(p.is_task_running());
        assert!(!p.begin_task(now)); // doppio begin rifiutato
        assert!(p.end_task());
        assert_eq!(p.kind_label(), "idle");
        assert!(!p.end_task()); // end su Idle = no-op
    }
}
