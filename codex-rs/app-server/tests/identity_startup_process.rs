#![allow(clippy::expect_used)]

//! Process-level checks for the identity fail-closed startup gate.
//!
//! The unit tests cover the decision in isolation; these tests run the real
//! `codex-app-server` binary and assert what an operator would observe: a
//! refusal on stderr, a non-zero exit, and no socket on disk. The standalone
//! case proves the opposite direction — no declaration means a normal start.

use codex_utils_cargo_bin::cargo_bin;
use std::path::Path;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;
use std::time::Duration;
use std::time::Instant;
use tempfile::TempDir;

const REQUIRED_ENV: &str = "CODEX_APP_SERVER_IDENTITY_REQUIRED";
const FD_ENV: &str = "NEXUSCREW_IDENTITY_FD";
const BROKEN_MARKER: &str = "IDENTITY_CHANNEL_BROKEN";
const STARTUP_BUDGET: Duration = Duration::from_secs(30);

/// Build the daemon command with a private home and a socket path this test owns.
fn daemon(home: &Path, socket: &Path) -> Command {
    let mut command =
        Command::new(cargo_bin("codex-app-server").expect("locate app-server binary"));
    command
        .arg("--listen")
        .arg(format!("unix://{}", socket.display()))
        .env("CODEX_HOME", home)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env_remove("NEXUSCREW_MCP_SESSION");
    command
}

/// Wait for the process to exit, without hanging the suite if it does not.
fn wait_for_exit(child: &mut Child) -> Option<ExitStatus> {
    let deadline = Instant::now() + STARTUP_BUDGET;
    loop {
        match child.try_wait().expect("poll child") {
            Some(status) => return Some(status),
            None if Instant::now() >= deadline => return None,
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn assert_refused(output: &std::process::Output, socket: &Path, case: &str) {
    assert!(
        !output.status.success(),
        "{case}: the daemon must not report success: {}",
        stderr_of(output)
    );
    let stderr = stderr_of(output);
    assert!(
        stderr.contains(BROKEN_MARKER),
        "{case}: stderr must name the broken channel: {stderr}"
    );
    assert!(
        !socket.exists(),
        "{case}: a refused start must not leave a socket at {}",
        socket.display()
    );
}

#[test]
fn required_without_declaration_refuses_before_binding() {
    let home = TempDir::new().expect("temporary home");
    let dir = TempDir::new().expect("temporary socket dir");
    let socket = dir.path().join("refused-absent.sock");
    let output = daemon(home.path(), &socket)
        .env(REQUIRED_ENV, "1")
        .env_remove(FD_ENV)
        .output()
        .expect("run app-server");
    assert_refused(&output, &socket, "required without declaration");
}

#[test]
fn required_with_closed_declaration_refuses_before_binding() {
    let home = TempDir::new().expect("temporary home");
    let dir = TempDir::new().expect("temporary socket dir");
    let socket = dir.path().join("refused-closed.sock");
    // Descriptor numbers this process does not inherit: the declaration is
    // present but the channel behind it is closed.
    let output = daemon(home.path(), &socket)
        .env(REQUIRED_ENV, "1")
        .env(FD_ENV, "90:91")
        .output()
        .expect("run app-server");
    assert_refused(&output, &socket, "required with a closed declaration");
}

#[test]
fn standalone_starts_and_listens_without_declaration() {
    let home = TempDir::new().expect("temporary home");
    let dir = TempDir::new().expect("temporary socket dir");
    let socket = dir.path().join("standalone.sock");
    let mut child = daemon(home.path(), &socket)
        .env(REQUIRED_ENV, "0")
        .env_remove(FD_ENV)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn app-server");

    let deadline = Instant::now() + STARTUP_BUDGET;
    let mut listening = false;
    while Instant::now() < deadline {
        if socket.exists() {
            listening = true;
            break;
        }
        if let Some(status) = child.try_wait().expect("poll child") {
            panic!("standalone daemon exited early: {status}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let outcome = child.kill();
    let _ = child.wait();
    assert!(
        listening,
        "standalone daemon never listened on {}",
        socket.display()
    );
    assert!(
        outcome.is_ok(),
        "kill the standalone daemon to leave no process behind"
    );
}
