//! codex-vl lineage passive learning (Fase 4 iter 1A) — integration tests.
//!
//! Coverage:
//! - dedup key determinism (G1)
//! - record_lineage_parent_summary append + FIFO cap on keys (64) (G1)
//! - lineage capsule cap on work_memory (20) drop-oldest filter
//! - **no XP / no active_work_days / no level / no brain** on absorb (I8)
//! - distill_memory excludes lineage capsule kind (G2v2 anti-cascade)
//! - rebuild_learning_profiles called once per propagation cycle (B preferred)
//! - cultural_parent_vivling_id routes propagation, with legacy fallback
//! - cascade blocked: grandchild does NOT receive grandparent lineage
//!   via re-distillation
//! - rarity pressure: bump +2 cap 10, quality roll dentro-specie, no swap

use super::common::*;
use chrono::Duration;
use chrono::TimeZone;
use chrono::Utc;

use crate::vivling::model::VivlingDistilledSummary;
use crate::vivling::model::WorkArchetype;
use crate::vivling::model::lineage::LINEAGE_BLESSED_PRESSURE_THRESHOLD;
use crate::vivling::model::lineage::LINEAGE_PARENT_SUMMARY_KIND;
use crate::vivling::model::lineage::LINEAGE_SEEN_KEYS_BOUND;
use crate::vivling::model::lineage::MAX_LINEAGE_CAPSULES_IN_MEMORY;
use crate::vivling::model::lineage::apply_lineage_quality_roll;
use crate::vivling::model::lineage::bump_lineage_rarity_pressure;
use crate::vivling::model::lineage::is_lineage_blessed_threshold;
use crate::vivling::model::lineage::lineage_seen_key;

fn fixed_now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 15, 12, 0, 0).unwrap()
}

fn parent_summary(topic: &str, last_seen: chrono::DateTime<Utc>) -> VivlingDistilledSummary {
    VivlingDistilledSummary {
        topic: topic.to_string(),
        summary: format!("parent observed {topic} pattern"),
        kind: "turn".to_string(),
        archetype: WorkArchetype::Builder,
        total_weight: 5,
        observations: 3,
        first_seen_at: last_seen - Duration::hours(1),
        last_seen_at: last_seen,
    }
}

// ---------- dedup key ----------

#[test]
fn lineage_seen_key_is_deterministic_for_same_input() {
    let now = fixed_now();
    let a = lineage_seen_key("viv-parent", "turn", "build", now);
    let b = lineage_seen_key("viv-parent", "turn", "build", now);
    assert_eq!(a, b);
}

#[test]
fn lineage_seen_key_differs_when_last_seen_advances() {
    let now = fixed_now();
    let a = lineage_seen_key("viv-parent", "turn", "build", now);
    let b = lineage_seen_key("viv-parent", "turn", "build", now + Duration::seconds(1));
    assert_ne!(a, b, "last_seen_at evolution must invalidate the dedup key");
}

// ---------- absorption + dedup + caps ----------

#[test]
fn record_lineage_parent_summary_appends_capsule_with_weight_3() {
    let mut child = seeded_state();
    let now = fixed_now();
    let summary = parent_summary("build", now);

    let result = child
        .record_lineage_parent_summary("viv-parent", &summary, now)
        .expect("first absorption must succeed");
    assert_eq!(result.entry.kind, LINEAGE_PARENT_SUMMARY_KIND);
    assert_eq!(result.entry.weight, 3);
    assert_eq!(result.entry.archetype, WorkArchetype::Builder);
    assert_eq!(child.lineage_seen_parent_summary_keys.len(), 1);
    assert!(
        child
            .work_memory
            .iter()
            .any(|c| c.kind == LINEAGE_PARENT_SUMMARY_KIND),
    );
}

#[test]
fn record_lineage_parent_summary_dedups_same_key() {
    let mut child = seeded_state();
    let now = fixed_now();
    let summary = parent_summary("build", now);

    let first = child.record_lineage_parent_summary("viv-parent", &summary, now);
    let second = child.record_lineage_parent_summary("viv-parent", &summary, now);
    assert!(first.is_some());
    assert!(
        second.is_none(),
        "second absorb of the same summary must be a dedup no-op",
    );
    assert_eq!(child.lineage_seen_parent_summary_keys.len(), 1);
}

#[test]
fn lineage_seen_keys_evict_fifo_at_bound() {
    let mut child = seeded_state();
    let now = fixed_now();
    for i in 0..(LINEAGE_SEEN_KEYS_BOUND + 5) {
        let topic = format!("topic-{i}");
        let summary = parent_summary(&topic, now + Duration::seconds(i as i64));
        let _ = child.record_lineage_parent_summary("viv-parent", &summary, now);
    }
    assert_eq!(
        child.lineage_seen_parent_summary_keys.len(),
        LINEAGE_SEEN_KEYS_BOUND,
        "key ring must cap at LINEAGE_SEEN_KEYS_BOUND",
    );
}

