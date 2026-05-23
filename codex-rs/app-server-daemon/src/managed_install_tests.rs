use pretty_assertions::assert_eq;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

use super::executable_identity_from_bytes;
use super::managed_codex_bin;
use super::parse_codex_version;
use super::resolve_managed_codex_bin_for_install_context;
use codex_install_context::InstallContext;
use codex_install_context::InstallMethod;
use codex_install_context::StandalonePlatform;

static ENV_LOCK: Mutex<()> = Mutex::new(());

// codex-vl: post-merge InstallContext is a struct; helpers below build the
// per-method fixture variants the old enum API gave for free.
fn ctx(method: InstallMethod) -> InstallContext {
    InstallContext {
        method,
        package_layout: None,
    }
}

#[test]
fn parses_codex_cli_version_output() {
    assert_eq!(
        parse_codex_version("codex 1.2.3\n").expect("version"),
        "1.2.3"
    );
}

#[test]
fn rejects_malformed_codex_cli_version_output() {
    assert!(parse_codex_version("codex\n").is_err());
}

#[test]
fn executable_identity_uses_binary_contents() {
    let old = executable_identity_from_bytes(b"old");
    let same = executable_identity_from_bytes(b"old");
    let new = executable_identity_from_bytes(b"new");

    assert_eq!(old, same);
    assert_ne!(old, new);
}

#[test]
fn managed_codex_bin_resolves_to_self_exe_when_npm() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("codex-vl");
    std::fs::write(&bin, b"binary").expect("write binary");

    with_self_exe(&bin, || {
        let resolved =
            resolve_managed_codex_bin_for_install_context(&ctx(InstallMethod::Npm), temp.path())
                .expect("resolve");

        assert_eq!(resolved, std::fs::canonicalize(&bin).expect("canonicalize"));
    });
}

#[test]
fn managed_codex_bin_resolves_to_self_exe_when_bun() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("codex-vl-bun");
    std::fs::write(&bin, b"binary").expect("write binary");

    with_self_exe(&bin, || {
        let resolved =
            resolve_managed_codex_bin_for_install_context(&ctx(InstallMethod::Bun), temp.path())
                .expect("resolve");

        assert_eq!(resolved, std::fs::canonicalize(&bin).expect("canonicalize"));
    });
}

#[test]
fn managed_codex_bin_ignores_missing_self_exe_for_npm() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("missing-codex-vl");

    with_self_exe(&missing, || {
        let resolved =
            resolve_managed_codex_bin_for_install_context(&ctx(InstallMethod::Npm), temp.path())
                .expect("resolve");

        assert_eq!(
            resolved,
            std::fs::canonicalize(std::env::current_exe().expect("current exe"))
                .expect("canonicalize current exe")
        );
    });
}

#[test]
fn managed_codex_bin_falls_back_to_standalone_path_for_brew_and_standalone_contexts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let legacy = managed_codex_bin(temp.path());
    let release_dir = codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
        PathBuf::from("/tmp/codex-release"),
    )
    .expect("absolute path");
    let contexts = [
        ctx(InstallMethod::Standalone {
            release_dir,
            resources_dir: None,
            platform: StandalonePlatform::Unix,
        }),
        ctx(InstallMethod::Brew),
    ];

    for context in contexts {
        assert_eq!(
            resolve_managed_codex_bin_for_install_context(&context, temp.path()).expect("resolve"),
            legacy
        );
    }
}

#[test]
fn managed_codex_bin_routes_other_via_current_exe() {
    // codex-vl Step 14 Bug 2 fix — `InstallMethod::Other` happens when
    // the user runs the fork binary through a symlink that bypasses
    // the Node.js wrapper (so `CODEX_MANAGED_BY_NPM` is unset and the
    // exe is not under any known standalone release prefix). In that
    // case the daemon must re-launch via `current_exe` /
    // `CODEX_SELF_EXE`, not via the standalone path the fork never
    // ships — otherwise the fork-protection error in
    // `ensure_managed_codex_bin` fires spuriously and `/remote-control
    // start` fails for direct-binary users.
    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("codex-vl-other");
    std::fs::write(&bin, b"binary").expect("write binary");

    with_self_exe(&bin, || {
        let resolved =
            resolve_managed_codex_bin_for_install_context(&ctx(InstallMethod::Other), temp.path())
                .expect("resolve");
        assert_eq!(resolved, std::fs::canonicalize(&bin).expect("canonicalize"));
    });
}

fn with_self_exe(path: &Path, f: impl FnOnce()) {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let old = std::env::var_os("CODEX_SELF_EXE");
    // SAFETY: the test holds a process-wide mutex for this environment
    // mutation and restores the original value before releasing it.
    unsafe {
        std::env::set_var("CODEX_SELF_EXE", path);
    }
    f();
    // SAFETY: guarded by ENV_LOCK as above.
    unsafe {
        match old {
            Some(value) => std::env::set_var("CODEX_SELF_EXE", value),
            None => std::env::remove_var("CODEX_SELF_EXE"),
        }
    }
}
