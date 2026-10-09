//! Covers complete-package staging, conservative selection, and legacy preservation.
#![cfg(unix)]
use super::InstallMode;
use super::prepare_from_package;
use super::validate_package;
use crate::settings::DaemonSettings;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::path::PathBuf;

use codex_install_context::InstallContext;
use codex_install_context::InstallMethod;

use crate::managed_install::tests::with_self_exe;

/// Bytes della fixture di selezione npm Android: scritti dalla fixture e
/// riattesi dopo prepare, con una sola definizione per fixture e assert.
const NPM_ANDROID_FIXTURE_BYTES: &[u8] = b"fixture selection bytes";

/// Real layout of an Android npm package: an executable `bin/codex` plus the
/// manifest with the fork variant, next to the binary.
fn npm_android_layout(root: &Path) -> PathBuf {
    let bin = root.join("npm/bin/codex");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("npm bin directory");
    std::fs::write(&bin, NPM_ANDROID_FIXTURE_BYTES).expect("npm binary");
    std::fs::write(
        bin.parent()
            .and_then(Path::parent)
            .unwrap()
            .join("codex-package.json"),
        concat!(
            r#"{"layoutVersion":1,"version":"0.160.0-vl.1","target":"aarch64-linux-android","#,
            r#""variant":"codex-vl","entrypoint":"bin/codex","#,
            r#""resourcesDir":"codex-resources","pathDir":"codex-path"}"#,
        ),
    )
    .expect("npm manifest");
    bin
}

fn daemon(home: &std::path::Path) -> crate::Daemon {
    let state = home.join("app-server-daemon-vl");
    crate::Daemon {
        log_diagnostics: false,
        socket_path: state.join("app-server.sock"),
        pid_file: state.join("app-server.pid"),
        update_pid_file: state.join("app-server-updater.pid"),
        operation_lock_file: state.join("daemon.lock"),
        settings_file: state.join("settings.json"),
        managed_codex_bin: crate::managed_install::managed_codex_bin(home),
        install_context: codex_install_context::InstallContext::current().clone(),
        launch_grant: None,
    }
}

#[cfg(target_os = "android")]
#[tokio::test]
async fn prepare_reuses_termux_binary_outside_managed_packages() {
    let temp = tempfile::TempDir::new().expect("temp");
    let home = temp.path().join("home");
    let npm_bin = npm_android_layout(temp.path());
    // Fixture di selezione: il file fittizio basta al resolver e alla guardia
    // L1; per lo smoke serve il vero pacchetto npm Android.
    let mut daemon = daemon(&home);
    daemon.managed_codex_bin = npm_bin.clone();
    daemon.install_context = InstallContext {
        method: InstallMethod::Npm,
        package_layout: None,
    };
    // Canonicalized expectation: the resolver canonicalizes the self exe, and
    // a raw comparison against TMPDIR through a symlink would be a false red.
    let expected = std::fs::canonicalize(&npm_bin).expect("canonicalize npm binary");

    // Before prepare: current_installation keeps the npm selection
    // (self exe with the variant manifest), not the managed path.
    let installed = with_self_exe(&npm_bin, || {
        daemon.current_installation().expect("current installation")
    });
    assert_eq!(installed.managed_codex_bin, expected);
    assert!(!home.join("packages").exists());

    // Prepare gira sul daemon RESTITUITO da current_installation: e` il
    // lifecycle vero, non il costruttore con la selezione manuale.
    super::prepare(&installed, &DaemonSettings::default())
        .await
        .expect("reuse npm binary without a standalone package manifest");
    assert!(!home.join("packages").exists());
    assert_eq!(std::fs::read(&npm_bin).unwrap(), NPM_ANDROID_FIXTURE_BYTES);

    // Dopo prepare: la selezione ripetuta sullo STESSO lifecycle.
    with_self_exe(&npm_bin, || {
        let after = installed
            .current_installation()
            .expect("installation after prepare");
        assert_eq!(after.managed_codex_bin, expected);
    });

    // Foreign variant through prepare: fail closed, and no packages directory is created.
    std::fs::write(
        npm_bin
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("codex-package.json"),
        concat!(
            r#"{"layoutVersion":1,"version":"0.160.0-vl.1","target":"aarch64-linux-android","#,
            r#""variant":"codex-termux","entrypoint":"bin/codex"}"#,
        ),
    )
    .expect("foreign manifest");
    let error = super::prepare(&installed, &DaemonSettings::default())
        .await
        .expect_err("foreign variant must fail closed");
    assert!(error.to_string().contains("codex-vl"));
    assert!(!home.join("packages").exists());

    // Manifest assente ATTRAVERSO prepare: stesso fail closed.
    std::fs::remove_file(
        npm_bin
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("codex-package.json"),
    )
    .expect("remove manifest");
    let error = super::prepare(&installed, &DaemonSettings::default())
        .await
        .expect_err("missing manifest must fail closed");
    assert!(error.to_string().contains("manifest"));
    assert!(!home.join("packages").exists());
}

