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
use crate::UpdateOutput;
#[cfg(unix)]
use crate::UpdateStatus;
#[cfg(unix)]
use crate::managed_install::executable_identity;
#[cfg(unix)]
use crate::managed_install::executable_identity_from_bytes;
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
        reexec_managed_updater(missing, &install_context(method))
            .expect("package-shim updater replacement must short-circuit");
    }
}

struct FakeInstallerHttp {
    response: InstallerResponse,
    requested_urls: Mutex<Vec<String>>,
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
        super::run_installer_script(script.as_bytes(), "0.150.0-test", home.path(), async {
            cancelled.await.ok()
        }),
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
    use std::os::unix::fs::PermissionsExt;

    let target = if cfg!(target_os = "macos") {
        format!("{}-apple-darwin", std::env::consts::ARCH)
    } else {
        format!("{}-unknown-linux-musl", std::env::consts::ARCH)
    };
    let release = format!("1.0.0-{target}");
    let standalone = home.path().join("packages/standalone");
    let bin = standalone.join("releases").join(&release).join("codex");
    std::fs::create_dir_all(bin.parent().expect("binary parent")).expect("release directory");
    std::fs::write(
        &bin,
        b"#!/bin/sh\nif [ \"$1\" = '--version' ]; then echo codex 1.0.0; else exec sleep 30; fi\n",
    )
    .expect("managed binary");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
        .expect("executable binary");
    std::os::unix::fs::symlink(format!("releases/{release}"), standalone.join("current"))
        .expect("current release");
    std::fs::write(standalone.join("auto-update-version"), &release).expect("latest marker");
    let state = home.path().join("app-server-daemon");
    (
        Daemon {
            socket_path: home.path().join("app-server-control/server.sock"),
            pid_file: state.join("app-server.pid"),
            update_pid_file: state.join("app-server-updater.pid"),
            operation_lock_file: state.join("daemon.lock"),
            settings_file: state.join("settings.json"),
            managed_codex_bin: standalone.join("current/codex"),
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
    // Without the marker, the ordinary startup path reports unsupported. A
    // retry that only waits for a successor would time out instead.
    std::fs::remove_file(home.path().join("packages/standalone/auto-update-version"))
        .expect("remove latest marker");
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
    let worker =
        tokio::spawn(async move { super::run_with_http(&http, &updater_daemon, &identity).await });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !socket_path.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "updater did not listen"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    std::fs::remove_file(home.path().join("packages/standalone/auto-update-version"))
        .expect("remove latest selection");
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
        &executable_identity_from_bytes(b"updater"),
        &mut test_terminate(),
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
}
Test-Installer
"#
        .to_vec(),
    ));
    let script = super::fetch_installer_script(&valid)
        .await
        .expect("fetch installer");
    super::run_installer_script(&script, "0.150.0-x86_64-pc-windows-msvc")
        .await
        .expect("installer succeeds");
    let failing = FakeInstallerHttp::new(InstallerResponse::Success(
        b"throw 'installer failed'".to_vec(),
    ));
    let script = super::fetch_installer_script(&failing)
        .await
        .expect("fetch failing installer");
    assert!(
        super::run_installer_script(&script, "0.150.0-x86_64-pc-windows-msvc")
            .await
            .is_err()
    );
}
