//! Tests for the NFS migration directory lock.

use std::path::Path;

use super::NfsDbMigration;

/// Whether this filesystem enforces advisory locks between two open handles
/// of the same file. Filesystems that report `ErrorKind::Unsupported` cannot
/// observe contention; on them the migration lock degrades by design and the
/// exclusion test below would be meaningless.
#[cfg(unix)]
fn advisory_locks_are_enforced(directory: &Path) -> bool {
    // A name distinct from the migration lock file so the probe cannot hold it.
    let probe_path = directory.join("advisory-lock-probe");
    let first = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&probe_path)
        .expect("open first probe handle");
    let second = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&probe_path)
        .expect("open second probe handle");
    let enforced = matches!(
        (first.try_lock(), second.try_lock()),
        (Ok(()), Err(std::fs::TryLockError::WouldBlock))
    );
    let _ = std::fs::remove_file(probe_path);
    enforced
}

#[cfg(unix)]
#[test]
fn migration_lock_rejects_a_second_opener_until_release() {
    let home = tempfile::tempdir().expect("create temporary sqlite home");
    if !advisory_locks_are_enforced(home.path()) {
        // Advisory locks are unavailable here (the degrade path): nothing to
        // assert about exclusion. CI and MGM run on filesystems that enforce.
        return;
    }

    let first = NfsDbMigration::lock(home.path()).expect("first opener acquires the lock");

    let second = NfsDbMigration::lock(home.path())
        .expect_err("second opener must fail while the first holds the lock");
    assert!(
        second.to_string().contains("another database migration is running"),
        "unexpected error for a second opener: {second:#}"
    );

    drop(first);
    NfsDbMigration::lock(home.path())
        .expect("lock is acquirable again after the first opener releases it");
}