// The Android policy can also be exercised on Linux: with the Npm method the
// lifecycle selection is the npm binary (self exe), never
// packages/app-server-daemon-vl/current. Repeated unit check of the
// selection and of the variant check: it does not go through prepare (that
// is covered by the Android test below, on the real lifecycle).
#[test]
fn android_policy_keeps_npm_selection_and_never_builds_packages() {
    let temp = tempfile::TempDir::new().expect("temp");
    let home = temp.path().join("home");
    let npm_bin = npm_android_layout(temp.path());
    let context = InstallContext {
        method: InstallMethod::Npm,
        package_layout: None,
    };

    with_self_exe(&npm_bin, || {
        let selected = crate::Daemon::managed_selection_for_platform(true, &context, &home)
            .expect("android selection");
        assert_eq!(
            selected,
            std::fs::canonicalize(&npm_bin).expect("canonicalize")
        );
        crate::managed_install::ensure_selected_variant_is_fork(&selected)
            .expect("variant codex-vl accepted");
        assert!(!home.join("packages").exists());

        // Selezione ripetuta: deterministica, stesso risultato.
        let repeated = crate::Daemon::managed_selection_for_platform(true, &context, &home)
            .expect("android selection repeated");
        assert_eq!(repeated, selected);
        assert!(!home.join("packages").exists());
    });
}

// Fail closed when the manifest is missing or the variant is foreign: the selection
// resolves, but the L1 check rejects execution and packages/ is not created.
#[test]
fn android_selection_fail_closed_on_missing_or_foreign_manifest() {
    let temp = tempfile::TempDir::new().expect("temp");
    let home = temp.path().join("home");
    let bin = temp.path().join("npm/bin/codex");
    std::fs::create_dir_all(bin.parent().unwrap()).expect("npm bin directory");
    std::fs::write(&bin, b"bundled ELF").expect("npm binary");
    let context = InstallContext {
        method: InstallMethod::Npm,
        package_layout: None,
    };

    with_self_exe(&bin, || {
        let selected = crate::Daemon::managed_selection_for_platform(true, &context, &home)
            .expect("selection resolves");
        let error = crate::managed_install::ensure_selected_variant_is_fork(&selected)
            .expect_err("missing manifest must fail closed");
        assert!(error.to_string().contains("manifest"));
    });

    std::fs::write(
        bin.parent()
            .and_then(Path::parent)
            .unwrap()
            .join("codex-package.json"),
        concat!(
            r#"{"layoutVersion":1,"version":"0.160.0-vl.1","target":"aarch64-linux-android","#,
            r#""variant":"codex-termux","entrypoint":"bin/codex"}"#,
        ),
    )
    .expect("foreign manifest");
    with_self_exe(&bin, || {
        let selected = crate::Daemon::managed_selection_for_platform(true, &context, &home)
            .expect("selection resolves");
        let error = crate::managed_install::ensure_selected_variant_is_fork(&selected)
            .expect_err("foreign variant must fail closed");
        assert!(error.to_string().contains("codex-vl"));
    });
    assert!(!home.join("packages").exists());
}

fn package(root: &Path, version: &str) -> PathBuf {
    let target = super::platform_target().expect("target");
    for dir in ["bin", "codex-path", "codex-resources/nested"] {
        std::fs::create_dir_all(root.join(dir)).expect("package directory");
    }
    let bin = root.join("bin/codex");
    codex_utils_cargo_bin::write_executable(&bin, &format!("#!/bin/sh\necho 'codex {version}'\n"))
        .expect("codex executable");
    for file in ["bin/codex-code-mode-host", "codex-path/rg"] {
        codex_utils_cargo_bin::write_executable(&root.join(file), "runtime")
            .expect("executable helper");
    }
    std::fs::write(root.join("codex-resources/nested/runtime"), b"runtime").expect("package file");
    if cfg!(target_os = "linux") {
        codex_utils_cargo_bin::write_executable(&root.join("codex-resources/bwrap"), "runtime")
            .expect("executable bwrap");
    }
    std::fs::write(
        root.join("codex-package.json"),
        serde_json::json!({
            "version": version, "target": target, "variant": "codex-vl", "entrypoint": "bin/codex"
        })
        .to_string(),
    )
    .expect("manifest");
    bin
}

