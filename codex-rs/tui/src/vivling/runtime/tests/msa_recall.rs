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

// --- Gate "capsule ricche" (design 2026-06-05, audit Codex APPROVE) ---

fn rich_source_with(marker: &str) -> String {
    format!(
        "Analisi completa del bug nel modulo websocket: la backoff window cresceva senza \
         limite perche' il moltiplicatore non veniva azzerato dopo un successo. Il fix vive \
         nella funzione di reconnect e il marcatore distintivo del dettaglio e' {marker}, \
         che si trova ben oltre il taglio dei centoventi caratteri del summary."
    )
}

/// Gate (b): senza sorgente, o con sorgente low-signal, il documento resta
/// summary-only (layout v1) con metadata coerente.
#[test]
fn rich_capsule_falls_back_summary_only() {
    let storage = TempDir::new().expect("msa storage tempdir");
    let msa = VivlingMsa::open_for_tests(storage.path());
    let vid = "viv-rich-fallback";
    let cap = capsule("turn", "turn completed: lavoro breve di routine");

    msa.index_capsule_rich(vid, &cap, None);
    msa.index_capsule_rich(vid, &cap, Some("ok")); // low-signal

    let idx = msa.collection_for(vid).expect("collection");
    let hits = idx.search("routine", 3, None).expect("search");
    assert!(!hits.is_empty());
    let doc = idx.fetch_doc(&hits[0].doc_id).expect("fetch");
    assert_eq!(doc.text, cap.summary, "low-signal deve restare summary-only");
    assert_eq!(doc.metadata.get("rich"), Some(&serde_json::json!(false)));
    assert_eq!(
        doc.metadata.get("index_text_version"),
        Some(&serde_json::json!(2))
    );
}

/// Gate (a, adapter) + (c): il detail oltre il taglio 120c e' retrievabile e
/// arriva nella sezione di recall; la capsule (stato) non lo contiene.
#[test]
fn rich_capsule_detail_recallable_beyond_truncation() {
    let storage = TempDir::new().expect("msa storage tempdir");
    let msa = VivlingMsa::open_for_tests(storage.path());
    let vid = "viv-rich-recall";
    let source = rich_source_with("SMERALDO99");
    let cap = capsule("turn", &format!("turn completed: {}", &source[..120]));
    assert!(!cap.summary.contains("SMERALDO99"));

    msa.index_capsule_rich(vid, &cap, Some(&source));

    // (c) la query sul contenuto del detail trova il documento e la sezione
    // di recall porta il marcatore al brain.
    let section = msa
        .recall_section(vid, "marcatore distintivo SMERALDO99 reconnect")
        .expect("injection section");
    assert!(section.contains("SMERALDO99"), "{section}");
    assert!(section.contains("detail:"), "{section}");

    // (a) la capsule resta corta: il detail e' solo un artifact dell'indice.
    assert!(!cap.summary.contains("SMERALDO99"));
}

/// Gate (d): un segreto simulato nel sorgente NON deve mai essere
/// retrievabile, ne' via search ne' via recall_section.
#[test]
fn rich_capsule_never_indexes_secrets() {
    let storage = TempDir::new().expect("msa storage tempdir");
    let msa = VivlingMsa::open_for_tests(storage.path());
    let vid = "viv-rich-privacy";
    let source = format!(
        "{} Durante il deploy ho usato Authorization: Bearer FAKESECRET1234567890abc e \
         api_key=NOTAREALKEY99 per il test.",
        rich_source_with("CONTESTO")
    );
    let cap = capsule("turn", "turn completed: deploy con credenziali di test");

    msa.index_capsule_rich(vid, &cap, Some(&source));

    let idx = msa.collection_for(vid).expect("collection");
    for secret in ["FAKESECRET1234567890abc", "NOTAREALKEY99"] {
        let hits = idx.search(secret, 3, None).expect("search");
        assert!(hits.is_empty(), "segreto retrievabile via search: {secret}");
        assert!(
            msa.recall_section(vid, secret).is_none(),
            "segreto retrievabile via recall: {secret}"
        );
        msa.reset_interleave_state(vid);
    }
    // Il resto del detail invece c'e' (la sanitizzazione non ha buttato tutto).
    let hits = idx.search("deploy", 3, None).expect("search");
    assert!(!hits.is_empty());
}

/// Ingest gate (live audit 2026-06-07, F1): bookkeeping kinds must NOT reach
/// the MSA archive — a 6-week-old vivling's index was 78% loop noise. Turn
/// capsules keep flowing; the recall section never serves bookkeeping.
#[test]
fn ingest_gate_drops_bookkeeping_kinds() {
    let storage = TempDir::new().expect("msa storage tempdir");
    let msa = VivlingMsa::open_for_tests(storage.path());
    let vid = "viv-gate-1";

    for kind in [
        "live_context",
        "loop_runtime",
        "loop_config",
        "loop_profile",
        "loop_blocked_busy",
        "loop_blocked_review",
        "loop_blocked_side",
    ] {
        msa.index_capsule(
            vid,
            &capsule(kind, "loop bookkeeping about the quarzo websocket task"),
        );
    }
    assert!(
        msa.recall_section(vid, "quarzo websocket").is_none(),
        "bookkeeping must not be recallable"
    );

    // Knowledge still flows: same wording, kind=turn.
    msa.index_capsule(
        vid,
        &capsule("turn", "fixed the quarzo websocket reconnect for good"),
    );
    let section = msa
        .recall_section(vid, "quarzo websocket")
        .expect("turn capsule must be recallable");
    assert!(section.contains("reconnect"));
}
