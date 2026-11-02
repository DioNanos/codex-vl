//! Tests for the NFS migration directory lock.

use std::path::Path;

use super::NfsDbMigration;

/// Whether this filesystem enforces advisory locks between two open handles
/// of the same file. Filesystems that report `ErrorKind::Unsupported` cannot
/// observe contention; on them the migration lock degrades by design and the
/// exclusion test below would be meaningless.
#[cfg(unix)]
pub(crate) fn advisory_locks_are_enforced(directory: &Path) -> bool {
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
        // assert about exclusion. CI runners use filesystems that enforce
        // advisory locks.
        return;
    }

    let first = NfsDbMigration::lock(home.path()).expect("first opener acquires the lock");

    let Err(second) = NfsDbMigration::lock(home.path()) else {
        panic!("second opener must fail while the first holds the lock");
    };
    assert!(
        second
            .to_string()
            .contains("another database migration is running"),
        "unexpected error for a second opener: {second:#}"
    );
    // The typed cause must survive the context layer: managed openers decide
    // whether to wait solely on `TryLockError::WouldBlock`, never on messages.
    assert!(
        matches!(
            second.downcast_ref::<std::fs::TryLockError>(),
            Some(std::fs::TryLockError::WouldBlock)
        ),
        "second opener must carry TryLockError::WouldBlock as its cause: {second:#}"
    );

    drop(first);
    NfsDbMigration::lock(home.path())
        .expect("lock is acquirable again after the first opener releases it");
}

/// Filesystem qualification must separate "advisory locks are unsupported"
/// (the migration proceeds unlocked by design) from any other lock failure
/// (fatal). Contention is never routed through this classification: it is a
/// distinct typed outcome of `try_lock`.
#[test]
fn migration_lock_distinguishes_unsupported_from_unexpected_errors() {
    let unsupported = std::io::Error::from(std::io::ErrorKind::Unsupported);
    assert!(super::is_unsupported_file_lock_error(&unsupported));
    for kind in [
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::NotFound,
        std::io::ErrorKind::TimedOut,
    ] {
        let unexpected = std::io::Error::from(kind);
        assert!(!super::is_unsupported_file_lock_error(&unexpected));
    }
}