#[tokio::test]
async fn seeds_full_package() {
    let temp = tempfile::TempDir::new().expect("temp");
    let home = temp.path().join("home");
    let daemon = daemon(&home);
    let settings = DaemonSettings::default();
    let old = temp.path().join("old");
    let old_bin = package(&old, "0.152.0");
    prepare_from_package(
        &daemon,
        &settings,
        InstallMode::Missing,
        Some(&old),
        &old_bin,
        |_| Ok(true),
    )
    .await
    .expect("seed");

    let standalone = home.join("packages/app-server-daemon-vl");
    let selected = std::fs::canonicalize(standalone.join("current")).expect("selected");
    assert_eq!(
        std::fs::read(selected.join("codex-resources/nested/runtime")).expect("runtime"),
        b"runtime"
    );
    assert_eq!(
        std::fs::read_to_string(standalone.join("auto-update-version")).expect("marker"),
        selected.file_name().expect("name").to_string_lossy()
    );
    assert!(validate_package(&selected).is_ok());
}

/// The Android package does not bundle ripgrep (Termux resolves `rg` from
/// PATH via `pkg install ripgrep`), so it must validate without
/// `codex-path/rg` while every other platform still requires it.
#[test]
fn android_package_without_bundled_ripgrep_is_valid() {
    let temp = tempfile::TempDir::new().expect("temp");
    let source = temp.path().join("package");
    package(&source, "0.152.0");
    std::fs::remove_file(source.join("codex-path/rg")).expect("remove bundled rg");

    let error = super::validate_package_for_platform(&source, /*android*/ false)
        .expect_err("non-Android platforms still require the bundled rg");
    assert!(
        error.to_string().contains("codex-path/rg"),
        "the error must name the missing file: {error}"
    );

    super::validate_package_for_platform(&source, /*android*/ true)
        .expect("the Android package ships without bundled ripgrep");
}

