use std::path::PathBuf;
use std::sync::Mutex;
#[cfg(unix)]
use std::time::Duration;

use pretty_assertions::assert_eq;
#[cfg(unix)]
use tempfile::TempDir;

use super::INSTALL_URL;
use super::InstallerHttp;
use super::InstallerResponse;
use super::fetch_installer_script;
use super::install_latest_standalone;
#[cfg(unix)]
use super::manual_update::run as manual_update_once;
use super::reexec_managed_updater;
#[cfg(unix)]
use crate::Daemon;
#[cfg(unix)]
use crate::RestartIfRunningOutcome;
#[cfg(unix)]
use crate::UpdateOutput;
#[cfg(unix)]
use crate::UpdateStatus;
#[cfg(unix)]
use crate::managed_install::executable_identity;
#[cfg(unix)]
use crate::managed_install::executable_identity_from_reader;
#[cfg(unix)]
use codex_install_context::InstallContext;
use codex_install_context::InstallMethod;

#[tokio::test]
async fn installer_fetch_uses_exact_url_and_preserves_bytes() {
    let script = b"#!/bin/sh\nprintf 'update bytes'\n".to_vec();
    let http = FakeInstallerHttp::new(InstallerResponse::Success(script.clone()));

    assert_eq!(
        fetch_installer_script(&http)
            .await
            .expect("installer fetch should succeed"),
        script
    );
    assert_eq!(http.requested_urls(), vec![INSTALL_URL.to_string()]);
}

#[tokio::test]
async fn installer_fetch_rejects_non_success_status() {
    let http = FakeInstallerHttp::new(InstallerResponse::Unsuccessful { status: 503 });

    let error = fetch_installer_script(&http)
        .await
        .expect_err("non-success response should fail");

    assert!(error.to_string().contains("503"));
    assert_eq!(http.requested_urls(), vec![INSTALL_URL.to_string()]);
}

fn install_context(method: InstallMethod) -> InstallContext {
    InstallContext {
        method,
        package_layout: None,
    }
}

/// The standalone auto-updater MUST stay disabled in the fork: the upstream
/// installer would replace this binary and silently remove fork behavior.
#[tokio::test]
async fn install_latest_standalone_is_disabled_in_fork() {
    let result = install_latest_standalone().await;
    let error =
        result.expect_err("standalone updates must fail closed without fetching an installer");
    let message = format!("{error}");
    assert!(
        message.contains("codex-vl fork"),
        "fork-disabled error must identify the fork: {message}"
    );
    assert!(
        message.contains("disabled"),
        "fork-disabled error must state that updates are disabled: {message}"
    );
    assert!(
        message.contains("@mmmbuto/codex-vl"),
        "fork-disabled error must name the supported package: {message}"
    );
}

#[test]
fn reexec_managed_updater_short_circuits_for_package_shims() {
    let missing = std::path::Path::new("/definitely/not/a/codex-vl-binary");

    for method in [
        InstallMethod::Npm,
        InstallMethod::Bun,
        InstallMethod::VitePlus,
        InstallMethod::Pnpm,
    ] {
        reexec_managed_updater(
            missing,
            &install_context(method),
            missing.parent().expect("state dir"),
        )
        .expect("package-shim updater replacement must short-circuit");
    }
}

/// The refusal is deterministic for this fork: a scheduled tick must stop the
/// update loop instead of waking up to fail identically on every interval.
#[cfg(unix)]
#[tokio::test]
async fn scheduled_refusal_stops_the_update_loop() {
    let home = TempDir::new().expect("home");
    let (daemon, _) = manual_update_daemon(&home);
    let identity = executable_identity(&daemon.managed_codex_bin)
        .await
        .expect("updater identity");
    let http = FakeInstallerHttp::new(InstallerResponse::Success(Vec::new()));

    let (control, restart) = super::update_once(
        &http,
        &daemon,
        &identity,
        &mut test_terminate(),
        super::UpdateTrigger::Scheduled,
    )
    .await
    .expect("a scheduled refusal must be a clean stop, not a retry-able error");

    assert!(
        matches!(control, super::UpdateLoopControl::Stop),
        "the loop must exit instead of scheduling another attempt"
    );
    assert!(restart.is_none(), "a refusal must not restart anything");
    assert!(
        http.requested_urls().is_empty(),
        "the refusal must happen before any installer HTTP request"
    );
}

/// The standalone updater can never install anything for package-manager
/// installs, so it must not be spawned for them in the first place.
#[cfg(unix)]
#[tokio::test]
async fn package_manager_installs_never_start_the_standalone_updater() {
    let home = TempDir::new().expect("home");
    let (daemon, _) = manual_update_daemon(&home);
    // A shim whose `pid-update-loop --help` probe succeeds and whose spawned
    // form keeps running: the only thing that can keep the updater down for
    // npm-style installs is the install-context gate itself.
    std::fs::write(
        &daemon.managed_codex_bin,
        b"#!/bin/sh\nif [ \"$1\" = '--version' ]; then echo codex 1.0.0; exit 0; fi\nif [ \"$4\" = '--help' ]; then exit 0; fi\nexec sleep 30\n",
    )
    .expect("managed shim");

    for method in [InstallMethod::Npm, InstallMethod::Bun] {
        let daemon = crate::Daemon {
            install_context: install_context(method.clone()),
            ..daemon.clone()
        };
        let settings = daemon.load_settings().await.expect("settings");
        assert!(
            !daemon
                .ensure_managed_updater(&settings)
                .await
                .expect("gate result"),
            "{method:?} installs must not auto-update through the standalone updater"
        );
        let updater = crate::backend::pid_update_loop_backend(daemon.backend_paths(&settings));
        assert!(
            !updater
                .is_starting_or_running()
                .await
                .expect("updater state"),
            "{method:?} installs must not leave an updater running"
        );
    }

    // The gate does not own other install contexts: the updater still starts
    // for them (its loop then exits at the fork's install refusal).
    let settings = daemon.load_settings().await.expect("settings");
    assert!(
        daemon
            .ensure_managed_updater(&settings)
            .await
            .expect("gate result"),
        "non-package install contexts keep the standalone updater"
    );
    let updater = crate::backend::pid_update_loop_backend(daemon.backend_paths(&settings));
    assert!(
        updater
            .is_starting_or_running()
            .await
            .expect("updater state"),
        "the started updater must be alive before the test stops it"
    );
    updater.stop().await.expect("stop updater");
}

#[cfg(unix)]
#[test]
fn updater_reexec_preserves_paths_and_recovers_deleted_working_directory() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;

    for (delete_cwd, home_name) in [
        (false, "home"),
        (true, "home"),
        (false, "alias"),
        (true, "alias"),
        // macOS filesystems reject non-Unicode names.
        #[cfg(target_os = "linux")]
        (false, "link/../home"),
    ] {
        let home = TempDir::new().expect("home");
        let project = home.path().join("project");
        let state = home.path().join("state");
        let codex_home = home.path().join("home");
        std::fs::create_dir(&project).expect("project");
        std::fs::create_dir(&state).expect("state");
        std::fs::create_dir(&codex_home).expect("codex home");
        std::os::unix::fs::symlink(&codex_home, home.path().join("alias")).expect("home alias");
        if home_name == "link/../home" {
            let non_unicode = home.path().join(std::ffi::OsStr::from_bytes(b"home-\xff"));
            std::fs::create_dir_all(non_unicode.join("child")).expect("non-Unicode parent");
            std::fs::create_dir(non_unicode.join("home")).expect("non-Unicode home");
            std::os::unix::fs::symlink(non_unicode.join("child"), home.path().join("link"))
                .expect("Unicode alias");
        }
        let executable = home.path().join("updater");
        std::fs::write(
        &executable,
        b"#!/bin/sh\ntest -d \"$CODEX_HOME\" || exit 1\n{ pwd -P; printf '%s\\n' \"$CODEX_HOME\" \"$CODEX_SQLITE_HOME\" \"$AWS_CONFIG_FILE\" \"$SSL_CERT_DIR\" \"$NPM_CONFIG_CAFILE\"; } > \"$CODEX_TEST_UPDATER_OUTPUT\"\n",
    )
    .expect("updater shim");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("executable");
        let output = state.join("output");
        let project_path = project.canonicalize().expect("project path");
        let ca_file = if delete_cwd {
            project_path.join("ca.pem").into_os_string()
        } else {
            " ca.pem ".into()
        };
        let cert_dir = if delete_cwd {
            project_path.join("certs")
        } else {
            std::path::PathBuf::from("certs")
        };
        let result = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", "update_loop::tests::updater_reexec_child"])
            .current_dir(&project)
            .env("CODEX_TEST_UPDATER_EXECUTABLE", &executable)
            .env("CODEX_TEST_UPDATER_STATE", &state)
            .env("CODEX_TEST_UPDATER_OUTPUT", &output)
            .env("CODEX_TEST_UPDATER_DELETE_CWD", delete_cwd.to_string())
            .env("CODEX_HOME", format!("../{home_name}"))
            .env("CODEX_SQLITE_HOME", " sqlite ")
            .env("AWS_CONFIG_FILE", "~/config")
            .env("NPM_CONFIG_CAFILE", ca_file)
            .env(
                "SSL_CERT_DIR",
                std::env::join_paths([&cert_dir, &state]).unwrap(),
            )
            .output()
            .expect("spawn updater test");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let state = state.canonicalize().expect("state path");
        let expected = [
            if delete_cwd { &state } else { &project_path }
                .display()
                .to_string(),
            if home_name == "link/../home" {
                project_path.join(format!("../{home_name}"))
            } else {
                home.path()
                    .canonicalize()
                    .expect("parent path")
                    .join(home_name)
            }
            .display()
            .to_string(),
            " sqlite ".to_string(),
            "~/config".to_string(),
            std::env::join_paths([project_path.join("certs"), home.path().join("state")])
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            project_path.join("ca.pem").display().to_string(),
        ]
        .join("\n")
            + "\n";
        assert_eq!(
            std::fs::read_to_string(output).expect("updater output"),
            expected
        );
    }
}

