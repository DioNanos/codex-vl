use pretty_assertions::assert_eq;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

use super::managed_codex_bin;
use super::resolve_managed_codex_bin_for_install_context;
use codex_install_context::InstallContext;
use codex_install_context::InstallMethod;
use codex_install_context::StandalonePlatform;

use super::ExecutableIdentity;
use super::executable_identity;
use super::parse_codex_version;

// codex-vl: helper di test ripristinati (persi dall'auto-merge rust-v0.158.0).
pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());

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

#[tokio::test]
async fn executable_identity_uses_path_and_binary_contents() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let executable = directory.path().join("codex");
    // Span multiple reads, including a partial final buffer, and preserve the
    // digest stored by older clients that hashed the complete file in memory.
    let mut bytes: Vec<u8> = (0..200_003).map(|index| (index % 251) as u8).collect();
    for contents in [&bytes[..], &[][..]] {
        std::fs::write(&executable, contents).expect("write executable");
        assert_eq!(
            executable_identity(&executable).await.expect("identity"),
            ExecutableIdentity {
                digest: *blake3::hash(contents).as_bytes(),
                path_digest: Some(super::path_digest(
                    &std::fs::canonicalize(&executable).expect("canonical executable"),
                )),
            }
        );
    }
    let copy = directory.path().join("codex-copy");
    std::fs::copy(&executable, &copy).expect("copy executable");
    let identity = executable_identity(&executable).await.expect("identity");
    let copy_identity = executable_identity(&copy).await.expect("copy identity");
    assert_ne!(identity, copy_identity);
    assert!(identity.same_contents(&copy_identity));
    std::fs::write(&executable, &bytes).expect("write executable");
    let old = executable_identity(&executable).await.expect("identity");
    bytes[100_000] ^= 1;
    std::fs::write(&executable, bytes).expect("replace executable");
    assert_ne!(
        executable_identity(&executable)
            .await
            .expect("new identity"),
        old
    );
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
fn managed_codex_bin_resolves_to_self_exe_when_vite_plus() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("codex-vl-vite-plus");
    std::fs::write(&bin, b"binary").expect("write binary");

    with_self_exe(&bin, || {
        let resolved = resolve_managed_codex_bin_for_install_context(
            &ctx(InstallMethod::VitePlus),
            temp.path(),
        )
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

pub(crate) fn with_self_exe<T>(path: &Path, f: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let old = std::env::var_os("CODEX_SELF_EXE");
    // SAFETY: the test holds a process-wide mutex for this environment
    // mutation and restores the original value before releasing it.
    unsafe {
        std::env::set_var("CODEX_SELF_EXE", path);
    }
    let result = f();
    // SAFETY: guarded by ENV_LOCK as above.
    unsafe {
        match old {
            Some(value) => std::env::set_var("CODEX_SELF_EXE", value),
            None => std::env::remove_var("CODEX_SELF_EXE"),
        }
    }
    result
}