#[tokio::test]
async fn incomplete_source_fails_without_selecting_it() {
    let temp = tempfile::TempDir::new().expect("temp");
    let source = temp.path().join("package");
    let bin = package(&source, "0.152.0");
    std::fs::remove_file(source.join("bin/codex-code-mode-host")).expect("remove helper");
    let home = temp.path().join("home");
    let error = prepare_from_package(
        &daemon(&home),
        &DaemonSettings::default(),
        InstallMode::Missing,
        Some(&source),
        &bin,
        |_| Ok(true),
    )
    .await
    .expect_err("incomplete package");
    assert!(error.to_string().contains("bin/codex-code-mode-host"));
    assert!(!home.join("packages/app-server-daemon/current").exists());
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn provisioned_macos_bundle_seeds_from_its_running_executable() {
    let temp = tempfile::TempDir::new().expect("temp");
    let source = temp.path().join("package");
    let launcher = package(&source, "0.1.0-internal-test.202609091200.1");
    codex_utils_cargo_bin::write_executable(&launcher, "#!/bin/sh\necho codex 0.0.0\n")
        .expect("launcher");
    let bundle = source.join("CodexCLI.app/Contents/MacOS/codex");
    std::fs::create_dir_all(bundle.parent().expect("bundle parent")).expect("bundle dir");
    std::fs::write(&bundle, b"provisioned executable").expect("bundle executable");
    let home = temp.path().join("home");
    prepare_from_package(
        &daemon(&home),
        &DaemonSettings::default(),
        InstallMode::Missing,
        Some(&source),
        &bundle,
        |_| Ok(true),
    )
    .await
    .expect("seed provisioned bundle");
    let selected = std::fs::canonicalize(home.join("packages/app-server-daemon-vl/current"))
        .expect("selected release");
    assert_eq!(
        std::fs::read(selected.join("CodexCLI.app/Contents/MacOS/codex"))
            .expect("bundled executable"),
        b"provisioned executable"
    );
    assert!(
        !home
            .join("packages/app-server-daemon-vl/auto-update-version")
            .exists()
    );
}

#[tokio::test]
async fn upstream_legacy_installation_is_preserved_while_fork_seeds_its_own() {
    let temp = tempfile::TempDir::new().unwrap();
    let home = temp.path().join("home");
    let legacy = home.join("packages/standalone");
    let upstream_bin = package(&legacy.join("releases/old"), "0.150.0");
    std::os::unix::fs::symlink("releases/old", legacy.join("current")).unwrap();
    let upstream_bin_before = std::fs::read(&upstream_bin).unwrap();
    // Sentinel upstream: anche il LOG upstream (nello state upstream, non
    // nello state -vl) va predisposto e ricontrollato dopo prepare.
    let upstream_state = home.join("app-server-daemon");
    std::fs::create_dir_all(&upstream_state).unwrap();
    std::fs::write(
        upstream_state.join("app-server.stderr.log"),
        b"upstream log",
    )
    .unwrap();
    let source = temp.path().join("new");
    let new_bin = package(&source, "0.160.0");
    prepare_from_package(
        &daemon(&home),
        &DaemonSettings::default(),
        InstallMode::Missing,
        Some(&source),
        &new_bin,
        |_| Ok(true),
    )
    .await
    .unwrap();
    // Il fork seleziona la COPIA del suo pacchetto appena seminato nel root
    // -vl (stessi bytes del source, mai il binario upstream).
    let selected = crate::managed_install::managed_codex_bin(&home)
        .canonicalize()
        .unwrap();
    assert!(
        selected.starts_with(home.join("packages/app-server-daemon-vl/releases")),
        "selezione fuori dal root -vl: {selected:?}"
    );
    assert_eq!(
        std::fs::read(&selected).unwrap(),
        std::fs::read(&new_bin).unwrap()
    );
    // L'installazione legacy upstream resta esattamente com'era: binario,
    // current, log, nessun marker o migrazione, nessun root app-server-daemon.
    assert_eq!(std::fs::read(&upstream_bin).unwrap(), upstream_bin_before);
    assert_eq!(
        std::fs::read(upstream_state.join("app-server.stderr.log")).unwrap(),
        b"upstream log"
    );
    assert_eq!(
        std::fs::read_link(legacy.join("current")).unwrap(),
        PathBuf::from("releases/old")
    );
    assert!(!legacy.join("auto-update-version").exists());
    assert!(!home.join("packages/app-server-daemon").exists());
}

#[tokio::test]
async fn standalone_seed_preserves_explicit_pin_or_latest_channel() {
    for follows_latest in [false, true] {
        let temp = tempfile::TempDir::new().unwrap();
        let home = temp.path().join("home");
        let standalone = home.join("packages/standalone");
        let source = standalone.join("releases/0.152.0-local-target");
        let bin = package(&source, "0.152.0");
        std::os::unix::fs::symlink(&source, standalone.join("current")).unwrap();
        if follows_latest {
            std::fs::write(
                standalone.join("auto-update-version"),
                "0.152.0-local-target",
            )
            .unwrap();
        }
        prepare_from_package(
            &daemon(&home),
            &DaemonSettings::default(),
            InstallMode::Missing,
            Some(&source),
            &bin,
            |_| Ok(true),
        )
        .await
        .unwrap();
        let root = home.join("packages/app-server-daemon-vl");
        let selected = root.join("current").canonicalize().unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("auto-update-version")).ok(),
            follows_latest.then(|| selected.file_name().unwrap().to_string_lossy().into_owned())
        );
    }
}

