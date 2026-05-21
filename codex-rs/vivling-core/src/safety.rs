//! Filesystem safety primitives for Vivling state persistence.
//!
//! Three guarantees:
//!
//! - **`acquire_lock`**: advisory `flock(LOCK_EX)` on a per-Vivling lock file
//!   with a bounded wait. Prevents the memory agent and the TUI from racing on
//!   the same `<vivling_id>.json`.
//! - **`write_atomic`**: write to a temp sibling and `rename(2)` into place, so
//!   a crash mid-write never produces a partial file.
//! - **`backup_pre_migration` / `backup_last_write`**: two distinct backup
//!   policies. Migration backup is one-shot per schema transition (never
//!   overwritten). Last-write backup rotates on every safe save and exists to
//!   recover from a single bad memory-agent run.

use std::fs;
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SafetyError {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("flock timed out after {waited:?} on {path}")]
    LockTimeout { path: PathBuf, waited: Duration },
}

impl SafetyError {
    fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        SafetyError::Io {
            path: path.into(),
            source,
        }
    }
}

/// RAII guard for an advisory file lock. The lock is released automatically
/// when the guard is dropped (the kernel releases `flock` on `close(2)`).
pub struct VivlingLockGuard {
    #[allow(dead_code)] // held to keep the fd alive until Drop
    file: fs::File,
    path: PathBuf,
}

impl VivlingLockGuard {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Acquire an exclusive advisory lock on `lock_path`. Polls with
/// `LOCK_EX | LOCK_NB` every 100ms until either the lock is granted or
/// `timeout` elapses. The lock file is created if missing.
pub fn acquire_lock(lock_path: &Path, timeout: Duration) -> Result<VivlingLockGuard, SafetyError> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|e| SafetyError::io(parent, e))?;
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|e| SafetyError::io(lock_path, e))?;
    let start = Instant::now();
    loop {
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            stamp_holder_pid(&file);
            return Ok(VivlingLockGuard {
                file,
                path: lock_path.to_path_buf(),
            });
        }
        let err = io::Error::last_os_error();
        // On Linux EWOULDBLOCK == EAGAIN, so a single arm is enough; on other
        // platforms the kernel may distinguish them but `flock` returns
        // EWOULDBLOCK for "already locked".
        if err.raw_os_error() != Some(libc::EWOULDBLOCK) {
            return Err(SafetyError::io(lock_path, err));
        }
        if start.elapsed() >= timeout {
            return Err(SafetyError::LockTimeout {
                path: lock_path.to_path_buf(),
                waited: start.elapsed(),
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
}

// Best-effort: stamp the holder PID so external operators can identify
// stale locks. Failure here must never fail the lock acquisition itself.
fn stamp_holder_pid(mut file: &fs::File) {
    use std::io::Write;
    let _ = writeln!(&mut file, "pid={}", process::id());
}

/// Write `contents` to `target` atomically by going through a temp sibling
/// and `rename(2)`. The parent directory is created if missing.
pub fn write_atomic(target: &Path, contents: &[u8]) -> Result<(), SafetyError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| SafetyError::io(parent, e))?;
    }
    let temp = temp_sibling(target);
    fs::write(&temp, contents).map_err(|e| SafetyError::io(&temp, e))?;
    fs::rename(&temp, target).map_err(|e| SafetyError::io(target, e))?;
    Ok(())
}

/// One-shot pre-migration backup. If `backup` already exists, do nothing
/// (preserving the earliest snapshot of the legacy schema).
pub fn backup_pre_migration(source: &Path, backup: &Path) -> Result<(), SafetyError> {
    if !source.exists() {
        return Ok(());
    }
    if backup.exists() {
        return Ok(());
    }
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent).map_err(|e| SafetyError::io(parent, e))?;
    }
    fs::copy(source, backup).map_err(|e| SafetyError::io(backup, e))?;
    Ok(())
}

/// Rotational last-write backup. Always overwrites the previous `backup`.
pub fn backup_last_write(source: &Path, backup: &Path) -> Result<(), SafetyError> {
    if !source.exists() {
        return Ok(());
    }
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent).map_err(|e| SafetyError::io(parent, e))?;
    }
    fs::copy(source, backup).map_err(|e| SafetyError::io(backup, e))?;
    Ok(())
}

fn temp_sibling(target: &Path) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    parent.join(format!(".{name}.tmp.{}", process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use tempfile::tempdir;

    #[test]
    fn write_atomic_round_trips() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("nested/state.json");
        write_atomic(&target, b"hello").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"hello");
    }

    #[test]
    fn write_atomic_overwrites_existing() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("state.json");
        write_atomic(&target, b"v1").unwrap();
        write_atomic(&target, b"v2").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"v2");
    }

    #[test]
    fn pre_migration_backup_is_one_shot() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("state.json");
        let backup = dir.path().join("state.json.v8.bak");
        fs::write(&source, b"first").unwrap();
        backup_pre_migration(&source, &backup).unwrap();
        // Mutate source; backup must NOT be overwritten.
        fs::write(&source, b"second").unwrap();
        backup_pre_migration(&source, &backup).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), b"first");
    }

    #[test]
    fn last_write_backup_rotates() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("state.json");
        let backup = dir.path().join("state.json.bak");
        fs::write(&source, b"a").unwrap();
        backup_last_write(&source, &backup).unwrap();
        fs::write(&source, b"b").unwrap();
        backup_last_write(&source, &backup).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), b"b");
    }

    #[test]
    fn lock_grants_then_blocks_concurrent_acquire() {
        let dir = tempdir().unwrap();
        let lock_path = dir.path().join("state.json.lock");
        let g1 = acquire_lock(&lock_path, Duration::from_millis(200)).unwrap();
        // Background thread tries to acquire while g1 is held; must time out.
        let lp = lock_path.clone();
        let handle = thread::spawn(move || acquire_lock(&lp, Duration::from_millis(300)));
        let result = handle.join().unwrap();
        assert!(matches!(result, Err(SafetyError::LockTimeout { .. })));
        drop(g1);
        // After release a fresh acquire succeeds quickly.
        let _g2 = acquire_lock(&lock_path, Duration::from_millis(500)).unwrap();
    }

    #[test]
    fn backup_noop_when_source_missing() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("missing.json");
        let backup = dir.path().join("missing.json.bak");
        backup_pre_migration(&source, &backup).unwrap();
        backup_last_write(&source, &backup).unwrap();
        assert!(!backup.exists());
    }
}
