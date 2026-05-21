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
