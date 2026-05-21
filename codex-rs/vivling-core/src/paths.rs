//! Deterministic Vivling file path helpers.
//!
//! Paths are derived at runtime from a `roster_dir: &Path` argument and never
//! serialized into the on-disk state JSON. This keeps Vivling state portable
//! between devices (e.g. exported from VPS3, imported on Pixel9Pro) and
//! independent of filesystem layout differences.

use std::path::Path;
use std::path::PathBuf;

/// Sidecar file with the Vivling's `VivlingSkill` collection.
pub fn skills_file_path(roster_dir: &Path, vivling_id: &str) -> PathBuf {
    roster_dir.join(format!("{vivling_id}_skills.json"))
}

/// Mirror markdown copy of `self_voice` for read-friendly inspection by the user.
pub fn voice_file_path(roster_dir: &Path, vivling_id: &str) -> PathBuf {
    roster_dir.join(format!("{vivling_id}_voice.md"))
}

/// One-shot pre-migration backup of the legacy state JSON. Created exactly
/// once when a Vivling transitions across a schema version boundary
/// (e.g. v8 -> v9). Never overwritten.
pub fn pre_migration_backup_path(
    roster_dir: &Path,
    vivling_id: &str,
    from_version: u32,
) -> PathBuf {
    roster_dir.join(format!("{vivling_id}.json.v{from_version}.bak"))
}

/// Rotational last-write backup. Overwritten on every safe save. Used to
/// recover from a single bad memory-agent run.
pub fn last_write_backup_path(roster_dir: &Path, vivling_id: &str) -> PathBuf {
    roster_dir.join(format!("{vivling_id}.json.bak"))
}

/// Per-Vivling advisory lock file path. Held during the entire memory-agent
/// transaction to prevent races with the TUI save path.
pub fn lock_file_path(roster_dir: &Path, vivling_id: &str) -> PathBuf {
    roster_dir.join(format!("{vivling_id}.json.lock"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn roster() -> PathBuf {
        PathBuf::from("/tmp/codex_vivlings")
    }

    #[test]
    fn skills_path_is_deterministic() {
        let p = skills_file_path(&roster(), "viv-13ba0093");
        assert_eq!(
            p,
            PathBuf::from("/tmp/codex_vivlings/viv-13ba0093_skills.json")
        );
    }

    #[test]
    fn voice_path_is_deterministic() {
        let p = voice_file_path(&roster(), "viv-13ba0093");
        assert_eq!(
            p,
            PathBuf::from("/tmp/codex_vivlings/viv-13ba0093_voice.md")
        );
    }

    #[test]
    fn pre_migration_backup_path_encodes_version() {
        let p = pre_migration_backup_path(&roster(), "viv-13ba0093", 8);
        assert_eq!(
            p,
            PathBuf::from("/tmp/codex_vivlings/viv-13ba0093.json.v8.bak")
        );
    }

    #[test]
    fn last_write_and_lock_paths_distinct_from_state() {
        let lw = last_write_backup_path(&roster(), "viv-13ba0093");
        let lk = lock_file_path(&roster(), "viv-13ba0093");
        assert_ne!(lw, lk);
        assert!(lw.to_string_lossy().ends_with(".json.bak"));
        assert!(lk.to_string_lossy().ends_with(".json.lock"));
    }

    #[test]
    fn paths_are_relative_to_roster_dir() {
        // Same vivling_id under two different roster dirs must yield different paths.
        let a = skills_file_path(&PathBuf::from("/a"), "viv-1");
        let b = skills_file_path(&PathBuf::from("/b"), "viv-1");
        assert_ne!(a, b);
    }
}
