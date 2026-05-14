//! codex-vl lineage passive learning (Fase 4 iter 1).
//!
//! Helper model-level for **direct child** Vivlings to absorb parent
//! distilled summaries without becoming the active actor.
//!
//! Hard invariants enforced by this module:
//! - never grants work XP, daily work XP or active work days to the child;
//! - never unlocks brain (`brain_enabled`, `ai_mode`, `chat_unlocked_at`,
//!   `active_mode_unlocked_at` stay untouched);
//! - never calls `recompute_level` / `rebuild_learning_profiles` /
//!   `maybe_distill_memory` on the child;
//! - never mutates `loop_jobs`, `loop_exposure`, `loop_runtime_*`,
//!   `turns_observed`, `suggestions_made`, care meters, bond.
//!
//! All of these are kept off by **not calling** the side-effecting helpers
//! in `state_xp.rs`; the lineage capsule is pushed via the local
//! `push_lineage_capsule` (memory + cap only).

use chrono::DateTime;
use chrono::Utc;

use super::VivlingDistilledSummary;
use super::VivlingState;
use super::VivlingWorkMemoryEntry;
use super::WorkArchetype;
use super::text_utils::fnv1a64;
use super::text_utils::truncate_summary;

/// Capsule kind tag used for parent distilled summaries absorbed by a child.
pub(crate) const LINEAGE_PARENT_SUMMARY_KIND: &str = "lineage_parent_summary";

/// Weight given to lineage capsules — high enough to register in memory,
/// low enough to never drive level/XP (helper never grants XP anyway).
pub(crate) const LINEAGE_PARENT_SUMMARY_WEIGHT: u64 = 3;

/// Max new lineage capsules absorbed per propagation cycle for a single
/// child. Bounds disk write fan-out per parent turn.
pub(crate) const MAX_LINEAGE_PER_PROPAGATION_CYCLE: usize = 3;

/// Cap on lineage capsules kept in `work_memory` for a single child.
/// Eviction is FIFO on the lineage-only subset; non-lineage entries are
/// left alone.
pub(crate) const MAX_LINEAGE_CAPSULES_IN_MEMORY: usize = 20;

/// Bound on the dedup key ring (`lineage_seen_parent_summary_keys`).
/// FIFO eviction past this size.
pub(crate) const LINEAGE_SEEN_KEYS_BOUND: usize = 64;

/// Minimum interval between two propagation cycles targeting the same
/// child. Used at runtime via the child's last lineage capsule timestamp.
pub(crate) const LINEAGE_PROPAGATION_COOLDOWN_SECS: i64 = 60;

/// Compute the deterministic dedup key for a `(parent, kind, topic, last_seen_at)`
/// tuple. Identical input always maps to the same key; re-distilling the
/// same topic with a newer `last_seen_at` produces a different key so the
/// child can re-absorb the refreshed summary exactly once.
pub(crate) fn lineage_seen_key(
    parent_vivling_id: &str,
    kind: &str,
    topic: &str,
    last_seen_at: DateTime<Utc>,
) -> String {
    let payload = format!(
        "{parent_vivling_id}\u{001f}{kind}\u{001f}{topic}\u{001f}{}",
        last_seen_at.timestamp_millis()
    );
    format!("{:016x}", fnv1a64(payload.as_bytes()))
}

/// Outcome of attempting to absorb one parent distilled summary into the
/// child. `Some(entry)` means the summary was new and was appended;
/// `None` means it was already seen (dedup hit).
pub(crate) struct LineageAbsorption {
    pub(crate) entry: VivlingWorkMemoryEntry,
    pub(crate) seen_key: String,
}

impl VivlingState {
    /// Absorb a single parent distilled summary into this child without
    /// granting XP, active work days, brain unlock or loop ownership.
    ///
    /// Returns `Some(LineageAbsorption)` if the summary is new for this
    /// child (a memory capsule was appended and dedup key recorded), or
    /// `None` if the summary was already seen (dedup hit).
    ///
    /// Caps:
    /// - lineage capsule count in `work_memory` capped at
    ///   `MAX_LINEAGE_CAPSULES_IN_MEMORY` (drop-oldest on the lineage
    ///   subset only);
    /// - dedup ring capped at `LINEAGE_SEEN_KEYS_BOUND` (FIFO).
    pub(crate) fn record_lineage_parent_summary(
        &mut self,
        parent_vivling_id: &str,
        summary: &VivlingDistilledSummary,
        now: DateTime<Utc>,
    ) -> Option<LineageAbsorption> {
        let key = lineage_seen_key(
            parent_vivling_id,
            &summary.kind,
            &summary.topic,
            summary.last_seen_at,
        );
        if self
            .lineage_seen_parent_summary_keys
            .iter()
            .any(|k| k == &key)
        {
            return None;
        }

        let topic_label = if summary.topic.is_empty() {
            "lineage"
        } else {
            summary.topic.as_str()
        };
        let trimmed = truncate_summary(summary.summary.trim(), 120);
        let memory_summary = if trimmed.is_empty() {
            format!("lineage: parent shared a {topic_label} pattern")
        } else {
            format!("lineage:{topic_label}: {trimmed}")
        };

        let entry = VivlingWorkMemoryEntry {
            kind: LINEAGE_PARENT_SUMMARY_KIND.to_string(),
            summary: memory_summary,
            archetype: summary.archetype,
            weight: LINEAGE_PARENT_SUMMARY_WEIGHT,
            created_at: now,
        };

        self.work_memory.push(entry.clone());
        self.evict_lineage_capsules_if_needed();

        self.lineage_seen_parent_summary_keys.push(key.clone());
        if self.lineage_seen_parent_summary_keys.len() > LINEAGE_SEEN_KEYS_BOUND {
            let overflow = self
                .lineage_seen_parent_summary_keys
                .len()
                .saturating_sub(LINEAGE_SEEN_KEYS_BOUND);
            self.lineage_seen_parent_summary_keys.drain(0..overflow);
        }

        Some(LineageAbsorption {
            entry,
            seen_key: key,
        })
    }

