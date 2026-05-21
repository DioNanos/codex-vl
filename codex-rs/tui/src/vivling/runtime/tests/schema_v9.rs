//! Memory V2 Step 2.A schema scaffolding tests.
//!
//! These are pure data-model tests: they verify that the V9 schema bump
//! preserves backward compatibility with V8 JSON, that newly-spawned
//! Vivlings stamp `state.version = 9`, that the language-state default
//! matches the design (`MirrorUser`), and that the volatile cache fields
//! never leak into the on-disk snapshot.
//!
//! Runtime wiring of the new fields lives in later steps; nothing here
//! exercises the memory agent, brain inheritance, or axes A-G.

use super::common::*;
use codex_vivling_core::model::VERSION as CURRENT_STATE_VERSION;
use codex_vivling_core::model::VivlingLanguageMode;

#[test]
fn new_state_uses_current_schema_version() {
    // Memory V2 Step 12.B.A bumped the schema to 10. Step 2.A's earlier
    // pin of `CURRENT_STATE_VERSION == 9` is now obsolete; this test
    // just pins that newly-spawned Vivlings stamp whatever the current
    // schema version is. The exact value is checked from
    // `schema_v10::current_schema_version_is_10`.
    let state = seeded_state();
    assert_eq!(state.version, CURRENT_STATE_VERSION);
}

#[test]
fn v8_state_loads_in_v9_binary_with_defaults() {
    // Minimal V8-shaped JSON. None of the V2 fields are present; serde
    // defaults must fill them all in without complaint.
    let v8_json = r#"{
        "version": 8,
        "hatched": true,
        "vivling_id": "viv-v8-fixture",
        "primary_vivling_id": "viv-v8-fixture",
        "species": "syllo",
        "rarity": "common",
        "name": "Veroniq",
        "level": 5,
        "xp": 240,
        "work_xp": 240,
        "active_work_days": 4
    }"#;

    let state: VivlingState =
        serde_json::from_str(v8_json).expect("V8 JSON must deserialize into V9 VivlingState");

    // V8-era fields preserved.
    assert_eq!(state.version, 8, "load must not silently rewrite version");
    assert_eq!(state.vivling_id, "viv-v8-fixture");
    assert_eq!(state.level, 5);
    assert_eq!(state.work_xp, 240);

    // V2 scaffold fields default cleanly.
    assert!(state.self_voice.is_none());
    assert!(state.lineage_inheritance.is_none());
    assert!(state.cached_crt_phrase.is_none());
    assert!(state.cached_proactive.is_none());
    assert_eq!(state.accumulated_bias.caution, 0);
    assert_eq!(state.accumulated_bias.verification, 0);
    assert_eq!(state.recent_bias.caution, 0);
    assert_eq!(state.recent_bias.verification, 0);
    assert_eq!(state.language_state.detected_language, "");
    assert!(state.language_state.language_override.is_none());
    assert!(state.language_state.recent_samples.is_empty());
}

#[test]
fn language_state_default_is_mirror_user() {
    let state = seeded_state();
    assert_eq!(
        state.language_state.language_mode,
        VivlingLanguageMode::MirrorUser,
        "design §8.2 P2.10 q.3: MirrorUser is the default mode"
    );
}

#[test]
fn cached_crt_phrase_is_skipped_on_serialize() {
    use codex_vivling_core::model::CachedCrtPhrase;
    use codex_vivling_core::model::CachedProactive;

    let mut state = seeded_state();
    state.cached_crt_phrase = Some(CachedCrtPhrase {
        text: "design V2 memoria".to_string(),
        generated_at: None,
        prompt_hash: None,
        ttl_expires_at: None,
    });
    state.cached_proactive = Some(CachedProactive {
        text: "monitor release codex".to_string(),
        generated_at: None,
        prompt_hash: None,
        ttl_expires_at: None,
    });

    let serialized = serde_json::to_string(&state).expect("serialize");
    assert!(
        !serialized.contains("cached_crt_phrase"),
        "volatile cache must not leak into on-disk JSON: {serialized}"
    );
    assert!(
        !serialized.contains("cached_proactive"),
        "volatile cache must not leak into on-disk JSON: {serialized}"
    );
    assert!(!serialized.contains("design V2 memoria"));
    assert!(!serialized.contains("monitor release codex"));
}