#[cfg(unix)]
#[test]
fn updater_reexec_child() {
    use std::os::unix::process::CommandExt;

    let Some(executable) = std::env::var_os("CODEX_TEST_UPDATER_EXECUTABLE") else {
        return;
    };
    let state = std::env::var_os("CODEX_TEST_UPDATER_STATE").expect("state path");
    if std::env::var("CODEX_TEST_UPDATER_DELETE_CWD").as_deref() == Ok("true") {
        // Model the initial background launch while the relative home is resolvable.
        let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
        command
            .args(["--exact", "update_loop::tests::updater_reexec_child"])
            .env("CODEX_TEST_UPDATER_DELETE_CWD", "prepared");
        crate::background_command::set_working_directory(
            &mut command,
            std::path::Path::new(&state),
        )
        .expect("prepare background launch");
        panic!("failed to launch prepared updater: {}", command.exec());
    }
    if std::env::var("CODEX_TEST_UPDATER_DELETE_CWD").as_deref() == Ok("prepared") {
        std::fs::remove_dir(std::env::current_dir().expect("project cwd"))
            .expect("delete updater cwd");
    }
    super::reexec_managed_updater(
        std::path::Path::new(&executable),
        &install_context(InstallMethod::Other),
        std::path::Path::new(&state),
    )
    .expect("reexec updater");
    panic!("successful exec does not return");
}

struct FakeInstallerHttp {
    response: InstallerResponse,
    requested_urls: Mutex<Vec<String>>,
}

#[cfg(unix)]
#[tokio::test]
async fn explicit_update_rejects_the_standalone_updater_for_every_fork_state() {
    for report in check_explicit_update(LifecycleFault::None).await {
        report.assert_cleanup();
        for outcome in report.outcomes {
            outcome.expect("le verifiche del trigger devono passare");
        }
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, PartialEq)]
enum LifecycleFault {
    None,
    UpdaterStartAndBackendStop,
    Trigger,
    ControlPanic,
}

#[cfg(unix)]
#[derive(Debug, Default)]
struct LifecycleReport {
    outcomes: Vec<Result<(), String>>,
    backend_stop_attempted: bool,
    updater_stop_attempted: bool,
    control_joined: bool,
    control_panicked: bool,
    backend_alive_at_updater_failure: bool,
    cleanup: Vec<String>,
}

#[cfg(unix)]
impl LifecycleReport {
    fn assert_cleanup(&self) {
        assert_eq!(self.cleanup, Vec::<String>::new());
    }
}

#[cfg(unix)]
async fn check_explicit_update(fault: LifecycleFault) -> Vec<LifecycleReport> {
    let cases = if fault == LifecycleFault::None {
        vec![(false, false), (true, false), (false, true), (true, true)]
    } else {
        vec![(true, false)]
    };
    let mut reports = Vec::new();
    for (running, local) in cases {
        let mut report = LifecycleReport::default();
        let mut saved_pid_record = None;
        let mut saved_permissions = None;
        let home = TempDir::new().unwrap();
        let (daemon, release) = manual_update_daemon(&home);
        let root = home.path().join("packages/app-server-daemon-vl");
        if local {
            let package = root.join("releases/local-development");
            std::fs::create_dir_all(package.join("bin")).unwrap();
            codex_utils_cargo_bin::copy_executable(
                &daemon.managed_codex_bin,
                &package.join("bin/codex"),
            )
            .unwrap();
            std::fs::copy(
                root.join("releases")
                    .join(&release)
                    .join("codex-package.json"),
                package.join("codex-package.json"),
            )
            .unwrap();
            std::fs::remove_file(root.join("current")).unwrap();
            std::os::unix::fs::symlink(&package, root.join("current")).unwrap();
            std::fs::remove_file(root.join("auto-update-version")).unwrap();
        }
        // Baseline collected as a result: an error here excludes the triggers
        // but cleanup of the processes that were started still runs.
        let baseline = (|| -> Result<(std::path::PathBuf, Option<Vec<u8>>, Vec<u8>), String> {
            let previous = root
                .join("current")
                .canonicalize()
                .map_err(|error| format!("current: {error}"))?;
            let marker_before = std::fs::read(root.join("auto-update-version")).ok();
            let bin_before = std::fs::read(&daemon.managed_codex_bin)
                .map_err(|error| format!("managed binary: {error}"))?;
            Ok((previous, marker_before, bin_before))
        })();
        let settings = format!(
            r#"{{"updater":{{"autoUpdateEnabled":{running}}},"remoteControlEnabled":true}}"#
        );
        std::fs::write(&daemon.settings_file, &settings).unwrap();
        let daemon_settings = daemon.load_settings().await.unwrap();
        let old_backend = crate::backend::pid_backend(daemon.backend_paths(&daemon_settings));
        let old_updater =
            crate::backend::pid_update_loop_backend(daemon.backend_paths(&daemon_settings));
        // Avvii sequenziali: se il secondo fallisce, il primo viene fermato
        // nel cleanup comune.
        let mut started_backend = false;
        let mut started_updater = false;
        let mut outcomes: Vec<Result<(), String>> = Vec::new();
        let server = if running {
            if let Err(error) = old_backend.start().await {
                outcomes.push(Err(format!("backend start: {error:#}")));
                None
            } else {
                started_backend = true;
                let updater_start = async {
                    if fault == LifecycleFault::UpdaterStartAndBackendStop {
                        saved_pid_record = Some(std::fs::read(&daemon.pid_file)?);
                        saved_permissions =
                            Some(std::fs::metadata(&daemon.managed_codex_bin)?.permissions());
                        std::fs::set_permissions(
                            &daemon.managed_codex_bin,
                            <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(
                                0o644,
                            ),
                        )?;
                    }
                    old_updater.start().await
                }
                .await;
                if let Err(error) = updater_start {
                    // Il backend resta segnato come avviato: il cleanup comune
                    // lo fermera' e raccogliera' l'esito dello stop.
                    outcomes.push(Err(format!("updater start: {error:#}")));
                    report.backend_alive_at_updater_failure =
                        old_backend.is_starting_or_running().await.unwrap_or(false);
                    None
                } else {
                    // Il setup del control e' fallibile: raccolto, e se fallisce
                    // i processi gia' vivi vengono fermati (flag coerenti) prima
                    // della valutazione.
                    started_updater = true;
                    match test_control_server(&daemon, home.path()).await {
                        Ok(handle) => {
                            if fault == LifecycleFault::ControlPanic {
                                let task = tokio::spawn(async move {
                                    handle.abort();
                                    let joined = handle.await;
                                    if let Err(error) = joined
                                        && !error.is_cancelled()
                                    {
                                        panic!("unexpected original control failure: {error}");
                                    }
                                    panic!("injected control panic");
                                });
                                let finished =
                                    tokio::time::timeout(Duration::from_secs(5), async {
                                        while !task.is_finished() {
                                            tokio::task::yield_now().await;
                                        }
                                    })
                                    .await;
                                if finished.is_err() {
                                    outcomes.push(Err("control panic injection timed out".into()));
                                }
                                Some(task)
                            } else {
                                Some(handle)
                            }
                        }
                        Err(error) => {
                            outcomes.push(Err(format!("control server setup: {error:#}")));
                            if started_updater {
                                if let Err(stop_error) = old_updater.stop().await {
                                    outcomes.push(Err(format!("stop updater: {stop_error:#}")));
                                }
                                started_updater = false;
                            }
                            if let Err(stop_error) = old_backend.stop().await {
                                outcomes.push(Err(format!("stop backend: {stop_error:#}")));
                            }
                            started_backend = false;
                            None
                        }
                    }
                }
            }
        } else {
            None
        };
        // Il record PID di riferimento e' quello scritto dall'eventuale
        // avvio: il baseline pre-avvio sarebbe gia' obsoleto.
        let pid_record_before = if started_backend {
            std::fs::read(&daemon.pid_file).ok()
        } else {
            None
        };
        if let Some(permissions) = saved_permissions.take()
            && let Err(error) = std::fs::set_permissions(&daemon.managed_codex_bin, permissions)
        {
            report
                .cleanup
                .push(format!("restore executable permissions: {error}"));
        }
        match &baseline {
            Err(error) => outcomes.push(Err(error.clone())),
            Ok((previous, marker_before, bin_before)) => {
                // Verifiche per trigger raccolte in un risultato: Scheduled,
                // Manual e RestoreProduction devono essere rifiutati dalla
                // policy fork PRIMA di qualunque HTTP, per ogni stato
                // running/stopped e locale/stable, con settings, selezione,
                // marker e bytes intatti.
                async fn check_trigger<'a>(
                    http: &'a FakeInstallerHttp,
                    daemon: &'a crate::Daemon,
                    root: &'a std::path::Path,
                    previous: &'a std::path::Path,
                    pid_record_before: &'a Option<Vec<u8>>,
                    marker_before: &'a Option<Vec<u8>>,
                    bin_before: &'a [u8],
                    settings_before: &'a str,
                    home: &'a std::path::Path,
                    trigger: super::UpdateTrigger<'a>,
                ) -> Result<(), String> {
                    let identity = executable_identity(&daemon.managed_codex_bin)
                        .await
                        .map_err(|error| format!("executable identity: {error}"))?;
                    let outcome =
                        super::update_once(http, daemon, &identity, &mut test_terminate(), trigger)
                            .await;
                    if trigger == super::UpdateTrigger::Scheduled {
                        // The refusal is deterministic for the fork: a
                        // scheduled tick must stop the loop instead of
                        // failing again on every interval.
                        let (control, restart) = outcome
                            .map_err(|error| format!("scheduled refusal errored: {error:#}"))?;
                        if !matches!(control, super::UpdateLoopControl::Stop) {
                            return Err("a scheduled refusal must stop the update loop".to_string());
                        }
                        if restart.is_some() {
                            return Err("a scheduled refusal must not restart anything".to_string());
                        }
                    } else {
                        let error = outcome
                            .err()
                            .ok_or_else(|| "la policy fork deve rifiutare l'updater".to_string())?;
                        let message = format!("{error:#}");
                        if !message.contains("standalone auto-updater is disabled") {
                            return Err(format!("manca il rifiuto disabled: {message}"));
                        }
                        if !message.contains("@mmmbuto/codex-vl@latest") {
                            return Err(format!("manca il nome del pacchetto npm: {message}"));
                        }
                    }
                    if !http.requested_urls().is_empty() {
                        return Err(format!(
                            "richieste HTTP inattese: {:?}",
                            http.requested_urls()
                        ));
                    }
                    if root
                        .join("current")
                        .canonicalize()
                        .map_err(|error| format!("current: {error}"))?
                        != previous
                    {
                        return Err("current cambiato dal trigger".into());
                    }
                    if std::fs::read(root.join("auto-update-version")).ok() != *marker_before {
                        return Err("marker cambiato dal trigger".into());
                    }
                    if std::fs::read(daemon.managed_codex_bin.as_path())
                        .map_err(|error| format!("managed binary: {error}"))?
                        != bin_before
                    {
                        return Err("bytes del binario gestito cambiati dal trigger".into());
                    }
                    if crate::managed_install::package_root(home) != root {
                        return Err("root del fork cambiato dal trigger".into());
                    }
                    if std::fs::read_to_string(&daemon.settings_file)
                        .map_err(|error| format!("settings: {error}"))?
                        != settings_before
                    {
                        return Err("settings cambiati dal trigger".into());
                    }
                    if std::fs::read(daemon.pid_file.as_path()).ok() != *pid_record_before {
                        return Err("record PID cambiato dal trigger".into());
                    }
                    Ok(())
                }
                for trigger in [
                    super::UpdateTrigger::Scheduled,
                    super::UpdateTrigger::Manual,
                    super::UpdateTrigger::RestoreProduction(&release),
                ] {
                    let http = FakeInstallerHttp::new(InstallerResponse::Success(
                        b"# CODEX_INSTALL_IF_LATEST\nexit 0\n".to_vec(),
                    ));
                    let checked = check_trigger(
                        &http,
                        &daemon,
                        &root,
                        previous,
                        &pid_record_before,
                        marker_before,
                        bin_before,
                        &settings,
                        home.path(),
                        trigger,
                    )
                    .await;
                    outcomes.push(if fault == LifecycleFault::Trigger && checked.is_ok() {
                        Err("injected trigger verification failure".into())
                    } else {
                        checked
                    });
                }
            }
        }

        // I processi vivi restano vivi: la verifica entra nella raccolta,
        // PRIMA del cleanup che li ferma.
        outcomes.push(
            async {
                let backend_alive = old_backend
                    .is_starting_or_running()
                    .await
                    .map_err(|error| format!("backend state: {error:#}"))?;
                let updater_alive = old_updater
                    .is_starting_or_running()
                    .await
                    .map_err(|error| format!("updater state: {error:#}"))?;
                let updater_expected =
                    running && fault != LifecycleFault::UpdaterStartAndBackendStop;
                if (backend_alive, updater_alive) != (running, updater_expected) {
                    return Err("un processo vivo non resta vivo dopo i trigger".into());
                }
                Ok(())
            }
            .await,
        );

        // Cleanup SEMPRE: stop tentati entrambi con esiti raccolti, control
        // abortito; la valutazione arriva dopo la raccolta.
        if started_updater {
            report.updater_stop_attempted = true;
            if let Err(error) = old_updater.stop().await {
                outcomes.push(Err(format!("stop updater: {error:#}")));
            }
        }
        if started_backend {
            let inject_stop = if fault == LifecycleFault::UpdaterStartAndBackendStop {
                (|| -> std::io::Result<()> {
                    std::fs::remove_file(&daemon.pid_file)?;
                    std::fs::create_dir(&daemon.pid_file)
                })()
            } else {
                Ok(())
            };
            if let Err(error) = inject_stop {
                outcomes.push(Err(format!("stop injection setup: {error}")));
            }
            report.backend_stop_attempted = true;
            if let Err(error) = old_backend.stop().await {
                outcomes.push(Err(format!("stop backend: {error:#}")));
            }
        }
        if let Some(server) = server {
            server.abort();
            // Il control viene anche UNITO: cancellazione intenzionale
            // accettata, panic del task raccolto come fallimento.
            let join_esito = match tokio::time::timeout(Duration::from_secs(5), server).await {
                Err(_) => Some("control server non termina entro 5s dall'abort".into()),
                Ok(joined) => {
                    report.control_joined = true;
                    match joined {
                        Ok(()) => None,
                        Err(join_error) if join_error.is_cancelled() => None,
                        Err(join_error) => {
                            report.control_panicked = join_error.is_panic();
                            Some(format!("control server task: {join_error}"))
                        }
                    }
                }
            };
            if let Some(error) = join_esito {
                outcomes.push(Err(error));
            }
        }
        // Restore the exact record before the identity-aware rescue stop.
        // Capture a missed common stop before rescue, so regressions fail
        // after every live fixture has been given a chance to shut down.
        if let Some(bytes) = saved_pid_record {
            let restored = (|| -> std::io::Result<()> {
                if daemon.pid_file.is_dir() {
                    std::fs::remove_dir(&daemon.pid_file)?;
                }
                std::fs::write(&daemon.pid_file, bytes)
            })();
            if let Err(error) = restored {
                report.cleanup.push(format!("restore PID record: {error}"));
            }
        }
        for (name, backend) in [("backend", &old_backend), ("updater", &old_updater)] {
            match backend.is_starting_or_running().await {
                Ok(true) if fault != LifecycleFault::UpdaterStartAndBackendStop => {
                    report
                        .cleanup
                        .push(format!("{name} still active after common cleanup"));
                }
                Err(error) => report.cleanup.push(format!("{name} state: {error:#}")),
                _ => {}
            }
            if let Err(error) = backend.stop().await {
                report
                    .cleanup
                    .push(format!("rescue stop {name}: {error:#}"));
            }
            match backend.is_starting_or_running().await {
                Ok(false) => {}
                state => report
                    .cleanup
                    .push(format!("{name} not stopped after rescue: {state:?}")),
            }
        }
        if home.path().join("packages/standalone").exists()
            || home.path().join("packages/app-server-daemon").exists()
        {
            outcomes.push(Err("upstream package root created".into()));
        }
        report.outcomes = outcomes;
        reports.push(report);
    }
    reports
}

