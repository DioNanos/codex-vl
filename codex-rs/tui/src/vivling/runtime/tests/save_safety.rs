//! Step 1.C safety-primitive smoke tests for the Vivling save path.
//!
//! Verifies that the per-Vivling sidecar files documented in design §11.2
//! actually land on disk where `codex_vivling_core::paths` says they
//! should after the TUI save path completes:
//!
//! - `<vivling_id>.json.bak` is created on the **second** save (last-write
//!   rotation; the first save has nothing to rotate).
//! - `<vivling_id>.json.lock` is created next to the state JSON when the
//!   save path acquires its advisory file lock.

use super::common::*;
use codex_vivling_core::paths::last_write_backup_path;
use codex_vivling_core::paths::lock_file_path;
use codex_vivling_core::paths::pre_migration_backup_path;
use std::fs;

#[test]
fn second_save_creates_last_write_backup() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let vivling_id = vivling
        .state
        .as_ref()
        .expect("hatched state")
        .vivling_id
        .clone();
    let roster_dir = vivling.roster_dir().expect("roster_dir");
    let backup = last_write_backup_path(&roster_dir, &vivling_id);

    // First save (already performed by hatch) — backup must not yet exist
    // because there was no prior state file to rotate.
    assert!(
        !backup.exists(),
        "first save should not produce a backup; got {}",
        backup.display()
    );

    // Touch the state so the second save has something different to write,
    // then save again. The previous state file rotates into `.json.bak`.
    vivling.state.as_mut().expect("hatched state").meals = 1;
    vivling.save_state().expect("second save");

    assert!(
        backup.exists(),
        "second save must produce a .json.bak at {}",
        backup.display()
    );
}

#[test]
fn save_creates_lock_file_in_roster_dir() {
    let temp = TempDir::new().expect("tempdir");
    let vivling = hatched_vivling(temp.path());
    let vivling_id = vivling
        .state
        .as_ref()
        .expect("hatched state")
        .vivling_id
        .clone();
    let roster_dir = vivling.roster_dir().expect("roster_dir");
    let lock = lock_file_path(&roster_dir, &vivling_id);

    assert!(
        lock.exists(),
        "save path must create the per-Vivling lock file at {}",
        lock.display()
    );
}

// --- Memory V2 Step 2.B: pre-migration backup tests ---

/// Write the minimum V8-shaped state JSON we need to exercise the
/// pre-migration code path: a real vivling_id plus the legacy version
/// number. Everything else is filled in by serde defaults at load time.
fn write_v8_state_on_disk(roster_dir: &std::path::Path, vivling_id: &str) {
    fs::create_dir_all(roster_dir).expect("roster dir");
    let path = roster_dir.join(format!("{vivling_id}.json"));
    let json = format!(
        r#"{{
            "version": 8,
            "hatched": true,
            "vivling_id": "{vivling_id}",
            "primary_vivling_id": "{vivling_id}",
            "species": "syllo",
            "rarity": "common",
            "name": "LegacyV8",
            "level": 5
        }}"#
    );
    fs::write(&path, json).expect("write v8 state");
    // The roster index must also list the vivling_id so the runtime
    // happily loads it as an existing entry instead of treating it as
    // a fresh spawn.
    let roster_path = roster_dir.join("roster.json");
    fs::write(
        &roster_path,
        format!(
            r#"{{"version":8,"active_vivling_id":"{vivling_id}","vivling_ids":["{vivling_id}"],"external_vivling_ids":[]}}"#
        ),
    )
    .expect("write roster");
}