    /// Drop-oldest eviction on lineage capsules only. Non-lineage entries
    /// in `work_memory` are preserved; we never touch them.
    fn evict_lineage_capsules_if_needed(&mut self) {
        let lineage_count = self
            .work_memory
            .iter()
            .filter(|entry| entry.kind == LINEAGE_PARENT_SUMMARY_KIND)
            .count();
        if lineage_count <= MAX_LINEAGE_CAPSULES_IN_MEMORY {
            return;
        }
        let mut to_remove = lineage_count - MAX_LINEAGE_CAPSULES_IN_MEMORY;
        let mut keep = Vec::with_capacity(self.work_memory.len());
        for entry in self.work_memory.drain(..) {
            if to_remove > 0 && entry.kind == LINEAGE_PARENT_SUMMARY_KIND {
                to_remove -= 1;
                continue;
            }
            keep.push(entry);
        }
        self.work_memory = keep;
    }
}

/// Increment the parent's lineage rarity pressure on a successful spawn.
/// Caps at 10. Saturating, idempotent at full.
///
/// **Policy** (DAG design directive 2026-05-15): pressure stays *inside*
/// species — it never swaps the offspring's species. It biases the
/// quality roll on `gene_vector` and `brain_potential`. See
/// [`apply_lineage_quality_roll`].
pub(crate) fn bump_lineage_rarity_pressure(current: u8) -> u8 {
    current.saturating_add(2).min(10)
}

/// Threshold (in `lineage_rarity_pressure_pct` units) at and above which
/// a successful quality-roll trigger flags the offspring as
/// `lineage_blessed`. Used for audit/CRT cosmetic surfacing.
pub(crate) const LINEAGE_BLESSED_PRESSURE_THRESHOLD: u8 = 6;

/// Bias amount (additive) applied to each gene temperament when the
/// quality roll triggers. Computed from `pressure_pct` as `pressure/2`,
/// clamped to `1..=5`.
fn quality_uplift(pressure_pct: u8) -> u8 {
    (pressure_pct / 2).clamp(1, 5)
}

/// Bias amount applied to `brain_potential` (f32) on trigger. Small,
/// deterministic, scaled by pressure.
fn brain_potential_uplift(pressure_pct: u8) -> f32 {
    (pressure_pct as f32) * 0.01
}

/// Apply the **dentro-specie** quality roll on a freshly-built offspring
/// gene vector. Never changes species. May lift temperament stats and
/// `brain_potential` if the roll triggers.
///
/// Returns `true` if the roll triggered (caller can then set
/// `lineage_blessed` when pressure ≥ [`LINEAGE_BLESSED_PRESSURE_THRESHOLD`]).
///
/// Hard contract:
/// - never modifies `affinity_mod` (species/work bias stays untouched);
/// - never lifts past temperament clamp ceilings (saturating + `.min(99)`);
/// - never lifts `brain_potential` past `clamp_brain_potential` ceiling
///   (the gene module enforces its own clamp; we feed clamped values).
pub(crate) fn apply_lineage_quality_roll(
    gene: &mut super::gene::VivlingGeneVector,
    hash: u64,
    pressure_pct: u8,
) -> bool {
    if pressure_pct == 0 {
        return false;
    }
    let cap = pressure_pct.min(10);
    let trigger = ((hash >> 32) % 100) as u8;
    if trigger >= cap {
        return false;
    }
    let uplift = quality_uplift(cap);
    gene.curiosity = gene.curiosity.saturating_add(uplift).min(99);
    gene.caution = gene.caution.saturating_add(uplift).min(99);
    gene.sociability = gene.sociability.saturating_add(uplift).min(99);
    gene.patience = gene.patience.saturating_add(uplift).min(99);
    let brain_lift = brain_potential_uplift(cap);
    let next = gene.brain_potential + brain_lift;
    gene.brain_potential = super::gene::clamp_brain_potential_value(next);
    true
}

/// Returns `true` when the offspring should be marked as
/// `lineage_blessed` based on the pressure value at spawn time.
pub(crate) fn is_lineage_blessed_threshold(pressure_pct: u8) -> bool {
    pressure_pct >= LINEAGE_BLESSED_PRESSURE_THRESHOLD
}
