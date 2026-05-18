//! codex-vl iter 1C — spawn output (L1) + ZED Lineage panel (L2) tests.
//!
//! Coverage:
//! - `/vivling spawn` returns a `SpawnNarration` outcome carrying both a
//!   chat-history message (L1) and a ZED Lineage `VivlingPanelData` (L2);
//! - the message mentions parent name, child name, origin, species,
//!   and explicitly states the child stays inactive;
//! - the ZED Lineage panel title and lines mention the parent and child
//!   names and never leak raw distilled summary text;
//! - the panel keeps the brain/Chat/Loop-off contract visible.

use super::common::*;
use crate::vivling::VivlingPanelData;
use crate::vivling::zed::ZedTopic;
use crate::vivling::zed::zed_panel_data;
use crate::vivling::zed::zed_summary_for_lineage;
use crate::vivling::zed::zed_summary_for_topic;

#[test]
fn zed_lineage_topic_summary_mentions_lineage_and_inactivity() {
    let summary = zed_summary_for_topic(ZedTopic::Lineage);
    assert!(
        summary.contains("lineage"),
        "must call out the lineage event"
    );
    assert!(
        summary.contains("species") && summary.contains("culture"),
        "must restate bio-vs-cultural split",
    );
}

#[test]
fn zed_summary_for_lineage_includes_parent_child_species_and_inactive_contract() {
    let summary = zed_summary_for_lineage("Nilo", "Kira", "syllo", "primary_child");
    assert!(summary.contains("Nilo"), "must include parent name");
    assert!(summary.contains("Kira"), "must include child name");
    assert!(summary.contains("syllo"), "must include species id");
    assert!(
        summary.contains("inactive"),
        "must state that the newborn stays inactive",
    );
    assert!(
        !summary.contains("Brain") || summary.contains("no Brain") || summary.contains("inactive"),
        "must NOT promise Brain ownership",
    );
}

#[test]
fn zed_summary_for_lineage_branches_by_origin_label() {
    let primary_msg = zed_summary_for_lineage("Nilo", "Kira", "syllo", "primary_child");
    let veteran_msg = zed_summary_for_lineage("Nilo", "Kira", "syllo", "veteran_child");
    let zed_msg = zed_summary_for_lineage("Nilo", "Kira", "orchestra", "zed_hatch");
    assert!(primary_msg.contains("primary"));
    assert!(veteran_msg.contains("veteran"));
    assert!(
        zed_msg.contains("ZED introduces") || zed_msg.contains("new bloodline"),
        "ZED origin must be narrated as a new bloodline",
    );
}

#[test]
fn zed_lineage_panel_title_and_lines_render() {
    let summary = zed_summary_for_lineage("Nilo", "Kira", "syllo", "primary_child");
    let panel = zed_panel_data(ZedTopic::Lineage, &summary);
    assert!(
        panel.title.contains("Lineage"),
        "panel title must mark the topic: got {}",
        panel.title
    );
    let narrow = panel.narrow_lines.join("\n");
    assert!(
        narrow.contains("Kira"),
        "panel narrow lines must show the child"
    );
    assert!(
        narrow.contains("Nilo"),
        "panel narrow lines must show the parent"
    );
}

#[test]
fn spawn_vivling_returns_message_and_zed_lineage_panel() {
    let codex_home = TempDir::new().expect("codex home tempdir");
    let mut vivling = hatched_vivling(codex_home.path());
    set_active_level(&mut vivling, JUVENILE_LEVEL);

    let (message, panel): (String, VivlingPanelData) =
        vivling.spawn_vivling().expect("spawn should succeed");

    // L1 — chat-history message
    assert!(message.starts_with("Spawned "));
    assert!(
        message.contains("via "),
        "message must mention origin label"
    );
    assert!(
        message.contains("Child stays inactive"),
        "message must state child stays inactive: {message}",
    );
    let primary_name = vivling.state.as_ref().expect("primary").name.clone();
    assert!(
        message.contains(&primary_name),
        "message must include cultural parent name: {message}",
    );

    // L2 — ZED Lineage panel
    assert!(
        panel.title.contains("Lineage"),
        "panel title must be ZED Lineage: {}",
        panel.title,
    );
    let narrow = panel.narrow_lines.join("\n");
    assert!(narrow.contains(&primary_name));
    assert!(
        narrow.contains("inactive"),
        "panel must keep the no-Brain/no-Loop contract visible",
    );
}

#[test]
fn spawn_message_does_not_promise_brain_or_chat() {
    let codex_home = TempDir::new().expect("codex home tempdir");
    let mut vivling = hatched_vivling(codex_home.path());
    set_active_level(&mut vivling, JUVENILE_LEVEL);

    let (message, _panel) = vivling.spawn_vivling().expect("spawn should succeed");
    let lower = message.to_ascii_lowercase();
    assert!(
        !lower.contains("brain on") && !lower.contains("chat with"),
        "spawn message must NOT promise Brain/Chat surface: {message}",
    );
}