impl FakeInstallerHttp {
    fn new(response: InstallerResponse) -> Self {
        Self {
            response,
            requested_urls: Mutex::new(Vec::new()),
        }
    }

    fn requested_urls(&self) -> Vec<String> {
        self.requested_urls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl InstallerHttp for FakeInstallerHttp {
    async fn get(&self, url: &str) -> anyhow::Result<InstallerResponse> {
        self.requested_urls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(url.to_string());
        Ok(self.response.clone())
    }
}

#[cfg(unix)]
#[tokio::test]
async fn cancelling_installer_stops_children_and_releases_fallback_lock() {
    let home = tempfile::TempDir::new().expect("home");
    let ready = home.path().join("ready");
    let delayed = home.path().join("delayed");
    let lock = home.path().join("packages/standalone/install.lock.d");
    let script = format!(
        "mkdir -p '{lock}'\necho $$ > '{lock}/pid'\n(trap '' TERM; echo ready > '{ready}'; sleep 4; echo late > '{delayed}') &\nwait\n",
        lock = lock.display(),
        ready = ready.display(),
        delayed = delayed.display(),
    );
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    let ready_for_signal = ready.clone();
    let signal_sender = tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !ready_for_signal.exists() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "installer did not start"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        cancel.send(()).expect("cancel installer");
    });
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        super::run_installer_script(
            script.as_bytes(),
            super::InstallerMode::Update("0.150.0-test"),
            &home.path().join("packages/standalone"),
            async { cancelled.await.ok() },
        ),
    )
    .await
    .expect("installer cancellation timed out")
    .expect("installer cancellation failed");
    signal_sender.await.expect("signal sender");
    assert!(matches!(result, super::UpdateLoopControl::Stop));
    assert!(!lock.exists());
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(!delayed.exists());
}

#[cfg(unix)]
fn manual_update_daemon(home: &TempDir) -> (Daemon, String) {
    let target = if cfg!(target_os = "macos") {
        format!("{}-apple-darwin", std::env::consts::ARCH)
    } else {
        format!("{}-unknown-linux-musl", std::env::consts::ARCH)
    };
    let release = format!("1.0.0-{target}");
    let standalone = home.path().join("packages/app-server-daemon-vl");
    let bin = standalone
        .join("releases")
        .join(&release)
        .join("bin")
        .join("codex");
    std::fs::create_dir_all(bin.parent().expect("binary parent")).expect("release directory");
    codex_utils_cargo_bin::write_executable(
        &bin,
        "#!/bin/sh\nif [ \"$1\" = '--version' ]; then echo codex 1.0.0; else exec sleep 30; fi\n",
    )
    .expect("managed binary");
    // Variant manifest next to bin/: the daemon L1 check
    // accepts this selection only with the codex-vl variant.
    std::fs::write(
        standalone.join("releases").join(&release).join("codex-package.json"),
        format!(
            r#"{{"layoutVersion":1,"version":"{release}","target":"{target}","variant":"codex-vl","entrypoint":"bin/codex"}}"#
        ),
    )
    .expect("fork manifest");
    std::os::unix::fs::symlink(format!("releases/{release}"), standalone.join("current"))
        .expect("current release");
    std::fs::write(standalone.join("auto-update-version"), &release).expect("latest marker");
    let state = home.path().join("app-server-daemon-vl");
    std::fs::create_dir(&state).unwrap();
    std::fs::write(state.join("app-server.stderr.log"), b"").unwrap();
    (
        Daemon {
            log_diagnostics: false,
            socket_path: home.path().join("app-server-control-vl/server.sock"),
            pid_file: state.join("app-server.pid"),
            update_pid_file: state.join("app-server-updater.pid"),
            operation_lock_file: state.join("daemon.lock"),
            settings_file: state.join("settings.json"),
            managed_codex_bin: standalone.join("current/bin/codex"),
            install_context: install_context(InstallMethod::Other),
            launch_grant: None,
        },
        release,
    )
}

#[cfg(unix)]
fn test_terminate() -> tokio::signal::unix::Signal {
    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install test signal handler")
}

#[cfg(unix)]
#[tokio::test]
async fn manual_request_retries_after_updater_replacement() {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    let home = TempDir::new().expect("home");
    let (daemon, _) = manual_update_daemon(&home);
    let socket_path = daemon.manual_update_socket_path();
    codex_uds::prepare_private_socket_directory(socket_path.parent().expect("socket parent"))
        .await
        .expect("socket directory");
    let mut listener = codex_uds::UnixListener::bind(&socket_path)
        .await
        .expect("old updater socket");
    let expected = UpdateOutput {
        status: UpdateStatus::NoUpdate,
        managed_codex_path: daemon.managed_codex_bin.clone(),
        installed_version: None,
        running_version: None,
        message: "already current".to_string(),
    };
    let reply = expected.clone();
    let server = tokio::spawn(async move {
        let mut old = listener.accept().await.expect("first connection");
        let mut request = [0; 7];
        old.read_exact(&mut request).await.expect("first request");
        drop(old);
        drop(listener);
        tokio::fs::remove_file(&socket_path)
            .await
            .expect("remove old socket");
        let mut successor = codex_uds::UnixListener::bind(&socket_path)
            .await
            .expect("successor socket");
        let mut connection = successor.accept().await.expect("retried connection");
        connection
            .read_exact(&mut request)
            .await
            .expect("retried request");
        connection
            .write_all(&serde_json::to_vec(&Ok::<_, String>(reply)).expect("serialize response"))
            .await
            .expect("send response");
    });
    assert_eq!(
        super::manual_update::request(&daemon)
            .await
            .expect("request survives handoff"),
        expected
    );
    server.await.expect("replacement task");
}

