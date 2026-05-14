//! codex-vl lineage passive learning runtime tests (Fase 4 iter 1A).
//!
//! Cover the `propagate_parent_summaries_to_children` runtime hook
//! end-to-end with a real `Vivling` instance on a `TempDir` codex home:
//!
//! - cultural_parent match absorbs;
//! - legacy fallback to `parent_vivling_id` when cultural is `None`;
//! - active id invariato after the propagation cycle;
//! - imported child is skipped;
//! - cooldown skip when a recent lineage capsule exists on the child;
//! - grandchild cascade blocked (distillation excludes the lineage
//!   `kind`, so the child cannot re-propagate the parent's wisdom).

use super::common::*;
use chrono::Duration;
use chrono::TimeZone;
use chrono::Utc;

use crate::vivling::model::VivlingDistilledSummary;
use crate::vivling::model::VivlingWorkMemoryEntry;
use crate::vivling::model::WorkArchetype;
use crate::vivling::model::lineage::LINEAGE_PARENT_SUMMARY_KIND;

fn anchor() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 15, 14, 0, 0).unwrap()
}

fn parent_summary(topic: &str) -> VivlingDistilledSummary {
    let now = anchor();
    VivlingDistilledSummary {
        topic: topic.to_string(),
        summary: format!("observed {topic} pattern (lineage)"),
        kind: "turn".to_string(),
        archetype: WorkArchetype::Builder,
        total_weight: 5,
        observations: 3,
        first_seen_at: now - Duration::hours(2),
        last_seen_at: now,
    }
}

/// Create a child state on disk under the primary's lineage, with the
/// requested cultural-parent wiring. Returns the child id.
fn install_child(
    vivling: &Vivling,
    primary: &VivlingState,
    child_label: &str,
    cultural_parent: Option<String>,
    bio_parent: Option<String>,
    is_imported: bool,
) -> String {
    let mut child = primary.clone();
    child.vivling_id = format!("viv-child-{child_label}");
    child.is_primary = false;
    child.is_imported = is_imported;
    child.instance_label = Some(child_label.to_string());
    child.cultural_parent_vivling_id = cultural_parent;
    child.parent_vivling_id = bio_parent;
    child.work_memory.clear();
    child.distilled_summaries.clear();
    child.lineage_seen_parent_summary_keys.clear();
    vivling
        .save_state_record(&child, /*set_active*/ false, is_imported)
        .expect("save child");
    child.vivling_id
}

/// Set up a hatched primary with two distilled summaries ready to
/// propagate.
fn primary_with_distillates(home: &std::path::Path) -> Vivling {
    let mut vivling = hatched_vivling(home);
    set_active_level(&mut vivling, JUVENILE_LEVEL);
    let mut state = vivling.state.clone().expect("active state");
    state.is_primary = true;
    state.distilled_summaries = vec![parent_summary("build"), parent_summary("review")];
    vivling.state = Some(state.clone());
    vivling.save_state().expect("save primary distillates");
    vivling
}

#[test]
fn child_with_cultural_parent_match_absorbs_lineage() {
    let home = TempDir::new().expect("tempdir");
    let vivling = primary_with_distillates(home.path());
    let primary = vivling.state.clone().expect("primary");
    let child_id = install_child(
        &vivling,
        &primary,
        "cultural",
        Some(primary.vivling_id.clone()),
        Some(primary.vivling_id.clone()),
        /*is_imported*/ false,
    );

    let mut vivling = vivling;
    let report = vivling
        .propagate_parent_summaries_to_children()
        .expect("propagate ok");
    assert_eq!(report.children_visited, 1);
    assert!(report.capsules_absorbed >= 1);

    let child = vivling
        .load_state_for_id(&child_id)
        .expect("load child")
        .expect("child saved");
    let lineage_cnt = child
        .work_memory
        .iter()
        .filter(|c| c.kind == LINEAGE_PARENT_SUMMARY_KIND)
        .count();
    assert!(lineage_cnt >= 1);
    assert!(!child.lineage_seen_parent_summary_keys.is_empty());
}

#[test]
fn legacy_child_without_cultural_parent_falls_back_to_bio_parent() {
    let home = TempDir::new().expect("tempdir");
    let vivling = primary_with_distillates(home.path());
    let primary = vivling.state.clone().expect("primary");
    let child_id = install_child(
        &vivling,
        &primary,
        "legacy",
        /*cultural*/ None,
        /*bio*/ Some(primary.vivling_id.clone()),
        /*is_imported*/ false,
    );

    let mut vivling = vivling;
    let report = vivling
        .propagate_parent_summaries_to_children()
        .expect("propagate ok");
    assert_eq!(
        report.children_visited, 1,
        "legacy fallback on parent_vivling_id must apply",
    );

    let child = vivling
        .load_state_for_id(&child_id)
        .expect("load child")
        .expect("child saved");
    assert!(
        !child.lineage_seen_parent_summary_keys.is_empty(),
        "legacy child must absorb via bio-parent fallback",
    );
}