#[tokio::test]
async fn explicit_selection_requires_unchanged_cli_and_pins_all_versions() {
    for upstream_present in [false, true] {
        let temp = tempfile::TempDir::new().unwrap();
        let home = temp.path().join("home");
        let source = temp.path().join("source");
        let bin = package(&source, "0.152.0");
        let settings = DaemonSettings::default();
        let initial = daemon(&home);
        // Il seed fork resta INTATTO in entrambe le iterazioni: nessuna
        // migrazione o rename del root -vl.
        prepare_from_package(
            &initial,
            &settings,
            InstallMode::Missing,
            Some(&source),
            &bin,
            |_| Ok(true),
        )
        .await
        .unwrap();
        // The upstream install, when present, is a separate tree
        // with its own sentinels: its own release, current, marker, and log,
        // never the fork root. The initial bytes are saved and checked again
        // after rejection, cancellation, and replacement.
        let upstream_release = home.join("packages/standalone/releases/0.152.0-local-target");
        let upstream_current = home.join("packages/standalone/current");
        let upstream_marker = home.join("packages/standalone/auto-update-version");
        let upstream_state = home.join("app-server-daemon");
        let mut upstream_bin_before: Option<Vec<u8>> = None;
        let mut upstream_log_before: Option<Vec<u8>> = None;
        if upstream_present {
            let upstream_bin = package(&upstream_release, "0.152.0");
            std::os::unix::fs::symlink(&upstream_release, &upstream_current).unwrap();
            std::fs::write(&upstream_marker, b"0.152.0-local-target").unwrap();
            std::fs::create_dir_all(&upstream_state).unwrap();
            std::fs::write(
                upstream_state.join("app-server.stderr.log"),
                b"upstream log",
            )
            .unwrap();
            upstream_bin_before = Some(std::fs::read(&upstream_bin).unwrap());
            upstream_log_before =
                Some(std::fs::read(upstream_state.join("app-server.stderr.log")).unwrap());
        }
        let initial_root = crate::managed_install::package_root(&home);
        // Baseline del fork prima del rifiuto: selezione, marker e bytes del
        // pacchetto selezionato, da ricontrollare dopo l'errore.
        let fork_current_before = initial_root.join("current").canonicalize().unwrap();
        let fork_marker_before = std::fs::read(initial_root.join("auto-update-version")).ok();
        let selected_bytes_before = std::fs::read(fork_current_before.join("bin/codex")).unwrap();
        let error = prepare_from_package(
            &daemon(&home),
            &settings,
            InstallMode::Replace,
            Some(&source),
            &bin,
            |_| {
                let mut rebuilt = std::fs::read(&bin)?;
                rebuilt.extend_from_slice(b"# rebuilt during confirmation\n");
                std::fs::write(&bin, rebuilt)?;
                Ok(true)
            },
        )
        .await
        .expect_err("reject a CLI rebuilt during confirmation");
        assert!(
            error
                .to_string()
                .contains("differs from the running executable")
        );
        // L'errore non muove la selezione del fork: current, marker e bytes
        // restano esattamente la baseline, prima di leggere qualunque nuova
        // selezione come riferimento.
        assert_eq!(
            initial_root.join("current").canonicalize().unwrap(),
            fork_current_before
        );
        assert_eq!(
            std::fs::read(initial_root.join("auto-update-version")).ok(),
            fork_marker_before
        );
        assert_eq!(
            std::fs::read(fork_current_before.join("bin/codex")).unwrap(),
            selected_bytes_before
        );
        if upstream_present {
            assert_eq!(
                upstream_current.canonicalize().unwrap(),
                upstream_release.canonicalize().unwrap()
            );
            assert_eq!(
                std::fs::read(&upstream_marker).unwrap(),
                b"0.152.0-local-target"
            );
            assert_eq!(
                std::fs::read(upstream_release.join("bin/codex")).unwrap(),
                upstream_bin_before.as_deref().unwrap()
            );
            // Anche il log upstream resta immutato dopo il rifiuto.
            assert_eq!(
                std::fs::read(upstream_state.join("app-server.stderr.log")).unwrap(),
                upstream_log_before.as_deref().unwrap()
            );
        }
        let root = home.join("packages/app-server-daemon-vl");
        let mut previous = initial_root.join("current").canonicalize().unwrap();
        for version in [
            "0.153.0",
            "0.151.0",
            "0.151.0",
            "0.154.0-alpha.1",
            "0.0.0",
            "0.0.0",
        ] {
            let daemon = daemon(&home);
            let previous_root = crate::managed_install::package_root(&home);
            package(&source, version);
            std::fs::write(
                source.join("codex-resources/nested/runtime"),
                previous.to_string_lossy().as_bytes(),
            )
            .unwrap();
            let before = std::fs::read(previous.join("bin/codex")).unwrap();
            let fork_marker_before_loop =
                std::fs::read(previous_root.join("auto-update-version")).ok();
            assert!(
                !prepare_from_package(
                    &daemon,
                    &settings,
                    InstallMode::Replace,
                    Some(&source),
                    &bin,
                    |request| {
                        assert_eq!(request.destination, root);
                        Ok(false)
                    }
                )
                .await
                .unwrap()
            );
            assert_eq!(crate::managed_install::package_root(&home), previous_root);
            assert_eq!(
                previous_root.join("current").canonicalize().unwrap(),
                previous
            );
            // Annullamento: marker del fork e sentinel upstream immutati,
            // non solo dopo la sostituzione andata a buon fine.
            assert_eq!(
                std::fs::read(previous_root.join("auto-update-version")).ok(),
                fork_marker_before_loop
            );
            assert_eq!(std::fs::read(previous.join("bin/codex")).unwrap(), before);
            if upstream_present {
                assert_eq!(
                    upstream_current.canonicalize().unwrap(),
                    upstream_release.canonicalize().unwrap()
                );
                assert_eq!(
                    std::fs::read(&upstream_marker).unwrap(),
                    b"0.152.0-local-target"
                );
                assert_eq!(
                    std::fs::read(upstream_release.join("bin/codex")).unwrap(),
                    upstream_bin_before.as_deref().unwrap()
                );
                assert_eq!(
                    std::fs::read(upstream_state.join("app-server.stderr.log")).unwrap(),
                    upstream_log_before.as_deref().unwrap()
                );
            }
            prepare_from_package(
                &daemon,
                &settings,
                InstallMode::Replace,
                Some(&source),
                &bin,
                |_| Ok(true),
            )
            .await
            .unwrap();
            let selected = root.join("current").canonicalize().unwrap();
            assert_eq!(crate::managed_install::package_root(&home), root);
            if upstream_present {
                // I sentinel upstream restano esattamente come predisposti.
                assert_eq!(
                    upstream_current.canonicalize().unwrap(),
                    upstream_release.canonicalize().unwrap()
                );
                assert_eq!(
                    std::fs::read(&upstream_marker).unwrap(),
                    b"0.152.0-local-target"
                );
                assert_eq!(
                    std::fs::read(upstream_release.join("bin/codex")).unwrap(),
                    upstream_bin_before.as_deref().unwrap()
                );
                assert_eq!(
                    std::fs::read(upstream_state.join("app-server.stderr.log")).unwrap(),
                    upstream_log_before.as_deref().unwrap()
                );
            }
            assert_ne!(selected, previous);
            assert_eq!(
                std::fs::read(selected.join("bin/codex")).unwrap(),
                std::fs::read(&bin).unwrap()
            );
            assert_eq!(std::fs::read(previous.join("bin/codex")).unwrap(), before);
            assert!(!root.join("auto-update-version").exists());
            assert!(daemon.running_backend(&settings).await.unwrap().is_none());
            previous = selected;
        }
    }
}

