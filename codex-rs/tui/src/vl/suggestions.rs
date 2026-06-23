//! FASE 5 Release 5A — modello + gating dei suggerimenti loop del Vivling.
//! Pura logica: nessuna applicazione automatica, nessun side-effect. Le
//! suggestion sono volatili (non persistite). Vedi plan §scoping.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::events::LoopCommandRequest;

/// Soglie di gating (§5.2). Adult e brain sono verificati a monte sul
/// VivlingState; qui restano le soglie numeriche + confidence.
pub(crate) const BOND_MIN: u8 = 50;
pub(crate) const LOOP_EXPOSURE_MIN: u64 = 20;
pub(crate) const CONFIDENCE_MIN: f32 = 0.60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VivlingSuggestionKind {
    Unblock,
    AdjustInterval,
    Split,
    MarkDone,
    RefinePrompt,
    Disable,
}

impl VivlingSuggestionKind {
    /// Etichetta testuale stable per messaggi UI (5A apply/dismiss feedback).
    pub(crate) fn kind_label(self) -> &'static str {
        match self {
            VivlingSuggestionKind::Unblock => "unblock",
            VivlingSuggestionKind::AdjustInterval => "adjust_interval",
            VivlingSuggestionKind::Split => "split",
            VivlingSuggestionKind::MarkDone => "mark_done",
            VivlingSuggestionKind::RefinePrompt => "refine_prompt",
            VivlingSuggestionKind::Disable => "disable",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct VivlingLoopProposedAction {
    // i64 per combaciare con LoopCommandRequest::Update (vl/events.rs:30 Option<i64>)
    // e ThreadLoopJob.interval_seconds (state/.../thread_loop_job.rs:11 i64).
    #[serde(default)]
    pub(crate) interval_seconds: Option<i64>,
    #[serde(default)]
    pub(crate) prompt_text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct VivlingLoopSuggestion {
    pub(crate) id: String,
    pub(crate) loop_label: String,
    pub(crate) kind: VivlingSuggestionKind,
    pub(crate) reasoning: String,
    pub(crate) confidence: f32,
    #[serde(default)]
    pub(crate) proposed_action: Option<VivlingLoopProposedAction>,
    pub(crate) created_at: DateTime<Utc>,
}

/// Dati di gating estratti dal VivlingState dal chiamante (il modello non
/// importa codex-tui::vivling per restare puro/test-abile).
#[derive(Clone, Copy, Debug)]
pub(crate) struct SuggestionGate {
    pub(crate) is_adult: bool,
    /// V10: `brain_enabled` basta. `BrainTarget::SessionDefault` e' target
    /// valido (expression.rs:1011, state_init.rs:273, brain.rs:164
    /// active_loop_owner_identity) -> NON richiedere un brain_profile pinned,
    /// altrimenti si bloccano a torto gli Adult su SessionDefault.
    pub(crate) brain_enabled: bool,
    pub(crate) bond_value: u8,
    pub(crate) loop_exposure: u64,
    pub(crate) confidence: f32,
}

impl SuggestionGate {
    /// True solo se TUTTE le condizioni §5.2 valgono (brain_profile RIMOSSO, vedi sopra).
    pub(crate) fn passes(&self) -> bool {
        self.is_adult
            && self.brain_enabled
            && self.bond_value >= BOND_MIN
            && self.loop_exposure >= LOOP_EXPOSURE_MIN
            && self.confidence >= CONFIDENCE_MIN
    }
}

/// Mapping sicuro suggestion -> comando loop (§5.3). Ritorna None per i
/// kind che in 5A NON producono azione automatica (Unblock, Split) o per
/// proposed_action mancante/invalido.
pub(crate) fn map_to_command(
    sugg: &VivlingLoopSuggestion,
    auto_remove_on_completion: bool,
) -> Option<LoopCommandRequest> {
    let label = sugg.loop_label.clone();
    match sugg.kind {
        VivlingSuggestionKind::MarkDone => Some(if auto_remove_on_completion {
            LoopCommandRequest::Remove { label }
        } else {
            LoopCommandRequest::Disable { label }
        }),
        VivlingSuggestionKind::AdjustInterval => {
            let secs = sugg.proposed_action.as_ref()?.interval_seconds?;
            if secs <= 0 {
                return None; // interval invalido (i64: rifiuta 0 e negativi)
            }
            Some(LoopCommandRequest::Update {
                label,
                interval_seconds: Some(secs),
                prompt_text: None,
                goal_text: None,
                auto_remove_on_completion: None,
                enabled: None,
            })
        }
        VivlingSuggestionKind::RefinePrompt => {
            let prompt = sugg.proposed_action.as_ref()?.prompt_text.clone()?;
            if prompt.trim().is_empty() {
                return None; // prompt vuoto vietato (§5.3)
            }
            Some(LoopCommandRequest::Update {
                label,
                interval_seconds: None,
                prompt_text: Some(prompt),
                goal_text: None,
                auto_remove_on_completion: None,
                enabled: None,
            })
        }
        VivlingSuggestionKind::Disable => Some(LoopCommandRequest::Disable { label }),
        // 5A: nessuna azione automatica, solo messaggio.
        VivlingSuggestionKind::Unblock | VivlingSuggestionKind::Split => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(confidence: f32) -> SuggestionGate {
        SuggestionGate {
            is_adult: true,
            brain_enabled: true,
            bond_value: 60,
            loop_exposure: 30,
            confidence,
        }
    }

    #[test]
    fn gate_passes_when_all_conditions_hold() {
        // brain_enabled true e SENZA profilo pinned: V10 SessionDefault valido.
        assert!(gate(0.7).passes());
    }

    #[test]
    fn gate_fails_on_low_confidence_bond_exposure_stage_or_brain() {
        assert!(!gate(0.59).passes());
        let mut g = gate(0.7);
        g.bond_value = 49;
        assert!(!g.passes());
        let mut g = gate(0.7);
        g.loop_exposure = 19;
        assert!(!g.passes());
        let mut g = gate(0.7);
        g.is_adult = false;
        assert!(!g.passes());
        let mut g = gate(0.7);
        g.brain_enabled = false;
        assert!(!g.passes());
    }

    fn sugg(
        kind: VivlingSuggestionKind,
        action: Option<VivlingLoopProposedAction>,
    ) -> VivlingLoopSuggestion {
        VivlingLoopSuggestion {
            id: "s1".into(),
            loop_label: "build".into(),
            kind,
            reasoning: "r".into(),
            confidence: 0.8,
            proposed_action: action,
            created_at: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
        }
    }

    #[test]
    fn markdone_maps_to_remove_or_disable_by_auto_remove() {
        assert!(matches!(
            map_to_command(&sugg(VivlingSuggestionKind::MarkDone, None), true),
            Some(LoopCommandRequest::Remove { .. })
        ));
        assert!(matches!(
            map_to_command(&sugg(VivlingSuggestionKind::MarkDone, None), false),
            Some(LoopCommandRequest::Disable { .. })
        ));
    }

    #[test]
    fn adjust_interval_requires_valid_seconds() {
        let ok = sugg(
            VivlingSuggestionKind::AdjustInterval,
            Some(VivlingLoopProposedAction {
                interval_seconds: Some(120),
                prompt_text: None,
            }),
        );
        assert!(matches!(
            map_to_command(&ok, false),
            Some(LoopCommandRequest::Update { .. })
        ));
        let zero = sugg(
            VivlingSuggestionKind::AdjustInterval,
            Some(VivlingLoopProposedAction {
                interval_seconds: Some(0),
                prompt_text: None,
            }),
        );
        assert!(map_to_command(&zero, false).is_none());
        let missing = sugg(VivlingSuggestionKind::AdjustInterval, None);
        assert!(map_to_command(&missing, false).is_none());
    }

    #[test]
    fn refine_prompt_rejects_empty() {
        let empty = sugg(
            VivlingSuggestionKind::RefinePrompt,
            Some(VivlingLoopProposedAction {
                interval_seconds: None,
                prompt_text: Some("  ".into()),
            }),
        );
        assert!(map_to_command(&empty, false).is_none());
    }

    #[test]
    fn unblock_and_split_have_no_auto_action_in_5a() {
        assert!(map_to_command(&sugg(VivlingSuggestionKind::Unblock, None), false).is_none());
        assert!(map_to_command(&sugg(VivlingSuggestionKind::Split, None), false).is_none());
    }
}