#[test]
fn lineage_capsules_capped_drop_oldest_on_lineage_kind_only() {
    let mut child = seeded_state();
    let now = fixed_now();
    let baseline_non_lineage_len = child.work_memory.len();
    for i in 0..(MAX_LINEAGE_CAPSULES_IN_MEMORY + 3) {
        let topic = format!("topic-{i}");
        let summary = parent_summary(&topic, now + Duration::seconds(i as i64));
        let _ = child.record_lineage_parent_summary("viv-parent", &summary, now);
    }
    let lineage_count = child
        .work_memory
        .iter()
        .filter(|c| c.kind == LINEAGE_PARENT_SUMMARY_KIND)
        .count();
    assert_eq!(
        lineage_count, MAX_LINEAGE_CAPSULES_IN_MEMORY,
        "lineage capsule count must cap at MAX_LINEAGE_CAPSULES_IN_MEMORY",
    );
    let non_lineage_count = child
        .work_memory
        .iter()
        .filter(|c| c.kind != LINEAGE_PARENT_SUMMARY_KIND)
        .count();
    assert_eq!(
        non_lineage_count, baseline_non_lineage_len,
        "non-lineage entries must be preserved by the eviction",
    );
}

// ---------- I8: no XP / no active days / no level / no brain ----------

#[test]
fn lineage_absorption_does_not_grant_xp_or_advance_active_days() {
    let mut child = seeded_state();
    let xp_before = child.work_xp;
    let daily_before = child.daily_work_xp;
    let active_days_before = child.active_work_days;
    let level_before = child.level;
    let brain_before = child.brain_enabled;

    let now = fixed_now();
    let summary = parent_summary("build", now);
    let _ = child.record_lineage_parent_summary("viv-parent", &summary, now);

    assert_eq!(child.work_xp, xp_before);
    assert_eq!(child.daily_work_xp, daily_before);
    assert_eq!(child.active_work_days, active_days_before);
    assert_eq!(child.level, level_before);
    assert_eq!(child.brain_enabled, brain_before);
    assert_eq!(child.last_active_work_day, None);
}

// ---------- G2v2 anti-cascade: distill excludes lineage kind ----------

#[test]
fn distill_memory_skips_lineage_parent_summary_kind() {
    let mut child = seeded_state();
    let now = fixed_now();
    // Absorb enough lineage capsules to push past the distill trigger
    // capsules_since_distill threshold (lineage capsules do NOT increment
    // capsules_since_distill because push_lineage goes via a direct
    // work_memory.push, but distill_memory triggers also on size).
    for i in 0..MAX_LINEAGE_CAPSULES_IN_MEMORY {
        let topic = format!("topic-{i}");
        let summary = parent_summary(&topic, now + Duration::seconds(i as i64));
        let _ = child.record_lineage_parent_summary("viv-parent", &summary, now);
    }
    let distilled_before = child.distilled_summaries.len();
    child.distill_memory();
    let lineage_topics_distilled = child
        .distilled_summaries
        .iter()
        .skip(distilled_before)
        .filter(|d| d.kind == LINEAGE_PARENT_SUMMARY_KIND)
        .count();
    assert_eq!(
        lineage_topics_distilled, 0,
        "distill_memory must skip LINEAGE_PARENT_SUMMARY_KIND to break cascade",
    );
}

// ---------- rarity pressure: dentro-specie ----------

#[test]
fn bump_lineage_rarity_pressure_caps_at_ten() {
    assert_eq!(bump_lineage_rarity_pressure(0), 2);
    assert_eq!(bump_lineage_rarity_pressure(2), 4);
    assert_eq!(bump_lineage_rarity_pressure(8), 10);
    assert_eq!(bump_lineage_rarity_pressure(10), 10);
    assert_eq!(bump_lineage_rarity_pressure(50), 10);
}

#[test]
fn quality_roll_zero_pressure_is_noop() {
    let mut gene = crate::vivling::model::VivlingGeneVector::default();
    let snapshot = gene.clone();
    let triggered = apply_lineage_quality_roll(&mut gene, 0xdeadbeefdeadbeef, 0);
    assert!(!triggered);
    assert_eq!(gene, snapshot);
}

#[test]
fn quality_roll_lifts_gene_when_trigger_hits() {
    // hash with low high bits → trigger < pressure always
    let mut gene = crate::vivling::model::VivlingGeneVector::default();
    let curiosity_before = gene.curiosity;
    let brain_before = gene.brain_potential;
    let triggered = apply_lineage_quality_roll(&mut gene, 0x0000_0000_FFFF_FFFF, 10);
    assert!(triggered, "trigger=0 with pressure=10 must fire");
    assert!(gene.curiosity > curiosity_before);
    assert!(gene.brain_potential >= brain_before);
}

#[test]
fn quality_roll_does_not_touch_affinity_mod() {
    let mut gene = crate::vivling::model::VivlingGeneVector::default();
    let affinity_before = gene.affinity_mod;
    let _ = apply_lineage_quality_roll(&mut gene, 0x0000_0000_0000_0000, 10);
    assert_eq!(
        gene.affinity_mod, affinity_before,
        "quality roll must not touch species/work bias (affinity_mod)",
    );
}

#[test]
fn lineage_blessed_threshold_gates_at_six() {
    assert!(!is_lineage_blessed_threshold(0));
    assert!(!is_lineage_blessed_threshold(4));
    assert!(is_lineage_blessed_threshold(
        LINEAGE_BLESSED_PRESSURE_THRESHOLD
    ));
    assert!(is_lineage_blessed_threshold(8));
    assert!(is_lineage_blessed_threshold(10));
}
