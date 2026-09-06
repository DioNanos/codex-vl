use codex_app_server_client::RemoteAppServerEndpoint;
use codex_tui::identity_gate_test_support::app_server_target_kind;
use codex_tui::identity_gate_test_support::maybe_probe_default_daemon_socket;
use codex_utils_absolute_path::AbsolutePathBuf;
use serial_test::serial;
use std::process::Command;
use tempfile::TempDir;

const DIAGNOSTIC_CHILD_ENV: &str = "CODEX_IDENTITY_GATE_DIAGNOSTIC_CHILD";

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
        /*explicit_endpoint*/ None,
        default_daemon.as_deref(),
        /*can_reuse_implicit_local_daemon*/ true,
        /*workload_identity_selected*/ false,
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
    let error = app_server_target_kind(
        Some(RemoteAppServerEndpoint::UnixSocket {
            socket_path: AbsolutePathBuf::from_absolute_path(socket_path.as_path())?,
        }),
        /*default_socket*/ None,
        /*can_reuse_implicit_local_daemon*/ false,
        /*workload_identity_selected*/ false,
    )
    .expect_err("an explicit endpoint must not fall back to local execution");

    assert_eq!(
        error.to_string(),
        "explicit app-server endpoint has no verified Fleet identity"
    );
    Ok(())
}

#[tokio::test]
#[serial]
async fn unverified_fleet_identity_rejects_explicit_websocket_with_workload_identity()
-> anyhow::Result<()> {
    let _identity = EnvGuard::set("cell-B");
    let error = app_server_target_kind(
        Some(RemoteAppServerEndpoint::WebSocket {
            websocket_url: "wss://remote.example.test:443/".to_string(),
            auth_token: None,
        }),
        /*default_socket*/ None,
        /*can_reuse_implicit_local_daemon*/ false,
        /*workload_identity_selected*/ true,
    )
    .expect_err("an explicit endpoint must not fall back to local execution");

    assert_eq!(
        error.to_string(),
        "explicit app-server endpoint has no verified Fleet identity"
    );
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
        /*explicit_endpoint*/ None,
        default_daemon.as_deref(),
        /*can_reuse_implicit_local_daemon*/ true,
        /*workload_identity_selected*/ false,
    )?;

    assert!(matches!(
        kind,
        codex_tui::identity_gate_test_support::TargetKind::LocalDaemon
    ));
    Ok(())
}

#[tokio::test]
#[serial]
async fn absent_socket_keeps_fleet_tui_embedded() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let _identity = EnvGuard::set("cell-B");
    let kind = app_server_target_kind(
        /*explicit_endpoint*/ None, /*default_socket*/ None,
        /*can_reuse_implicit_local_daemon*/ true, /*workload_identity_selected*/ false,
    )?;

    assert!(matches!(
        kind,
        codex_tui::identity_gate_test_support::TargetKind::Embedded
    ));
    Ok(())
}

#[test]
fn embedded_fallback_emits_user_visible_diagnostic() -> anyhow::Result<()> {
    if std::env::var_os(DIAGNOSTIC_CHILD_ENV).is_some() {
        let _identity = EnvGuard::set("cell-B");
        let default_socket = AbsolutePathBuf::from_absolute_path(std::path::Path::new(
            "/tmp/codex-identity-gate-default.sock",
        ))?;
        let kind = app_server_target_kind(
            /*explicit_endpoint*/ None,
            Some(default_socket.as_path()),
            /*can_reuse_implicit_local_daemon*/ true,
            /*workload_identity_selected*/ false,
        )?;
        assert!(matches!(
            kind,
            codex_tui::identity_gate_test_support::TargetKind::Embedded
        ));
        return Ok(());
    }

    let output = Command::new(std::env::current_exe()?)
        .env(DIAGNOSTIC_CHILD_ENV, "1")
        .args([
            "--exact",
            "embedded_fallback_emits_user_visible_diagnostic",
            "--nocapture",
        ])
        .output()?;
    assert!(
        output.status.success(),
        "diagnostic child failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "No verified Fleet identity for the shared app-server; using an embedded app-server."
        ),
        "fallback diagnostic was not emitted on stderr: {stderr}"
    );
    Ok(())
}

#[test]
fn d_mode_does_not_bypass_identity_required_endpoint() {
    let error =
        codex_tui::identity_gate_test_support::require_verified_identity_for_endpoint(true, false)
            .expect_err("D mode must reject an endpoint that requires binding");
    assert_eq!(
        error.to_string(),
        codex_tui::identity_gate_test_support::identity_required_endpoint_diagnostic()
    );
}

#[test]
fn mcp_initialize_identity_proof_is_forwarded_only_when_present() {
    let proof = serde_json::json!({
        "version": "1",
        "kind": "connection-v1",
        "challenge": {
            "version": "1",
            "connectionId": "connection-a",
            "daemonBootId": "boot-a",
            "audience": "daemon/a",
            "nonce": "nonce-a",
            "issuedAt": "2026-09-06T03:00:00Z",
            "expiresAt": "2026-09-06T03:00:15Z"
        },
        "claims": {
            "issuerOwner": "owner-a",
            "audience": "daemon/a",
            "ownerInstanceId": "owner-a",
            "cellId": "cell-a",
            "tmuxSession": "cloud-a",
            "incarnationId": "incarnation-a",
            "launchEpoch": "epoch-a",
            "daemonBootId": "boot-a",
            "connectionId": "connection-a",
            "bindingId": "binding-a",
            "origin": "local_tui",
            "scopes": ["thread/start"],
            "issuedAt": "2026-09-06T03:00:00Z",
            "notBefore": "2026-09-06T03:00:00Z",
            "expiresAt": "2026-09-06T03:00:15Z",
            "nonce": "nonce-a"
        },
        "proof": "authority-proof"
    });
    let mut response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {"identityBinding": {"proof": proof}}
    });
    assert!(codex_tui::identity_gate_test_support::parse_mcp_initialize_identity_proof(&response));
    response["result"]["identityBinding"]
        .as_object_mut()
        .expect("identity binding object")
        .remove("proof");
    assert!(!codex_tui::identity_gate_test_support::parse_mcp_initialize_identity_proof(&response));
}