#[tokio::test]
async fn broken_selection_is_not_a_missing_installation() {
    let temp = tempfile::TempDir::new().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("source");
    let bin = package(&source, "0.152.0");
    let daemon = daemon(&home);
    // Selezione rotta DENTRO il root del fork: current punta a una release
    // mancante. La diagnostica deve nominare il percorso selezionato del fork,
    // non proporre riparazioni dell'installazione upstream.
    let current = crate::managed_install::package_root(&home).join("current");
    std::fs::create_dir_all(current.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("missing-release", &current).unwrap();
    // Sentinel upstream: state e current completi, da lasciare intatti.
    let upstream_state = home.join("app-server-daemon");
    std::fs::create_dir_all(&upstream_state).unwrap();
    std::fs::write(upstream_state.join("app-server.pid"), b"upstream daemon").unwrap();
    let upstream_current = home.join("packages/app-server-daemon/current");
    std::fs::create_dir_all(upstream_current.join("bin")).unwrap();
    std::fs::write(upstream_current.join("bin/codex"), b"upstream codex").unwrap();

    let error = prepare_from_package(
        &daemon,
        &DaemonSettings::default(),
        InstallMode::Missing,
        Some(&source),
        &bin,
        |_| panic!("broken installation must not request replacement"),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("managed standalone install not found")
    );
    assert!(
        error.to_string().contains(
            crate::managed_install::package_root(&home)
                .join("current/bin")
                .join("codex")
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(
        std::fs::read_link(current).unwrap(),
        PathBuf::from("missing-release")
    );
    // Nessuna nuova release o marker nasce nel root del fork per colpa della
    // selezione rotta.
    assert!(
        !crate::managed_install::package_root(&home)
            .join("releases")
            .exists()
    );
    assert!(
        !crate::managed_install::package_root(&home)
            .join("auto-update-version")
            .exists()
    );
    // I sentinel upstream non sono solo presenti: i loro bytes sono immutati.
    assert_eq!(
        std::fs::read(upstream_current.join("bin/codex")).unwrap(),
        b"upstream codex"
    );
    assert_eq!(
        std::fs::read(upstream_state.join("app-server.pid")).unwrap(),
        b"upstream daemon"
    );
}
