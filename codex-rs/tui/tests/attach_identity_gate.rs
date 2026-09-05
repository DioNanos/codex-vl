use codex_tui::identity_gate_test_support::app_server_target_kind;
use codex_tui::identity_gate_test_support::maybe_probe_default_daemon_socket;
use serial_test::serial;
use tempfile::TempDir;

struct EnvGuard {
    old: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(value: &str) -> Self {
        let old = std::env::var_os("NEXUSCREW_MCP_SESSION");
        // SAFETY: these tests use `serial` and mutate only the fork-specific
        // identity key that production tests do not read concurrently.
        unsafe { std::env::set_var("NEXUSCREW_MCP_SESSION", value) };
        Self { old }
    }

    fn remove() -> Self {
        let old = std::env::var_os("NEXUSCREW_MCP_SESSION");
        // SAFETY: these tests use `serial` and mutate only the fork-specific
        // identity key that production tests do not read concurrently.
        unsafe { std::env::remove_var("NEXUSCREW_MCP_SESSION") };
        Self { old }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(old) = self.old.take() {
            // SAFETY: see `EnvGuard::set`.
            unsafe { std::env::set_var("NEXUSCREW_MCP_SESSION", old) };
        } else {
            // SAFETY: see `EnvGuard::set`.
            unsafe { std::env::remove_var("NEXUSCREW_MCP_SESSION") };
        }
    }
}

async fn bind_default_daemon_socket(
    codex_home: &TempDir,
) -> anyhow::Result<(std::path::PathBuf, tokio::net::UnixListener)> {
    let socket_path = codex_app_server_client::app_server_control_socket_path(codex_home.path())?;
    std::fs::create_dir_all(socket_path.as_path().parent().expect("socket parent"))?;
    let listener = tokio::net::UnixListener::bind(socket_path.as_path())?;
    Ok((socket_path.into_path_buf(), listener))
}

#[tokio::test]
#[serial]
async fn unverified_fleet_identity_keeps_shared_daemon_embedded() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let (socket_path, _listener) = bind_default_daemon_socket(&codex_home).await?;
    let _identity = EnvGuard::set("cell-B");
    assert_eq!(
        std::env::var_os("NEXUSCREW_MCP_SESSION").as_deref(),
        Some(std::ffi::OsStr::new("cell-B"))
    );
    let default_daemon = maybe_probe_default_daemon_socket(codex_home.path()).await;
    let kind = app_server_target_kind(
        /*explicit_socket*/ None,
        default_daemon.as_deref(),
        /*can_reuse_implicit_local_daemon*/ true,
        /*workload_identity_selected*/ false,
        /*has_fleet_identity*/ true,
    )?;

    assert_eq!(default_daemon.as_deref(), Some(socket_path.as_path()));
    assert!(matches!(
        kind,
        codex_tui::identity_gate_test_support::TargetKind::Embedded
    ));
    Ok(())
}

#[tokio::test]
#[serial]
async fn unverified_fleet_identity_rejects_explicit_shared_endpoint() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let (socket_path, _listener) = bind_default_daemon_socket(&codex_home).await?;
    let _identity = EnvGuard::set("cell-B");
    let kind = app_server_target_kind(
        Some(socket_path.as_path()),
        /*default_socket*/ None,
        /*can_reuse_implicit_local_daemon*/ false,
        /*workload_identity_selected*/ false,
        /*has_fleet_identity*/ true,
    )?;

    assert!(matches!(
        kind,
        codex_tui::identity_gate_test_support::TargetKind::Embedded
    ));
    Ok(())
}

#[tokio::test]
#[serial]
async fn tui_without_fleet_identity_attaches_to_live_shared_daemon() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let (socket_path, _listener) = bind_default_daemon_socket(&codex_home).await?;
    let _identity = EnvGuard::remove();
    assert_eq!(std::env::var_os("NEXUSCREW_MCP_SESSION"), None);
    let default_daemon = maybe_probe_default_daemon_socket(codex_home.path()).await;
    let kind = app_server_target_kind(
        /*explicit_socket*/ None,
        default_daemon.as_deref(),
        /*can_reuse_implicit_local_daemon*/ true,
        /*workload_identity_selected*/ false,
        /*has_fleet_identity*/ false,
    )?;

    assert!(matches!(
        kind,
        codex_tui::identity_gate_test_support::TargetKind::LocalDaemon
    ));
    Ok(())
}

#[tokio::test]
async fn absent_socket_keeps_fleet_tui_embedded() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let kind = app_server_target_kind(
        /*explicit_socket*/ None, /*default_socket*/ None,
        /*can_reuse_implicit_local_daemon*/ true, /*workload_identity_selected*/ false,
        /*has_fleet_identity*/ true,
    )?;

    assert!(matches!(
        kind,
        codex_tui::identity_gate_test_support::TargetKind::Embedded
    ));
    Ok(())
}