#[cfg(unix)]
#[tokio::test]
async fn manual_request_accepts_maximum_installer_stderr_detail() {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    let home = TempDir::new().expect("home");
    let (daemon, _) = manual_update_daemon(&home);
    let socket_path = daemon.manual_update_socket_path();
    codex_uds::prepare_private_socket_directory(socket_path.parent().expect("socket parent"))
        .await
        .expect("socket directory");
    let mut listener = codex_uds::UnixListener::bind(&socket_path)
        .await
        .expect("updater socket");
    let expected_suffix = "installer failed";
    let message = format!(
        "standalone updater failed:\n{}{}",
        "\0".repeat(super::INSTALLER_STDERR_TAIL_BYTES),
        expected_suffix,
    );
    let response = serde_json::to_vec(&Err::<UpdateOutput, _>(message)).expect("serialize error");
    assert!(response.len() < super::manual_update::MAX_RESPONSE_BYTES as usize);
    let server = tokio::spawn(async move {
        let mut connection = listener.accept().await.expect("request connection");
        let mut request = [0; 7];
        connection.read_exact(&mut request).await.expect("request");
        connection
            .write_all(&response)
            .await
            .expect("send response");
    });

    let error = super::manual_update::request(&daemon)
        .await
        .expect_err("manual update should fail");
    assert!(error.to_string().ends_with(expected_suffix));
    server.await.expect("server task");
}

#[cfg(unix)]
#[tokio::test]
async fn manual_request_recovers_when_one_shot_updater_exits() {
    use tokio::io::AsyncReadExt;

    let home = TempDir::new().expect("home");
    let (daemon, _) = manual_update_daemon(&home);
    let socket_path = daemon.manual_update_socket_path();
    codex_uds::prepare_private_socket_directory(socket_path.parent().expect("socket parent"))
        .await
        .expect("socket directory");
    let mut listener = codex_uds::UnixListener::bind(&socket_path)
        .await
        .expect("one-shot updater socket");
    let server = tokio::spawn(async move {
        let mut connection = listener.accept().await.expect("request connection");
        let mut request = [0; 7];
        connection.read_exact(&mut request).await.expect("request");
        drop(connection);
        drop(listener);
        tokio::fs::remove_file(socket_path)
            .await
            .expect("remove exited updater socket");
    });
    // Without the selected executable, the startup path reports unsupported. A
    // retry that only waits for a successor would time out instead.
    std::fs::remove_file(&daemon.managed_codex_bin).expect("remove selected binary");
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        super::manual_update::request(&daemon),
    )
    .await
    .expect("retry should return to normal startup")
    .expect("unsupported response");
    assert_eq!(result.status, UpdateStatus::Unsupported);
    server.await.expect("updater task");
}

#[cfg(unix)]
#[tokio::test]
async fn unsupported_request_preserves_updater_schedule() {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    let home = TempDir::new().expect("home");
    let (daemon, _) = manual_update_daemon(&home);
    let daemon = std::sync::Arc::new(daemon);
    let identity = executable_identity(&daemon.managed_codex_bin)
        .await
        .expect("updater identity");
    let socket_path = daemon.manual_update_socket_path();
    let http = FakeInstallerHttp::new(InstallerResponse::Success(Vec::new()));
    let updater_daemon = std::sync::Arc::clone(&daemon);
    let worker = tokio::spawn(async move {
        super::run_with_http(
            &http,
            &updater_daemon,
            &identity,
            /*restore_release*/ None,
        )
        .await
    });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !socket_path.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "updater did not listen"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // An external installer can pin while this ordinary worker still listens.
    // Raw IPC must not gain the manual CLI's authority to restore production.
    let marker = super::selected_release(&daemon)
        .unwrap()
        .0
        .join("auto-update-version");
    let previous_marker = std::fs::read(&marker).unwrap();
    std::fs::remove_file(&marker).unwrap();
    let mut pinned = codex_uds::UnixStream::connect(&socket_path).await.unwrap();
    pinned.write_all(b"update\n").await.unwrap();
    let mut response = Vec::new();
    pinned.read_to_end(&mut response).await.unwrap();
    let response: Result<UpdateOutput, String> = serde_json::from_slice(&response).unwrap();
    assert_eq!(response.unwrap().status, UpdateStatus::Unsupported);
    assert!(!marker.exists());
    std::fs::write(&marker, previous_marker).unwrap();
    std::fs::remove_file(&daemon.managed_codex_bin).expect("remove selected binary");
    let mut malformed = codex_uds::UnixStream::connect(&socket_path)
        .await
        .expect("connect malformed request");
    malformed
        .write_all(b"upd")
        .await
        .expect("send partial request");
    malformed.shutdown().await.expect("disconnect request");
    let mut discarded = Vec::new();
    malformed
        .read_to_end(&mut discarded)
        .await
        .expect("rejected request");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!worker.is_finished(), "malformed request stopped updater");
    assert_eq!(
        super::manual_update::request(&daemon)
            .await
            .expect("unsupported response")
            .status,
        UpdateStatus::Unsupported
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!worker.is_finished(), "unsupported request stopped updater");
    worker.abort();
}

/// When the update loop stops running it must take its PID record with it: a
/// record naming a dead updater makes readers believe one is alive (here the
/// record names this very test process, so the leftover is indistinguishable
/// from a live updater until the loop removes it).
#[cfg(unix)]
#[tokio::test]
async fn exiting_updater_removes_its_own_pid_record() {
    let home = TempDir::new().expect("home");
    let (daemon, _) = manual_update_daemon(&home);
    let identity = executable_identity(&daemon.managed_codex_bin)
        .await
        .expect("updater identity");
    let socket_path = daemon.manual_update_socket_path();
    std::fs::write(
        &daemon.update_pid_file,
        serde_json::json!({"pid": std::process::id(), "processStartTime": "recorded-start"})
            .to_string(),
    )
    .expect("own pid record");

    let http = FakeInstallerHttp::new(InstallerResponse::Success(Vec::new()));
    let worker_daemon = daemon.clone();
    let worker = tokio::spawn(async move {
        super::run_with_http(
            &http,
            &worker_daemon,
            &identity,
            /*restore_release*/ None,
        )
        .await
    });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !socket_path.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "updater did not listen"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    worker.abort();
    let _ = worker.await;

    assert!(
        !daemon.update_pid_file.exists(),
        "an exited updater must not leave its own pid record behind"
    );
    let settings = daemon.load_settings().await.expect("settings");
    assert!(
        !crate::backend::pid_update_loop_backend(daemon.backend_paths(&settings))
            .is_starting_or_running()
            .await
            .expect("updater state"),
        "a removed record must not read as a live updater"
    );

    // A successor's record must survive this updater's exit untouched.
    std::fs::remove_file(&socket_path).expect("remove socket from the first run");
    std::fs::write(
        &daemon.update_pid_file,
        serde_json::json!({"pid": u32::MAX, "processStartTime": "successor-start"}).to_string(),
    )
    .expect("successor pid record");
    let http = FakeInstallerHttp::new(InstallerResponse::Success(Vec::new()));
    let worker_daemon = daemon.clone();
    let successor_identity = executable_identity(&daemon.managed_codex_bin)
        .await
        .expect("updater identity");
    let worker = tokio::spawn(async move {
        super::run_with_http(&http, &worker_daemon, &successor_identity, None).await
    });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !socket_path.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "updater did not listen again"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    worker.abort();
    let _ = worker.await;

    assert!(
        daemon.update_pid_file.exists(),
        "an exiting updater must not delete a successor's pid record"
    );
    std::fs::remove_file(&daemon.update_pid_file).expect("clean successor record");
}

#[cfg(unix)]
async fn test_control_server(
    daemon: &Daemon,
    home: &std::path::Path,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    use anyhow::Context as _;
    use futures::SinkExt;
    use futures::StreamExt;
    std::fs::create_dir_all(daemon.socket_path.parent().context("socket parent")?)?;
    let mut listener = codex_uds::UnixListener::bind(&daemon.socket_path)
        .await
        .context("control listener")?;
    let codex_home = home.to_path_buf();
    Ok(tokio::spawn(async move {
        loop {
            let connection = listener.accept().await.expect("control connection");
            let mut websocket = tokio_tungstenite::accept_async(connection)
                .await
                .expect("websocket handshake");
            websocket
                .next()
                .await
                .expect("initialize request")
                .expect("frame");
            let version = if std::fs::read_to_string(
                crate::managed_install::package_root(&codex_home).join("auto-update-version"),
            )
            .unwrap_or_default()
            .starts_with("1.1.0")
            {
                "1.1.0"
            } else {
                "1.0.0"
            };
            websocket.send(tokio_tungstenite::tungstenite::Message::Text(
                serde_json::json!({"id": 1, "result": {
                    "userAgent": format!("codex_app_server_daemon/{version}"),
                    "codexHome": codex_home, "platformFamily": "unix", "platformOs": std::env::consts::OS,
                }}).to_string().into(),
            )).await.expect("initialize response");
            websocket
                .next()
                .await
                .expect("initialized notification")
                .expect("frame");
        }
    }))
}