#[test]
fn save_with_v8_on_disk_creates_v8_bak() {
    let temp = TempDir::new().expect("tempdir");
    let vivling_id = "viv-v8-fixture";
    let mut vivling = configured_vivling(temp.path());
    let roster_dir = vivling.roster_dir().expect("roster_dir");
    write_v8_state_on_disk(&roster_dir, vivling_id);

    // Load and re-save: the on-disk version (8) is < VERSION (9), so the
    // pre-migration backup must land before the new JSON is written.
    let mut state = vivling
        .load_state_for_id(vivling_id)
        .expect("load v8")
        .expect("v8 state present");
    state.version = 8; // load may have already normalised version; keep the test honest
    vivling.state = Some(state);
    vivling.active_vivling_id = Some(vivling_id.to_string());
    vivling.save_state().expect("save v8 -> v9");

    let pre_bak = pre_migration_backup_path(&roster_dir, vivling_id, 8);
    assert!(
        pre_bak.exists(),
        "save with v8 on disk must create {}",
        pre_bak.display()
    );
    let body = fs::read_to_string(&pre_bak).expect("read v8 bak");
    assert!(
        body.contains("\"version\": 8") || body.contains("\"version\":8"),
        "pre-migration backup must preserve the legacy schema; got {body}"
    );
}

#[test]
fn second_save_does_not_overwrite_pre_migration_bak() {
    let temp = TempDir::new().expect("tempdir");
    let vivling_id = "viv-v8-once";
    let mut vivling = configured_vivling(temp.path());
    let roster_dir = vivling.roster_dir().expect("roster_dir");
    write_v8_state_on_disk(&roster_dir, vivling_id);

    let mut state = vivling
        .load_state_for_id(vivling_id)
        .expect("load v8")
        .expect("v8 state present");
    state.version = 8;
    vivling.state = Some(state);
    vivling.active_vivling_id = Some(vivling_id.to_string());

    // First save: writes V9, captures the V8 snapshot.
    vivling.save_state().expect("first save");
    let pre_bak = pre_migration_backup_path(&roster_dir, vivling_id, 8);
    let captured = fs::read_to_string(&pre_bak).expect("read v8 bak");

    // Mutate the in-memory state and save again. The on-disk file is
    // now V9, so the second save must NOT touch the V8 backup.
    vivling.state.as_mut().expect("state").meals = 99;
    vivling.save_state().expect("second save");

    let still = fs::read_to_string(&pre_bak).expect("read v8 bak after second save");
    assert_eq!(
        captured, still,
        "pre-migration backup must be one-shot; got rewritten on second save"
    );
}

#[test]
fn first_save_of_new_vivling_creates_no_pre_migration_bak() {
    let temp = TempDir::new().expect("tempdir");
    let vivling = hatched_vivling(temp.path());
    let vivling_id = vivling
        .state
        .as_ref()
        .expect("hatched state")
        .vivling_id
        .clone();
    let roster_dir = vivling.roster_dir().expect("roster_dir");

    // A newly-hatched Vivling stamps version 9 from the start, so no
    // pre-migration snapshot should ever be produced.
    for from_version in [0, 6, 7, 8] {
        let pre_bak = pre_migration_backup_path(&roster_dir, &vivling_id, from_version);
        assert!(
            !pre_bak.exists(),
            "fresh hatch must not produce {}",
            pre_bak.display()
        );
    }
}

#[test]
fn last_write_bak_remains_separate_from_pre_migration_bak() {
    let temp = TempDir::new().expect("tempdir");
    let vivling_id = "viv-v8-twin";
    let mut vivling = configured_vivling(temp.path());
    let roster_dir = vivling.roster_dir().expect("roster_dir");
    write_v8_state_on_disk(&roster_dir, vivling_id);

    let mut state = vivling
        .load_state_for_id(vivling_id)
        .expect("load v8")
        .expect("v8 state present");
    state.version = 8;
    vivling.state = Some(state);
    vivling.active_vivling_id = Some(vivling_id.to_string());
    vivling.save_state().expect("save v8 -> v9");

    let last_write = last_write_backup_path(&roster_dir, vivling_id);
    let pre_bak = pre_migration_backup_path(&roster_dir, vivling_id, 8);
    assert!(
        last_write.exists(),
        "Step 1.C rotational backup must still land: {}",
        last_write.display()
    );
    assert!(
        pre_bak.exists(),
        "Step 2.B pre-migration backup must land alongside: {}",
        pre_bak.display()
    );
    assert_ne!(last_write, pre_bak);
}
