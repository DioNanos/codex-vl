//! codex-vl lineage passive learning runtime hook (Fase 4 iter 1A).
//!
//! Propagates the **active primary**'s distilled summaries to all
//! direct children whose `cultural_parent_vivling_id` points to it (with
//! a legacy fallback on `parent_vivling_id` for old states that predate
//! the cultural-parent split).
//!
//! Hard invariants:
//! - never touches `roster.active_vivling_id` (one-active-only);
//! - never unlocks `brain_enabled` / `ai_mode` / `chat_unlocked_at` /
//!   `active_mode_unlocked_at` on the child;
//! - never grants `work_xp` / `daily_work_xp` / `active_work_days` /
//!   `level` on the child (the model helper enforces this);
//! - never recurses past depth 1: the recipient's own `distill_memory`
//!   filters out `LINEAGE_PARENT_SUMMARY_KIND`, so re-distillation
//!   cannot cascade to grandchildren via this path;
//! - imported children are skipped in iter 1A;
//! - rate-limited (max 3 absorptions per cycle per child) and cooled
//!   down (≥ 60s between two cycles targeting the same child).

use chrono::DateTime;
use chrono::Utc;

use super::super::model::VivlingState;
use super::super::model::lineage::LINEAGE_PARENT_SUMMARY_KIND;
use super::super::model::lineage::LINEAGE_PROPAGATION_COOLDOWN_SECS;
use super::super::model::lineage::MAX_LINEAGE_PER_PROPAGATION_CYCLE;
use super::Vivling;

/// Report describing what the propagation cycle did this turn.
/// Mostly used by tests; runtime callers only care about success.
#[derive(Debug, Default, Clone)]
pub(crate) struct LineageEchoReport {
    pub(crate) children_visited: usize,
    pub(crate) capsules_absorbed: usize,
    pub(crate) children_skipped_cooldown: usize,
    pub(crate) children_skipped_imported: usize,
    pub(crate) children_skipped_no_delta: usize,
}

impl Vivling {
    /// Hook called after the active primary's `record_turn_completed`
    /// (or `record_loop_event`) has finished updating its own
    /// `distilled_summaries`. No-op when there is no active primary,
    /// when its distilled summaries are empty, or when no direct
    /// children exist.
    pub(crate) fn propagate_parent_summaries_to_children(
        &mut self,
    ) -> Result<LineageEchoReport, String> {
        let Some(parent_state) = self.state.clone() else {
            return Ok(LineageEchoReport::default());
        };
        if !parent_state.hatched || !parent_state.is_primary {
            return Ok(LineageEchoReport::default());
        }
        if parent_state.distilled_summaries.is_empty() {
            return Ok(LineageEchoReport::default());
        }

        let primary_id = parent_state.vivling_id.clone();
        let lineage_states = self
            .load_lineage_states(&parent_state.primary_vivling_id)
            .map_err(|err| err.to_string())?;
        let now = Utc::now();
        let mut report = LineageEchoReport::default();

        for mut child in lineage_states {
            if !is_eligible_child(&child, &primary_id) {
                continue;
            }
            if child.is_imported {
                report.children_skipped_imported += 1;
                continue;
            }
            if recent_lineage_cooldown_active(&child, now) {
                report.children_skipped_cooldown += 1;
                continue;
            }
            report.children_visited += 1;

            let absorbed = absorb_parent_summaries(
                &mut child,
                &parent_state.distilled_summaries,
                &parent_state.vivling_id,
                now,
            );
            if absorbed.is_empty() {
                report.children_skipped_no_delta += 1;
                continue;
            }

            // G2v2 batch rebuild: a single rebuild per child per cycle,
            // after all 1..3 absorptions land. Never calls
            // maybe_distill_memory on the child — distillation of
            // lineage capsules is blocked at the model level by the
            // anti-cascade filter in `state_memory.rs::distill_memory`.
            child.rebuild_learning_profiles();

            // MSA index per child capsule under the *child's* vivling_id
            // (G3): the parent's collection stays untouched.
            if let Some(msa) = self.msa.as_deref() {
                for capsule in &absorbed {
                    msa.index_capsule(&child.vivling_id, capsule);
                }
            }

            report.capsules_absorbed += absorbed.len();

            self.save_state_record(&child, /*set_active*/ false, child.is_imported)
                .map_err(|err| err.to_string())?;
        }

        Ok(report)
    }
}

/// Eligibility test: the child must declare `primary_id` as its
/// cultural parent. Legacy entries without a cultural parent fall back
/// to the biological `parent_vivling_id`.
fn is_eligible_child(child: &VivlingState, primary_id: &str) -> bool {
    if !child.hatched {
        return false;
    }
    if child.vivling_id == primary_id {
        return false;
    }
    if let Some(cultural) = child.cultural_parent_vivling_id.as_deref() {
        return cultural == primary_id;
    }
    // Legacy fallback: pre-cultural-parent states still belong to the
    // biological parent for propagation purposes.
    child.parent_vivling_id.as_deref() == Some(primary_id)
}

/// Cooldown: skip if the most recent lineage capsule on the child is
/// younger than `LINEAGE_PROPAGATION_COOLDOWN_SECS`.
fn recent_lineage_cooldown_active(child: &VivlingState, now: DateTime<Utc>) -> bool {
    let most_recent = child
        .work_memory
        .iter()
        .filter(|entry| entry.kind == LINEAGE_PARENT_SUMMARY_KIND)
        .map(|entry| entry.created_at)
        .max();
    let Some(latest) = most_recent else {
        return false;
    };
    let elapsed = now.signed_duration_since(latest);
    elapsed.num_seconds() < LINEAGE_PROPAGATION_COOLDOWN_SECS
}

/// Walk the parent's distilled summaries (most recent first) and absorb
/// up to `MAX_LINEAGE_PER_PROPAGATION_CYCLE` new entries into the
/// child. Returns the capsules actually appended (for MSA indexing).
fn absorb_parent_summaries(
    child: &mut VivlingState,
    parent_distilled: &[super::super::model::VivlingDistilledSummary],
    parent_vivling_id: &str,
    now: DateTime<Utc>,
) -> Vec<super::super::model::VivlingWorkMemoryEntry> {
    let mut ordered: Vec<&super::super::model::VivlingDistilledSummary> =
        parent_distilled.iter().collect();
    ordered.sort_by(|a, b| b.last_seen_at.cmp(&a.last_seen_at));

    let mut absorbed = Vec::new();
    for summary in ordered {
        if absorbed.len() >= MAX_LINEAGE_PER_PROPAGATION_CYCLE {
            break;
        }
        if let Some(result) = child.record_lineage_parent_summary(parent_vivling_id, summary, now) {
            absorbed.push(result.entry);
        }
    }
    absorbed
}