// Upstream test for the standalone installer. This fork refuses that installer:
// update_loop.rs `install_latest_standalone` bails before any HTTP work and a
// scheduled tick stops the loop at the first refusal (`update_once`), while
// lib.rs `ensure_managed_updater` never spawns the updater for package-manager
// installs. The scheduled no-op and resource-only restart paths are therefore
// unreachable here; the fork contract is covered by
// `scheduled_refusal_stops_the_update_loop`.
#[cfg(unix)]
#[tokio::test]
#[ignore = "fork: no standalone installer"]
async fn scheduled_no_op_preserves_daemon_and_hands_off_stale_updater() {
    let home = TempDir::new().expect("home");
    let (daemon, _) = manual_update_daemon(&home);
    let settings = crate::settings::DaemonSettings::default();
    let backend = crate::backend::pid_backend(daemon.backend_paths(&settings));
    backend.start().await.expect("start daemon");
    let server = test_control_server(&daemon, home.path())
        .await
        .expect("control server");
    let pid_record = std::fs::read(&daemon.pid_file).expect("daemon PID record");
    let no_op = FakeInstallerHttp::new(InstallerResponse::Success(
        b"# CODEX_INSTALL_IF_LATEST\nexit 0\n".to_vec(),
    ));

    let outcome = super::update_once(
        &no_op,
        &daemon,
        &executable_identity_from_reader(&b"stale updater"[..]).expect("updater identity"),
        &mut test_terminate(),
        super::UpdateTrigger::Scheduled,
    )
    .await
    .expect("scheduled update");

    assert!(matches!(
        outcome,
        (
            super::UpdateLoopControl::Continue,
            Some(RestartIfRunningOutcome::AlreadyCurrent)
        )
    ));
    assert_eq!(
        std::fs::read(&daemon.pid_file).expect("daemon PID record"),
        pid_record
    );
    backend.stop().await.expect("stop daemon");
    server.abort();
}