#[test]
fn propagation_keeps_active_id_invariant() {
    let home = TempDir::new().expect("tempdir");
    let vivling = primary_with_distillates(home.path());
    let primary = vivling.state.clone().expect("primary");
    let _ = install_child(
        &vivling,
        &primary,
        "cultural",
        Some(primary.vivling_id.clone()),
        Some(primary.vivling_id.clone()),
        false,
    );

    let mut vivling = vivling;
    let active_before = vivling.active_vivling_id.clone();
    let _ = vivling
        .propagate_parent_summaries_to_children()
        .expect("propagate ok");
    let active_after = vivling.active_vivling_id.clone();
    assert_eq!(
        active_before, active_after,
        "active_vivling_id must be invariato across propagation",
    );
    assert_eq!(active_after, Some(primary.vivling_id));
}

#[test]
fn imported_child_is_skipped_iter_1a() {
    let home = TempDir::new().expect("tempdir");
    let vivling = primary_with_distillates(home.path());
    let primary = vivling.state.clone().expect("primary");
    let child_id = install_child(
        &vivling,
        &primary,
        "imported",
        Some(primary.vivling_id.clone()),
        Some(primary.vivling_id.clone()),
        /*is_imported*/ true,
    );

    let mut vivling = vivling;
    let report = vivling
        .propagate_parent_summaries_to_children()
        .expect("propagate ok");
    assert_eq!(report.children_skipped_imported, 1);
    assert_eq!(report.capsules_absorbed, 0);

    let child = vivling
        .load_state_for_id(&child_id)
        .expect("load child")
        .expect("child saved");
    assert!(
        child.lineage_seen_parent_summary_keys.is_empty(),
        "imported child must not absorb in iter 1A",
    );
}

#[test]
fn child_in_cooldown_is_skipped() {
    let home = TempDir::new().expect("tempdir");
    let vivling = primary_with_distillates(home.path());
    let primary = vivling.state.clone().expect("primary");
    let mut child = primary.clone();
    child.vivling_id = "viv-child-cooldown".to_string();
    child.is_primary = false;
    child.is_imported = false;
    child.instance_label = Some("cooldown".to_string());
    child.cultural_parent_vivling_id = Some(primary.vivling_id.clone());
    child.parent_vivling_id = Some(primary.vivling_id.clone());
    child.work_memory.clear();
    child.distilled_summaries.clear();
    child.lineage_seen_parent_summary_keys.clear();
    // Plant a fresh lineage capsule to trigger the cooldown.
    child.work_memory.push(VivlingWorkMemoryEntry {
        kind: LINEAGE_PARENT_SUMMARY_KIND.to_string(),
        summary: "lineage:seed: fresh".to_string(),
        archetype: WorkArchetype::Builder,
        weight: 3,
        created_at: Utc::now(),
    });
    vivling
        .save_state_record(&child, /*set_active*/ false, false)
        .expect("save child");

    let mut vivling = vivling;
    let report = vivling
        .propagate_parent_summaries_to_children()
        .expect("propagate ok");
    assert_eq!(report.children_skipped_cooldown, 1);
    assert_eq!(report.capsules_absorbed, 0);
}

#[test]
fn grandchild_cascade_blocked_via_distill_filter() {
    // Simulate the cascade-block invariant at the layer where it
    // matters: the recipient child distills its own work_memory, and
    // the lineage capsules it absorbed must NOT graduate to
    // distilled_summaries — otherwise the child's next propagation
    // cycle would forward them to its own children.
    let home = TempDir::new().expect("tempdir");
    let vivling = primary_with_distillates(home.path());
    let primary = vivling.state.clone().expect("primary");
    let child_id = install_child(
        &vivling,
        &primary,
        "midgen",
        Some(primary.vivling_id.clone()),
        Some(primary.vivling_id.clone()),
        false,
    );

    let mut vivling = vivling;
    let _ = vivling
        .propagate_parent_summaries_to_children()
        .expect("propagate ok");

    let mut child = vivling
        .load_state_for_id(&child_id)
        .expect("load child")
        .expect("child saved");
    let lineage_count_before = child
        .work_memory
        .iter()
        .filter(|c| c.kind == LINEAGE_PARENT_SUMMARY_KIND)
        .count();
    assert!(lineage_count_before >= 1);

    let distilled_before = child.distilled_summaries.len();
    child.distill_memory();
    let new_distilled: Vec<_> = child
        .distilled_summaries
        .iter()
        .skip(distilled_before)
        .filter(|d| d.kind == LINEAGE_PARENT_SUMMARY_KIND)
        .collect();
    assert!(
        new_distilled.is_empty(),
        "distill_memory must skip LINEAGE_PARENT_SUMMARY_KIND so the \
         child cannot cascade absorbed lineage to its own offspring",
    );
}
