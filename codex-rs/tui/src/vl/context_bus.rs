//! FASE 5 Release 5A — context bus volatile (§5.1). Stato in-sessione, NON
//! persistito, NON memoria. Sintetizza il contesto del worker per il Vivling
//! senza fondere le memorie. Regole di troncamento dure.

use chrono::{DateTime, Utc};

use super::suggestions::{VivlingLoopSuggestion, VivlingSuggestionKind};

const TURN_SUMMARY_MAX: usize = 500;
const ACTIVE_LOOPS_MAX: usize = 10;

#[derive(Clone, Debug)]
pub(crate) struct WorkerTurnSnapshot {
    pub(crate) turn_summary: String,
    pub(crate) active_loops: Vec<String>,
    pub(crate) blockers: Vec<String>,
    pub(crate) timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct VivlingContextBus {
    pub(crate) worker_last_context: Option<WorkerTurnSnapshot>,
    pub(crate) pending_suggestions: Vec<VivlingLoopSuggestion>,
}

impl VivlingContextBus {
    /// Registra uno snapshot del turno worker applicando i limiti §5.1:
    /// summary <= 500 char, active_loops <= 10, blockers solo se passati
    /// (mai inventati dal bus).
    pub(crate) fn record_turn(
        &mut self,
        summary: String,
        mut active_loops: Vec<String>,
        blockers: Vec<String>,
        now: DateTime<Utc>,
    ) {
        let mut turn_summary = summary;
        if turn_summary.chars().count() > TURN_SUMMARY_MAX {
            turn_summary = turn_summary.chars().take(TURN_SUMMARY_MAX).collect();
        }
        active_loops.truncate(ACTIVE_LOOPS_MAX);
        self.worker_last_context = Some(WorkerTurnSnapshot {
            turn_summary,
            active_loops,
            blockers,
            timestamp: now,
        });
    }

    /// Aggiunge una suggestion se non e un duplicato recente per (label, kind).
    pub(crate) fn push_suggestion(&mut self, sugg: VivlingLoopSuggestion) {
        if self.is_duplicate(&sugg.loop_label, sugg.kind) {
            return;
        }
        self.pending_suggestions.push(sugg);
    }

    pub(crate) fn is_duplicate(&self, label: &str, kind: VivlingSuggestionKind) -> bool {
        self.pending_suggestions
            .iter()
            .any(|s| s.loop_label == label && s.kind == kind)
    }

    /// Rimuove e restituisce la suggestion con quell'id (per apply/dismiss).
    pub(crate) fn take_suggestion(&mut self, id: &str) -> Option<VivlingLoopSuggestion> {
        let pos = self.pending_suggestions.iter().position(|s| s.id == id)?;
        Some(self.pending_suggestions.remove(pos))
    }

    /// FASE5 5A — formatta lo snapshot worker (volatile) per il prompt del
    /// loop tick, così il Vivling vede l'attività worker recente. Legge tutti
    /// i campi di [`WorkerTurnSnapshot`]. None se non c'e' uno snapshot.
    pub(crate) fn worker_context_summary(&self) -> Option<String> {
        let snap = self.worker_last_context.as_ref()?;
        let mut out = String::new();
        out.push_str(&snap.turn_summary);
        if !snap.active_loops.is_empty() {
            out.push_str("\nactive loops: ");
            out.push_str(&snap.active_loops.join(", "));
        }
        if !snap.blockers.is_empty() {
            out.push_str("\nblockers: ");
            out.push_str(&snap.blockers.join(", "));
        }
        out.push_str(&format!("\n(as of {})", snap.timestamp.to_rfc3339()));
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(0, 0).unwrap()
    }
    fn sugg(id: &str, label: &str, kind: VivlingSuggestionKind) -> VivlingLoopSuggestion {
        VivlingLoopSuggestion {
            id: id.into(),
            loop_label: label.into(),
            kind,
            reasoning: "r".into(),
            confidence: 0.8,
            proposed_action: None,
            created_at: now(),
        }
    }

    #[test]
    fn record_turn_truncates_summary_and_loops() {
        let mut bus = VivlingContextBus::default();
        let long = "x".repeat(800);
        let loops: Vec<String> = (0..20).map(|i| format!("l{i}")).collect();
        bus.record_turn(long, loops, vec!["blk".into()], now());
        let snap = bus.worker_last_context.unwrap();
        assert_eq!(snap.turn_summary.chars().count(), 500);
        assert_eq!(snap.active_loops.len(), 10);
        assert_eq!(snap.blockers, vec!["blk".to_string()]);
    }

    #[test]
    fn dedupe_blocks_same_label_kind() {
        let mut bus = VivlingContextBus::default();
        bus.push_suggestion(sugg("a", "build", VivlingSuggestionKind::Disable));
        bus.push_suggestion(sugg("b", "build", VivlingSuggestionKind::Disable)); // dup
        bus.push_suggestion(sugg("c", "build", VivlingSuggestionKind::MarkDone)); // diverso kind
        assert_eq!(bus.pending_suggestions.len(), 2);
    }

    #[test]
    fn take_suggestion_removes_by_id() {
        let mut bus = VivlingContextBus::default();
        bus.push_suggestion(sugg("a", "build", VivlingSuggestionKind::Disable));
        assert!(bus.take_suggestion("a").is_some());
        assert!(bus.take_suggestion("a").is_none());
        assert!(bus.pending_suggestions.is_empty());
    }

    /// SAFETY 5A (DoD, OBBLIGATORIO): una suggestion nel bus NON produce
    /// alcuna azione finche non viene esplicitamente estratta con
    /// `take_suggestion` (path `/loop apply`). `SuggestionReady` ->
    /// `push_suggestion` NON genera comandi. `/loop dismiss` estrae senza
    /// mappare. Solo il path apply chiama `map_to_command`.
    #[test]
    fn fase5_5a_no_autonomy_suggestion_pending_until_explicit_apply() {
        let mut bus = VivlingContextBus::default();
        // SuggestionReady equivalente: suggestion nel bus, NESSUN comando.
        bus.push_suggestion(sugg("a", "build", VivlingSuggestionKind::Disable));
        assert_eq!(bus.pending_suggestions.len(), 1);

        // /loop dismiss: take + scarta (NESSUN map_to_command).
        let dismissed = bus.take_suggestion("a");
        assert!(dismissed.is_some());
        assert!(bus.pending_suggestions.is_empty());

        // /loop apply path: take + map_to_command genera il comando.
        bus.push_suggestion(sugg("b", "build", VivlingSuggestionKind::Disable));
        let for_apply = bus.take_suggestion("b").expect("present");
        let cmd = super::super::suggestions::map_to_command(&for_apply, false);
        assert!(matches!(
            cmd,
            Some(crate::vl::events::LoopCommandRequest::Disable { .. })
        ));
    }
}