// Upstream test for the standalone installer. This fork refuses that installer:
// update_loop.rs `install_latest_standalone` bails before any HTTP work and a
// scheduled tick stops the loop at the first refusal (`update_once`), while
// lib.rs `ensure_managed_updater` never spawns the updater for package-manager
// installs. The scheduled no-op and resource-only restart paths are therefore
// unreachable here; the fork contract is covered by
// `scheduled_refusal_stops_the_update_loop`.
#[cfg(unix)]
#[tokio::test]
#[ignore = "fork: no standalone installer"]
async fn scheduled_resource_only_update_restarts_daemon() {
    let home = TempDir::new().expect("home");
    let (daemon, release) = manual_update_daemon(&home);
    let settings = crate::settings::DaemonSettings::default();
    let backend = crate::backend::pid_backend(daemon.backend_paths(&settings));
    backend.start().await.expect("start daemon");
    let server = test_control_server(&daemon, home.path())
        .await
        .expect("control server");
    let previous_pid_record = std::fs::read(&daemon.pid_file).expect("daemon PID record");
    let root = home.path().join("packages/standalone");
    let next_release = release.replacen("1.0.0", "1.0.1", 1);
    let installer = FakeInstallerHttp::new(InstallerResponse::Success(
        format!(
            "#!/bin/sh\n# CODEX_INSTALL_IF_LATEST\nmkdir -p '{root}/releases/{next_release}'\ncp '{root}/releases/{release}/codex' '{root}/releases/{next_release}/codex'\nln -sfn 'releases/{next_release}' '{root}/current'\nprintf '{next_release}' > '{root}/auto-update-version'\n",
            root = root.display(),
        )
        .into_bytes(),
    ));

    let outcome = super::update_once(
        &installer,
        &daemon,
        &executable_identity(&daemon.managed_codex_bin)
            .await
            .expect("updater identity"),
        &mut test_terminate(),
        super::UpdateTrigger::Scheduled,
    )
    .await
    .expect("scheduled update");

    assert!(matches!(
        outcome,
        (
            super::UpdateLoopControl::Continue,
            Some(RestartIfRunningOutcome::Restarted)
        )
    ));
    assert_ne!(
        std::fs::read(&daemon.pid_file).expect("daemon PID record"),
        previous_pid_record
    );
    backend.stop().await.expect("stop daemon");
    server.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn manual_update_restarts_managed_daemon_with_automatic_updates_disabled() {
    check_manual_update_restart(false, RequestFault::None)
        .await
        .assert_success();
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_start_and_restart_preserve_launch_features() {
    for features in [
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::from([
            ("api_key_model_discovery".to_string(), true),
            ("code_mode_host".to_string(), false),
        ]),
    ] {
        let home = TempDir::new().unwrap();
        let (daemon, _) = manual_update_daemon(&home);
        let args_path = home.path().join("launch-args");
        std::fs::write(
        &daemon.settings_file,
        r#"{"featureOverrides":{"auth_elicitation":true},"updater":{"autoUpdateEnabled":false}}"#,
    )
    .unwrap();
        codex_utils_cargo_bin::write_executable(&daemon.managed_codex_bin, &format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo codex 1.0.0; exit; fi\nif [ \"$3\" = --help ]; then exit; fi\nprintf '%s\\n' \"$@\" > '{}'\nexec sleep 30\n",
        args_path.display(),
    )).unwrap();
        // Coordination with an explicit result: start runs in a task and
        // the control server comes up at the first sign of launch (args written).
        // NO assert before cleanup: task errors and panics stay
        // collected data, and the verdict comes after the shared cleanup
        // phase (daemon stopped, control aborted).
        let start_daemon = daemon.clone();
        let start_features = features.clone();
        let mut start_task = tokio::spawn(async move { start_daemon.start(&start_features).await });
        let control = async {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(/*secs*/ 10);
            while !args_path.exists() {
                if tokio::time::Instant::now() >= deadline {
                    return Err("daemon did not launch".to_string());
                }
                tokio::time::sleep(Duration::from_millis(/*millis*/ 20)).await;
            }
            Ok(test_control_server(&daemon, home.path())
                .await
                .map_err(|error| format!("control server: {error}"))?)
        };
        tokio::pin!(control);
        let mut started_result;
        let mut control_result;
        tokio::select! {
            result = &mut start_task => {
                started_result = Some(match result {
                    Ok(joined) => joined.map_err(|error| format!("daemon start failed: {error:#}")),
                    Err(join_error) => Err(format!("start task panicking: {join_error}")),
                });
                control_result = Some(control.await);
            }
            handle = &mut control => {
                control_result = Some(handle);
                // The control timed out without seeing the args: start is
                // awaited with its own limit and stopped if it runs out of time.
                started_result = Some(match tokio::time::timeout(
                    Duration::from_secs(/*secs*/ 5),
                    &mut start_task,
                )
                .await
                {
                    Ok(joined) => joined
                        .map_err(|error| format!("start task join: {error}"))
                        .and_then(|joined| {
                            joined.map_err(|error| format!("daemon start failed: {error:#}"))
                        }),
                    Err(_) => {
                        start_task.abort();
                        start_task
                            .await
                            .map_err(|error| format!("start task join: {error}"))
                            .and_then(|joined| {
                                joined.map_err(|error| {
                                    format!("daemon start failed: {error:#}")
                                })
                            })
                    }
                });
            }
        }

        // Verification phase: collected into a result, with ? instead of
        // asserts, so the cleanup below still runs on failure.
        let checks: Result<(), String> = async {
            let _control = control_result
                .as_ref()
                .expect("control outcome collected")
                .as_ref()
                .map_err(|error| format!("control server: {error}"))?;
            let started = started_result
                .as_ref()
                .expect("start outcome collected")
                .as_ref()
                .map_err(|error| error.clone())?;
            if started.status != crate::LifecycleStatus::Started {
                return Err(format!("stato di avvio inatteso: {:?}", started.status));
            }
            let loaded = daemon
                .load_settings()
                .await
                .map_err(|error| format!("load settings failed: {error:#}"))?;
            if loaded.feature_overrides != features {
                return Err("feature overrides cambiate dopo l'avvio".into());
            }
            let expected = if features.is_empty() {
                "app-server\n--listen\nunix://\n--managed-daemon\n"
            } else {
                "app-server\n--listen\nunix://\n-c\nfeatures.api_key_model_discovery=true\n-c\nfeatures.code_mode_host=false\n--managed-daemon\n"
            };
            let args = std::fs::read_to_string(&args_path)
                .map_err(|error| format!("launch args unreadable: {error}"))?;
            if args != expected {
                return Err(format!("launch args inattesi: {args:?}"));
            }
            let reused = daemon
                .start(&std::collections::BTreeMap::from([(
                    "api_key_model_discovery".to_string(),
                    false,
                )]))
                .await
                .map_err(|error| format!("reuse start failed: {error:#}"))?;
            if reused.status != crate::LifecycleStatus::AlreadyRunning {
                return Err(format!("stato di riuso inatteso: {:?}", reused.status));
            }
            let loaded = daemon
                .load_settings()
                .await
                .map_err(|error| format!("load settings failed: {error:#}"))?;
            if loaded.feature_overrides != features {
                return Err("feature overrides cambiate dal riuso".into());
            }
            let args = std::fs::read_to_string(&args_path)
                .map_err(|error| format!("launch args unreadable: {error}"))?;
            if args != expected {
                return Err(format!("launch args inattesi dopo il riuso: {args:?}"));
            }
            std::fs::remove_file(&args_path)
                .map_err(|error| format!("remove args failed: {error}"))?;
            let restarted = daemon
                .restart()
                .await
                .map_err(|error| format!("restart failed: {error:#}"))?;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(/*secs*/ 10);
            let mut args_after: Option<String> = None;
            while tokio::time::Instant::now() < deadline {
                if let Ok(text) = std::fs::read_to_string(&args_path) {
                    if text == expected {
                        args_after = Some(text);
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(/*millis*/ 20)).await;
            }
            let args = args_after
                .ok_or_else(|| "il restart non ha riscritto gli argomenti entro la scadenza".to_string())?;
            if restarted.status != crate::LifecycleStatus::Restarted {
                return Err(format!("stato di restart inatteso: {:?}", restarted.status));
            }
            if args != expected {
                return Err(format!("launch args inattesi dopo il restart: {args:?}"));
            }
            Ok(())
        }
        .await;

        // Cleanup comune (anche su fallimento): daemon fermato con esito
        // raccolto, control abortito E unito (join raccolto); solo dopo la
        // valutazione del risultato raccolto e degli esiti di cleanup.
        let stop_result = daemon.stop().await;
        if let Some(Ok(handle)) = &control_result {
            handle.abort();
        }
        let control_join = if let Some(Ok(handle)) = control_result.take() {
            match handle.await {
                Ok(()) => None,
                Err(join_error) if join_error.is_cancelled() => None,
                Err(join_error) => Some(format!("control join: {join_error}")),
            }
        } else {
            None
        };
        checks.expect("le verifiche di avvio/riavvio devono passare");
        if let Some(error) = control_join {
            panic!("control server join: {error}");
        }
        if let Err(error) = stop_result {
            panic!("daemon stop failed: {error:#}");
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn confirmed_feature_restart_preserves_ownership_and_skips_matching_settings() {
    use crate::LifecycleStatus;
    use std::collections::BTreeMap;

    for managed in [true, false] {
        let home = TempDir::new().unwrap();
        let (daemon, _) = manual_update_daemon(&home);
        std::fs::write(&daemon.settings_file,
            r#"{"featureOverrides":{"auth_elicitation":true,"api_key_model_discovery":true},"updater":{"autoUpdateEnabled":false},"shutdownGraceSeconds":0}"#
        ).unwrap();
        let original = daemon.load_settings().await.unwrap();
        if managed {
            daemon.start_managed_backend(&original).await.unwrap();
        }
        let server = test_control_server(&daemon, home.path())
            .await
            .expect("control server");
        let _lock = daemon.acquire_operation_lock().await.unwrap();
        let requested = BTreeMap::from([
            ("api_key_model_discovery".to_string(), false),
            ("mcp_oauth_refresh_coordination".to_string(), true),
        ]);
        if managed {
            // Hide the selection without removing the script the spawned shell still needs.
            let selected_package = daemon.managed_codex_bin.parent().unwrap();
            let saved_package = selected_package.with_extension("saved");
            std::fs::rename(selected_package, &saved_package).unwrap();
            let error = daemon
                .restart_with_features_locked(&requested)
                .await
                .unwrap_err();
            std::fs::rename(saved_package, selected_package).unwrap();
            assert!(
                error
                    .to_string()
                    .contains("managed standalone install not found"),
                "{error:#}"
            );
            assert_eq!(daemon.load_settings().await.unwrap(), original);
        }
        let result = daemon.restart_with_features_locked(&requested).await;
        if managed {
            assert_eq!(result.unwrap().status, LifecycleStatus::Restarted);
            let pid = std::fs::read(&daemon.pid_file).unwrap();
            let mut expected = original;
            expected.feature_overrides.extend(requested.clone());
            assert_eq!(daemon.load_settings().await.unwrap(), expected);
            assert_eq!(
                daemon
                    .restart_with_features_locked(&requested)
                    .await
                    .unwrap()
                    .status,
                LifecycleStatus::AlreadyRunning
            );
            assert_eq!(std::fs::read(&daemon.pid_file).unwrap(), pid);
            daemon.stop().await.unwrap();
        } else {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("no running managed daemon")
            );
            assert_eq!(daemon.load_settings().await.unwrap(), original);
        }
        server.abort();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn manual_update_restarts_local_daemon_with_automatic_updates_disabled() {
    check_manual_update_restart(true, RequestFault::None)
        .await
        .assert_success();
}

#[cfg(unix)]
#[path = "update_cleanup_tests.rs"]
mod cleanup_tests;

#[cfg(unix)]
#[derive(Debug, PartialEq)]
enum RequestObservation {
    NotReached,
    Rejected,
    Panic,
    TimeoutCancelled,
}

#[cfg(unix)]
#[derive(Debug)]
struct ManualReport {
    scenario: Result<(), String>,
    request: RequestObservation,
    request_joins: usize,
    worker_joined: bool,
    control_joined: bool,
    backend_inactive: Result<bool, String>,
    cleanup: Vec<String>,
}

#[cfg(unix)]
impl ManualReport {
    fn assert_cleanup(&self) {
        assert_eq!(self.cleanup, Vec::<String>::new());
        assert_eq!(self.backend_inactive, Ok(true));
        assert_eq!(self.request_joins, 1);
        assert!(self.worker_joined, "updater join missing: {self:?}");
        assert!(self.control_joined, "control join missing: {self:?}");
    }

    fn assert_success(self) {
        self.assert_cleanup();
        assert_eq!(self.request, RequestObservation::Rejected);
        self.scenario.expect("richiesta rifiutata dalla guardia");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn request_task_panic_is_collected_and_cleanup_still_runs() {
    let report = check_manual_update_restart(false, RequestFault::Panic).await;
    report.assert_cleanup();
    assert_eq!(report.request, RequestObservation::Panic);
    let error = report
        .scenario
        .expect_err("injected request panic must be collected");
    assert!(error.starts_with("request task panicking:"), "{error}");
    assert!(
        error.contains("iniezione: panic nel task di richiesta"),
        "{error}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn request_task_hang_reaches_abort_and_cleanup_still_runs() {
    let report = check_manual_update_restart(true, RequestFault::Hang).await;
    report.assert_cleanup();
    assert_eq!(report.request, RequestObservation::TimeoutCancelled);
    assert_eq!(
        report.scenario,
        Err("request timeout followed by intentional cancellation".into())
    );
}

#[cfg(unix)]
#[tokio::test]
async fn updater_start_and_backend_stop_failures_are_both_recorded() {
    let reports = check_explicit_update(LifecycleFault::UpdaterStartAndBackendStop).await;
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    report.assert_cleanup();
    assert!(
        report.backend_alive_at_updater_failure,
        "backend responsibility lost: {report:?}"
    );
    assert!(
        report.backend_stop_attempted,
        "common backend stop missing: {report:?}"
    );
    assert!(!report.updater_stop_attempted);
    let errors: Vec<_> = report
        .outcomes
        .iter()
        .filter_map(|result| result.as_ref().err())
        .collect();
    assert_eq!(errors.len(), 2, "{report:?}");
    assert!(errors[0].starts_with("updater start:"), "{report:?}");
    assert!(errors[0].contains("Permission denied"), "{report:?}");
    assert!(errors[1].starts_with("stop backend:"), "{report:?}");
    assert!(errors[1].contains("Is a directory"), "{report:?}");
    assert_eq!(
        report
            .outcomes
            .iter()
            .filter(|result| result.is_ok())
            .count(),
        4
    );
}

#[cfg(unix)]
#[derive(Clone, Copy, PartialEq)]
enum RequestFault {
    /// Comportamento reale: richiesta rifiutata dalla guardia.
    None,
    /// Il task di richiesta PANICA: il primo join consuma il risultato — il
    /// flag di attesa va impostato PRIMA di analizzarlo, altrimenti il
    /// cleanup ripolla un JoinHandle completato e tokio panica.
    Panic,
    /// Il task di richiesta resta appeso: il primo tetto scade e il cleanup
    /// deve abortire e attendere l'handle.
    Hang,
}

#[cfg(unix)]
async fn check_manual_update_restart(local_package: bool, fault: RequestFault) -> ManualReport {
    let home = TempDir::new().expect("home");
    let (mut daemon, mut release) = manual_update_daemon(&home);
    let standalone = home.path().join("packages/app-server-daemon-vl");
    if local_package {
        let local = format!("local-development-{release}");
        std::fs::rename(
            standalone.join("releases").join(&release),
            standalone.join("releases").join(&local),
        )
        .unwrap();
        std::fs::remove_file(standalone.join("current")).unwrap();
        std::os::unix::fs::symlink(format!("releases/{local}"), standalone.join("current"))
            .unwrap();
        std::fs::remove_file(standalone.join("auto-update-version")).unwrap();
        daemon.managed_codex_bin = standalone.join("current/bin/codex");
        release = local;
    }
    let daemon = std::sync::Arc::new(daemon);
    std::fs::create_dir_all(daemon.settings_file.parent().expect("state directory"))
        .expect("state directory");
    std::fs::write(
        &daemon.settings_file,
        r#"{"updater":{"autoUpdateEnabled":false}}"#,
    )
    .expect("disabled updater");

    // Handle delle risorse avviate: restano qui perche' il cleanup le vedra'
    // anche se l'esecuzione fallisce a meta' strada.
    let server = test_control_server(&daemon, home.path()).await;
    let settings = crate::settings::DaemonSettings::default();
    let backend = crate::backend::pid_backend(daemon.backend_paths(&settings));
    let http = std::sync::Arc::new(FakeInstallerHttp::new(InstallerResponse::Success(
        b"#!/bin/sh\n# CODEX_INSTALL_IF_LATEST CODEX_INSTALL_IF_CURRENT CODEX_INSTALL_DAEMON_ONLY\nexit 0\n".to_vec(),
    )));
    let mut worker_handle: Option<tokio::task::JoinHandle<anyhow::Result<()>>> = None;
    let mut request_handle: Option<tokio::task::JoinHandle<anyhow::Result<crate::UpdateOutput>>> =
        None;
    // La richiesta IPC e' gia' stata ATTESA nel percorso principale: il
    // cleanup non deve ripollare un JoinHandle completato (tokio panica).
    let mut request_finished = false;
    let mut request_observation = RequestObservation::NotReached;
    let mut request_joins = 0;

    // Da qui le risorse sono vive: precondizioni, updater, attesa del socket,
    // richiesta IPC, oracolo e invarianti vengono raccolti in UN unico
    // risultato; il cleanup sotto gira comunque, anche su fallimenti, e
    // raccoglie i propri esiti, valutati solo alla fine.
    let outcome: Result<(), String> = async {
        backend
            .start()
            .await
            .map_err(|error| format!("start daemon: {error:#}"))?;
        let current_pid = || -> Result<u64, String> {
            let record = std::fs::read(&daemon.pid_file)
                .map_err(|error| format!("daemon PID record: {error}"))?;
            let parsed = serde_json::from_slice::<serde_json::Value>(&record)
                .map_err(|error| format!("PID JSON: {error}"))?;
            parsed["pid"]
                .as_u64()
                .ok_or_else(|| "PID mancante nel record".to_string())
        };
        let before = current_pid()?;
        let bin_before = std::fs::read(&daemon.managed_codex_bin)
            .map_err(|error| format!("managed binary: {error}"))?;
        let current_before = std::fs::read_link(standalone.join("current"))
            .map_err(|error| format!("current symlink: {error}"))?;
        let marker_before = std::fs::read(standalone.join("auto-update-version")).ok();
        let settings_before = std::fs::read_to_string(&daemon.settings_file)
            .map_err(|error| format!("settings before: {error}"))?;

        // Ownership ed eligibilità attestate PRIMA di updater e richiesta IPC:
        // errori raccolti, non assert.
        let backend_state = daemon
            .running_backend_instance(&settings)
            .await
            .map_err(|error| format!("backend state: {error:#}"))?;
        if backend_state.is_none() {
            return Err("il backend deve essere riconosciuto come gestito dal daemon".into());
        }
        let selected = std::fs::canonicalize(&daemon.managed_codex_bin)
            .map_err(|error| format!("canonical selection: {error}"))?;
        if !selected.starts_with(standalone.join("releases")) {
            return Err(format!("la selezione deve restare dentro releases: {selected:?}"));
        }
        if marker_before.is_some() == local_package {
            return Err("il marker stable deve corrispondere a local_package".into());
        }

        // Nessun installer deve partire: se il updater chiamasse l'HTTP,
        // l'elenco delle richieste renderebbe la prova fallita.
        let updater_daemon = std::sync::Arc::clone(&daemon);
        let restore_release = local_package.then(|| release.clone());
        let worker_http = std::sync::Arc::clone(&http);
        worker_handle = Some(tokio::spawn(async move {
            super::run_with_http(
                worker_http.as_ref(),
                &updater_daemon,
                &executable_identity_from_reader(&b"updater"[..]).expect("updater identity"),
                restore_release,
            )
            .await
        }));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !daemon.manual_update_socket_path().exists() {
            if tokio::time::Instant::now() >= deadline {
                return Err("updater did not listen".into());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        // Richiesta manuale REALE via IPC: handle tenuta in variabile e attesa
        // con timeout su &mut; sul timeout abort e attesa del task.
        let request_daemon = std::sync::Arc::clone(&daemon);
        request_handle = Some(tokio::spawn(async move {
            match fault {
                RequestFault::None => super::manual_update::request(&request_daemon).await,
                RequestFault::Panic => panic!("iniezione: panic nel task di richiesta"),
                RequestFault::Hang => {
                    std::future::pending::<anyhow::Result<crate::UpdateOutput>>().await
                }
            }
        }));
        let outcome_ipc = match tokio::time::timeout(
            Duration::from_secs(5),
            request_handle.as_mut().expect("request spawned"),
        )
        .await
        {
            Ok(joined) => {
                // Il risultato del JoinHandle e' stato consumato: il cleanup
                // non deve ripollarlo (tokio panicherebbe).
                request_finished = true;
                request_joins += 1;
                match joined {
                    Ok(inner) => inner,
                    Err(join_error) => {
                        if join_error.is_panic() {
                            request_observation = RequestObservation::Panic;
                        }
                        return Err(format!("request task panicking: {join_error}"));
                    }
                }
            }
            Err(_) => {
                if let Some(handle) = request_handle.as_ref() {
                    handle.abort();
                }
                match tokio::time::timeout(
                    Duration::from_secs(5),
                    request_handle.as_mut().expect("request spawned"),
                )
                .await
                {
                    Ok(joined) => {
                        request_finished = true;
                        request_joins += 1;
                        match joined {
                            Ok(inner) => inner,
                            Err(join_error) if join_error.is_cancelled() => {
                                request_observation = RequestObservation::TimeoutCancelled;
                                return Err("request timeout followed by intentional cancellation".into());
                            }
                            Err(join_error) => {
                                return Err(format!("request task failed after abort: {join_error}"));
                            }
                        }
                    }
                    Err(_) => {
                        // Ancora pendente: resta al cleanup, che lo abortira'
                        // e lo attendera'.
                        return Err("la richiesta IPC non termina dopo l'abort".into());
                    }
                }
            }
        };
        // Oracolo raccolto: un Ok qui e' la richiesta ACCETTATA invece di
        // essere rifiutata dalla guardia — un fallimento, non un panic che
        // salta il cleanup.
        let error = match outcome_ipc {
            Err(error) => error,
            Ok(output) => {
                return Err(format!(
                    "la richiesta IPC e' stata accettata invece di essere rifiutata: status {:?}, messaggio {}",
                    output.status, output.message
                ));
            }
        };
        let message = format!("{error:#}");
        if fault == RequestFault::None {
            if !message.contains("standalone auto-updater is disabled") {
                return Err(format!("manca il rifiuto disabled: {message}"));
            }
            if !message.contains("@mmmbuto/codex-vl@latest") {
                return Err(format!("manca il nome del pacchetto npm: {message}"));
            }
        }

        request_observation = RequestObservation::Rejected;
        // Invarianti post-rifiuto, raccolte senza assert panikanti.
        let urls = http.requested_urls();
        if !urls.is_empty() {
            return Err(format!("richieste HTTP inattese: {urls:?}"));
        }
        let pid_after = current_pid()?;
        if pid_after != before {
            return Err(format!("PID cambiato: {pid_after} != {before}"));
        }
        let bin_after = std::fs::read(&daemon.managed_codex_bin)
            .map_err(|error| format!("managed binary: {error}"))?;
        if bin_after != bin_before {
            return Err("bytes del binario gestito cambiati".into());
        }
        let current_after = std::fs::read_link(standalone.join("current"))
            .map_err(|error| format!("current symlink: {error}"))?;
        if current_after != current_before {
            return Err(format!("current cambiato: {current_after:?} != {current_before:?}"));
        }
        let marker_after = std::fs::read(standalone.join("auto-update-version")).ok();
        if marker_after != marker_before {
            return Err("marker cambiato".into());
        }
        if local_package && marker_before.is_some() {
            return Err("la release locale non deve avere un pin".into());
        }
        let settings_after = std::fs::read_to_string(&daemon.settings_file)
            .map_err(|error| format!("settings after: {error}"))?;
        if settings_after != settings_before {
            return Err("settings cambiati".into());
        }
        Ok(())
    }
    .await;

    // Cleanup COMUNE anche su fallimenti: stop backend (esito raccolto),
    // abort e join della richiesta, abort e join del worker distinguendo
    // Ok(Ok(())) da Ok(Err) e dai JoinError, abort e join del control; gli
    // errori di cleanup si valutano solo alla fine.
    let stop_result = backend.stop().await;
    let backend_inactive = backend
        .is_starting_or_running()
        .await
        .map(|active| !active)
        .map_err(|error| format!("backend state: {error:#}"));
    // Rescue is separate from the observed cleanup: a broken stop must fail
    // the oracle without leaving the fixture process alive.
    let rescue_result = backend.stop().await;
    if let Some(handle) = request_handle.as_ref() {
        handle.abort();
    }
    let request_error = match request_handle.take() {
        None => None,
        Some(mut handle) => {
            if request_finished {
                // Gia' atteso nel percorso principale: ripollare un JoinHandle
                // completato farebbe panicare tokio.
                None
            } else {
                handle.abort();
                match tokio::time::timeout(Duration::from_secs(5), &mut handle).await {
                    Err(_) => Some("la richiesta IPC non termina dopo l'abort".into()),
                    Ok(joined) => {
                        request_joins += 1;
                        match joined {
                            Ok(_) => None,
                            Err(join_error) if join_error.is_cancelled() => None,
                            Err(join_error) => {
                                Some(format!("request task panicking: {join_error}"))
                            }
                        }
                    }
                }
            }
        }
    };
    if let Some(handle) = &worker_handle {
        handle.abort();
    }
    let mut worker_joined = false;
    let worker_error = match worker_handle.take() {
        None => None,
        Some(mut handle) => match tokio::time::timeout(Duration::from_secs(5), &mut handle).await {
            Err(_) => Some("updater worker non terminato entro 5s dopo l'abort".into()),
            Ok(joined) => {
                worker_joined = true;
                match joined {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(format!("loop updater fallito: {error:#}")),
                    Err(join_error) if join_error.is_cancelled() => None,
                    Err(join_error) => Some(format!("updater worker panicking: {join_error}")),
                }
            }
        },
    };
    let mut server_error: Option<String> = None;
    match &server {
        Ok(handle) => handle.abort(),
        Err(error) => server_error = Some(format!("control server setup: {error:#}")),
    }
    let mut control_joined = false;
    let server_join = match server {
        Ok(mut handle) => match tokio::time::timeout(Duration::from_secs(5), &mut handle).await {
            Ok(joined) => {
                control_joined = true;
                match joined {
                    Ok(()) => None,
                    Err(join_error) if join_error.is_cancelled() => None,
                    Err(join_error) => Some(format!("control server task: {join_error}")),
                }
            }
            Err(_) => Some("control server join timed out".into()),
        },
        Err(_) => None,
    };

    let mut cleanup = Vec::new();
    for result in [stop_result, rescue_result] {
        if let Err(error) = result {
            cleanup.push(format!("stop daemon: {error:#}"));
        }
    }
    cleanup.extend(
        [request_error, worker_error, server_join, server_error]
            .into_iter()
            .flatten(),
    );
    ManualReport {
        scenario: outcome,
        request: request_observation,
        request_joins,
        worker_joined,
        control_joined,
        backend_inactive,
        cleanup,
    }
}

#[cfg(unix)]
#[tokio::test]
async fn manual_update_is_disabled_even_with_auto_updates_off() {
    let home = TempDir::new().expect("home");
    let (daemon, _) = manual_update_daemon(&home);
    std::fs::create_dir_all(daemon.settings_file.parent().expect("state directory"))
        .expect("state directory");
    std::fs::write(
        &daemon.settings_file,
        r#"{"updater":{"autoUpdateEnabled":false}}"#,
    )
    .expect("disabled updater");
    let http = FakeInstallerHttp::new(InstallerResponse::Success(Vec::new()));

    let error = manual_update_once(
        &http,
        &daemon,
        &executable_identity_from_reader(&b"updater"[..]).expect("updater identity"),
        &mut test_terminate(),
        super::UpdateTrigger::Manual,
    )
    .await
    .expect_err("manual standalone updates must also fail closed");

    let message = format!("{error:#}");
    assert!(message.contains("codex-vl fork"), "message: {message}");
    assert!(message.contains("disabled"), "message: {message}");
    assert!(
        http.requested_urls().is_empty(),
        "a disabled fork update must never fetch an installer"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn powershell_installer_is_noninteractive_and_reports_script_failure() {
    let valid = FakeInstallerHttp::new(InstallerResponse::Success(
        br#"
function Test-Installer {
    if ($env:CODEX_NON_INTERACTIVE -ne '1') { throw 'interactive installer' }
    if ($env:CODEX_INSTALL_DAEMON_ONLY -ne '1') { throw 'wrong package destination' }
    if ($env:CODEX_INSTALL_IF_CURRENT -ne '1' -or $env:CODEX_INSTALL_IF_LATEST -ne '0') { throw 'wrong update guard' }
}
Test-Installer
"#
        .to_vec(),
    ));
    let script = super::fetch_installer_script(&valid)
        .await
        .expect("fetch installer");
    super::run_installer_script(
        &script,
        super::InstallerMode::RestoreProduction("0.150.0-x86_64-pc-windows-msvc"),
        std::path::Path::new("packages/app-server-daemon"),
    )
    .await
    .expect("installer succeeds");
    let failing = FakeInstallerHttp::new(InstallerResponse::Success(
        b"throw ([string][char]0x00E9 + 'chec installation')".to_vec(),
    ));
    let script = super::fetch_installer_script(&failing)
        .await
        .expect("fetch failing installer");
    let error = super::run_installer_script(
        &script,
        super::InstallerMode::RestoreProduction("0.150.0-x86_64-pc-windows-msvc"),
        std::path::Path::new("packages/app-server-daemon"),
    )
    .await
    .err()
    .expect("installer should fail");
    assert!(error.to_string().contains("échec installation"));
}

#[cfg(unix)]
#[tokio::test]
async fn installer_failure_includes_stderr() {
    let script = format!(
        "printf 'discard me{}' >&2\nprintf 'installer failed' >&2\nexit 1\n",
        "x".repeat(super::INSTALLER_STDERR_TAIL_BYTES),
    );
    let error = super::run_installer_script(
        script.as_bytes(),
        super::InstallerMode::Update("0.150.0-x86_64-unknown-linux-gnu"),
        std::path::Path::new("packages/app-server-daemon"),
        futures::future::pending(),
    )
    .await
    .err()
    .expect("installer should fail");
    let error = error.to_string();
    assert!(error.contains("installer failed"));
    assert!(!error.contains("discard me"));
}

#[cfg(unix)]
#[tokio::test]
async fn installer_failure_preserves_stderr_before_drain_timeout() {
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        super::run_installer_script(
            b"printf 'installer failed' >&2\nsleep 5 &\nexit 1\n",
            super::InstallerMode::Update("0.150.0-x86_64-unknown-linux-gnu"),
            std::path::Path::new("packages/app-server-daemon"),
            futures::future::pending(),
        ),
    )
    .await
    .expect("stderr drain should time out")
    .err()
    .expect("installer should fail");
    assert!(error.to_string().contains("installer failed"));
}

#[cfg(unix)]
#[tokio::test]
async fn update_rejects_a_package_root_change_during_download() {
    // The fork guard runs before any HTTP: the fake client counts
    // requests and must never be queried. If the download started,
    // the fake installer would try to remove the fork's current (the
    // sentinel mutation the real path should detect): the avoided
    // risk stays explicit in the body of get.
    struct CountingInstaller {
        requested_urls: Mutex<Vec<String>>,
        fork_current: PathBuf,
    }
    impl InstallerHttp for CountingInstaller {
        async fn get(&self, url: &str) -> anyhow::Result<InstallerResponse> {
            self.requested_urls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(url.to_string());
            let _ = std::fs::remove_file(&self.fork_current);
            Ok(InstallerResponse::Success(
                b"# CODEX_INSTALL_IF_LATEST\nexit 99\n".to_vec(),
            ))
        }
    }
    let home = TempDir::new().unwrap();
    let (daemon, release) = manual_update_daemon(&home);
    let identity = executable_identity(&daemon.managed_codex_bin)
        .await
        .unwrap();
    let bin_before = std::fs::read(&daemon.managed_codex_bin).unwrap();
    let http = CountingInstaller {
        requested_urls: Mutex::new(Vec::new()),
        fork_current: home.path().join("packages/app-server-daemon-vl/current"),
    };
    impl CountingInstaller {
        fn requested_urls(&self) -> Vec<String> {
            self.requested_urls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }
    let error = manual_update_once(
        &http,
        &daemon,
        &identity,
        &mut test_terminate(),
        super::UpdateTrigger::Manual,
    )
    .await
    .unwrap_err();
    let message = format!("{error:#}");
    assert!(
        message.contains("standalone auto-updater is disabled"),
        "{message}"
    );
    assert!(message.contains("@mmmbuto/codex-vl@latest"), "{message}");
    assert!(
        http.requested_urls().is_empty(),
        "la guardia deve precedere ogni HTTP: {} richieste",
        http.requested_urls().len()
    );
    // Root e selezione invariati.
    assert_eq!(
        crate::managed_install::package_root(home.path()),
        home.path().join("packages/app-server-daemon-vl")
    );
    assert_eq!(
        std::fs::read_link(home.path().join("packages/app-server-daemon-vl/current")).unwrap(),
        PathBuf::from(format!("releases/{release}"))
    );
    assert_eq!(
        std::fs::read(&daemon.managed_codex_bin).unwrap(),
        bin_before
    );
    // Nessuno dei due root upstream nasce per strada.
    assert!(!home.path().join("packages/standalone").exists());
    assert!(!home.path().join("packages/app-server-daemon").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_owned_updates_require_and_request_an_isolated_installer() {
    let home = TempDir::new().unwrap();
    let (daemon, release) = manual_update_daemon(&home);
    // Il root e` quello del factory (-vl) e la selezione e` current/bin/codex:
    // nessun rename, nessun packages upstream.
    let root = home.path().join("packages/app-server-daemon-vl");
    assert_eq!(crate::managed_install::package_root(home.path()), root);
    assert_eq!(
        std::fs::read_link(root.join("current")).unwrap(),
        PathBuf::from(format!("releases/{release}"))
    );
    let identity = executable_identity(&daemon.managed_codex_bin)
        .await
        .unwrap();
    let marker = root.join("auto-update-version");
    let bin_before = std::fs::read(&daemon.managed_codex_bin).unwrap();

    // Rifiuto PRIMA di HTTP per qualunque preferenza di auto-update e marker:
    // nessuna richiesta, nessun installer, nessun packages upstream creato,
    // settings e selezione immutati. L'oracolo e` DISTINTO per scenario:
    // marker stable presente -> la guardia disabled rifiuta; marker assente
    // -> Unsupported prima della guardia (release dentro releases, ma senza
    // stable latest, con trigger Manual).
    for auto_update_enabled in [false, true] {
        for marker_present in [false, true] {
            let settings =
                format!(r#"{{"updater":{{"autoUpdateEnabled":{auto_update_enabled}}}}}"#);
            std::fs::write(&daemon.settings_file, &settings).unwrap();
            if marker_present {
                std::fs::write(&marker, release.as_bytes()).unwrap();
            } else {
                std::fs::remove_file(&marker).ok();
            }
            // Precondizioni dello scenario, attestate prima della chiamata:
            // selezione dentro releases (supported), release stable numerica,
            // marker coerente con lo scenario.
            let selected = std::fs::canonicalize(&daemon.managed_codex_bin).unwrap();
            assert!(
                selected.starts_with(root.join("releases")),
                "la selezione deve restare dentro releases: {selected:?}"
            );
            assert!(release.starts_with("1.0.0-"), "release stable numerica");
            assert_eq!(std::fs::read(&marker).is_ok(), marker_present);
            let http = FakeInstallerHttp::new(InstallerResponse::Success(
                b"# CODEX_INSTALL_IF_LATEST\ntest \"$CODEX_INSTALL_IF_LATEST\" = 1 && test \"$CODEX_INSTALL_DAEMON_ONLY\" = 1\n".to_vec(),
            ));
            let outcome = manual_update_once(
                &http,
                &daemon,
                &identity,
                &mut test_terminate(),
                super::UpdateTrigger::Manual,
            )
            .await;
            if marker_present {
                let error = outcome
                    .expect_err("Manual con marker stable: la guardia disabled deve rifiutare");
                assert!(
                    error
                        .to_string()
                        .contains("standalone auto-updater is disabled"),
                    "{error:#}"
                );
                assert!(
                    error.to_string().contains("@mmmbuto/codex-vl@latest"),
                    "{error:#}"
                );
            } else {
                let output = outcome.expect(
                    "Manual senza marker: Unsupported prima della guardia (release dentro releases, stable assente)",
                );
                assert_eq!(output.status, UpdateStatus::Unsupported);
                assert!(
                    output.message.contains("managed releases directory"),
                    "message: {}",
                    output.message
                );
            }
            // Invarianze dopo ciascuna chiamata: zero HTTP, selezione, marker
            // (presenza e contenuto), bytes, settings, nessun root upstream.
            assert!(
                http.requested_urls().is_empty(),
                "nessuna richiesta HTTP: {:?}",
                http.requested_urls()
            );
            assert_eq!(
                std::fs::read_link(root.join("current")).unwrap(),
                PathBuf::from(format!("releases/{release}"))
            );
            assert_eq!(std::fs::read(&marker).is_ok(), marker_present);
            if marker_present {
                assert_eq!(std::fs::read(&marker).unwrap(), release.as_bytes());
            }
            assert_eq!(
                std::fs::read(&daemon.managed_codex_bin).unwrap(),
                bin_before
            );
            assert_eq!(
                std::fs::read_to_string(&daemon.settings_file).unwrap(),
                settings
            );
            assert!(!home.path().join("packages/standalone").exists());
            assert!(!home.path().join("packages/app-server-daemon").exists());
        }
    }

    // RestoreProduction with the marker absent: the trigger reaches the guard
    // (the selection is inside releases and supported by the trigger) and the
    // refusal is disabled, with the npm package name. The pin stays absent and
    // no upstream root is created.
    let restore = FakeInstallerHttp::new(InstallerResponse::Success(
        b"# CODEX_INSTALL_IF_CURRENT CODEX_INSTALL_DAEMON_ONLY\nexit 0\n".to_vec(),
    ));
    for auto_update_enabled in [false, true] {
        let settings = format!(r#"{{"updater":{{"autoUpdateEnabled":{auto_update_enabled}}}}}"#);
        std::fs::write(&daemon.settings_file, &settings).unwrap();
        std::fs::remove_file(&marker).ok();
        let error = manual_update_once(
            &restore,
            &daemon,
            &identity,
            &mut test_terminate(),
            super::UpdateTrigger::RestoreProduction(&release),
        )
        .await
        .expect_err("RestoreProduction con marker assente: la guardia disabled deve rifiutare");
        assert!(
            error
                .to_string()
                .contains("standalone auto-updater is disabled"),
            "{error:#}"
        );
        assert!(
            error.to_string().contains("@mmmbuto/codex-vl@latest"),
            "{error:#}"
        );
        // Invarianze: zero HTTP, marker ancora assente, bytes e settings
        // immutati, nessuno dei due root upstream, nessun PID.
        assert!(restore.requested_urls().is_empty());
        assert!(!marker.exists());
        assert_eq!(
            std::fs::read(&daemon.managed_codex_bin).unwrap(),
            bin_before
        );
        assert_eq!(
            std::fs::read_to_string(&daemon.settings_file).unwrap(),
            settings
        );
        assert!(!home.path().join("packages/standalone").exists());
        assert!(!home.path().join("packages/app-server-daemon").exists());
        assert!(!daemon.pid_file.exists());
        assert_eq!(
            std::fs::read_link(root.join("current")).unwrap(),
            PathBuf::from(format!("releases/{release}"))
        );
    }
}
