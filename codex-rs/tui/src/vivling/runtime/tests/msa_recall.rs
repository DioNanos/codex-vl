//! MSA injection recall (design 2026-06-05 v2): original-text injection for
//! the brain prompt, ephemeral interleave state, saturation reset policy.

use chrono::Utc;
use tempfile::TempDir;

use crate::vivling::model::VivlingWorkMemoryEntry;
use crate::vivling::model::WorkArchetype;
use crate::vivling::runtime::msa::VivlingMsa;

fn capsule(kind: &str, summary: &str) -> VivlingWorkMemoryEntry {
    VivlingWorkMemoryEntry {
        kind: kind.to_string(),
        archetype: WorkArchetype::Builder,
        summary: summary.to_string(),
        weight: 1,
        created_at: Utc::now(),
    }
}

/// The injection lever: the recall section must carry the ORIGINAL capsule
/// text in full (bounded by the per-doc cap), not the legacy 96-char snippet.
#[test]
fn recall_section_injects_original_text() {
    let storage = TempDir::new().expect("msa storage tempdir");
    let msa = VivlingMsa::open_for_tests(storage.path());
    let vid = "viv-recall-1";

    // Summary well beyond the legacy 96-char snippet truncation, under the
    // 1500-char per-doc cap: full text must survive injection verbatim.
    let long_tail = "the fix lives in retry_backoff and the answer token is QUARZO".to_string();
    let summary = format!(
        "Investigated the flaky websocket reconnect for three hours; {} {}",
        "intermediate findings were recorded step by step in the work log; ".repeat(3),
        long_tail
    );
    assert!(summary.len() > 96, "fixture must exceed snippet truncation");
    msa.index_capsule(vid, &capsule("turn", &summary));

    let section = msa
        .recall_section(vid, "websocket reconnect retry_backoff")
        .expect("injection section");
    assert!(section.starts_with("Relevant memory (original text):"));
    // The discriminating tail lives past the 96-char snippet horizon.
    assert!(
        section.contains("QUARZO"),
        "original text must be injected in full, got: {section}"
    );
}

/// No indexed capsules → no injected docs → `None`, so the brain prompt falls
/// back to the legacy path and the tick always composes.
#[test]
fn recall_section_returns_none_without_matches() {
    let storage = TempDir::new().expect("msa storage tempdir");
    let msa = VivlingMsa::open_for_tests(storage.path());
    assert!(msa.recall_section("viv-empty", "anything at all").is_none());
}

/// Reset policy (design v2): with a repeated identical payload, round 1
/// injects, later rounds dedup to empty (None → legacy fallback), and once
/// the state saturates `max_rounds` the adapter resets and injects again —
/// injection can never be permanently disabled.
#[test]
fn recall_saturation_resets_and_recovers() {
    let storage = TempDir::new().expect("msa storage tempdir");
    let msa = VivlingMsa::open_for_tests(storage.path());
    let vid = "viv-saturate";
    msa.index_capsule(vid, &capsule("turn", "alpha beta gamma delta payload memory"));

    let payload = "alpha beta gamma payload";
    assert!(
        msa.recall_section(vid, payload).is_some(),
        "round 1 must inject"
    );

    // Burn rounds: the same payload dedups to empty each round until the
    // round counter saturates (RECALL_MAX_ROUNDS = 8).
    let mut recovered = false;
    for _ in 0..16 {
        if msa.recall_section(vid, payload).is_some() {
            recovered = true;
            break;
        }
    }
    assert!(
        recovered,
        "after saturation the state must reset and inject again"
    );

    // Explicit reset hook (work-memory rotation trigger) re-arms immediately.
    msa.reset_interleave_state(vid);
    assert!(
        msa.recall_section(vid, payload).is_some(),
        "fresh state after explicit reset must inject"
    );
}
