//! Managed daemon lifecycle through the public CLI, using isolated standalone fixtures.
#![cfg(unix)]

#[path = "app_server_daemon/support.rs"]
mod support;

use support::TestDaemon;
use support::signal;
use support::wait_for_exit;

use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use codex_utils_cargo_bin::copy_executable;
use pretty_assertions::assert_eq;
use serde_json::Value;

#[test]
fn managed_daemon_restarts_from_deleted_working_directory() -> Result<()> {
    let daemon = TestDaemon::new()?;
    let project = daemon.home.path().join("project");
    std::fs::create_dir(&project)?;
    let started = daemon
        .command()
        .current_dir(&project)
        .args(["app-server", "daemon", "start"])
        .output()?;
    ensure!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );
    let old_pid = daemon.pid("app-server.pid")?;
    let old_updater_pid = daemon.pid("app-server-updater.pid")?;
    signal(old_updater_pid, libc::SIGTERM)?;
    wait_for_exit(old_updater_pid)?;

    // Delete the restart caller's cwd without changing the test process's cwd.
    let restarted = Command::new("/bin/sh")
        .args([
            "-c",
            "rmdir \"$1\" && exec \"$2\" app-server daemon restart",
            "restart",
        ])
        .arg(&project)
        .arg(&daemon.codex)
        .current_dir(&project)
        .env("CODEX_HOME", daemon.home.path())
        .output()?;
    ensure!(
        restarted.status.success(),
        "{}",
        String::from_utf8_lossy(&restarted.stderr)
    );
    let restarted: Value = serde_json::from_slice(&restarted.stdout)?;
    assert_eq!(restarted["status"], "restarted");
    assert_ne!(daemon.pid("app-server.pid")?, old_pid);
    assert_ne!(daemon.pid("app-server-updater.pid")?, old_updater_pid);
    assert_eq!(daemon.lifecycle("version")?["status"], "running");
    let updater_socket = daemon
        .home
        .path()
        .join("app-server-daemon-vl/app-server-updater.sock");
    let deadline = Instant::now() + Duration::from_secs(30);
    while std::os::unix::net::UnixStream::connect(&updater_socket).is_err() {
        ensure!(
            Instant::now() < deadline,
            "replacement updater did not become ready"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[test]
fn bootstrap_updater_is_ready_in_deep_codex_home() -> Result<()> {
    let mut daemon = TestDaemon::new()?;
    let deep_home = tempfile::Builder::new()
        .prefix(&"deep-codex-home-".repeat(8))
        .tempdir_in("/tmp")?;
    for name in ["packages", "app-server-daemon-vl"] {
        std::fs::rename(daemon.home.path().join(name), deep_home.path().join(name))?;
    }
    daemon.home = deep_home;
    let state = daemon.home.path().join("app-server-daemon-vl");
    std::fs::write(
        state.join("settings.json"),
        br#"{"shutdownGraceSeconds":0}"#,
    )?;
    let socket = state.join("app-server-updater.sock");
    let result = daemon.lifecycle("bootstrap")?;
    assert_eq!(result["autoUpdateEnabled"], true);
    assert_eq!(result["status"], "bootstrapped");
    signal(daemon.pid("app-server-updater.pid")?, /*signal*/ 0)?;
    let target = socket.canonicalize()?;
    let _connection = std::os::unix::net::UnixStream::connect(target)?;
    assert_eq!(daemon.lifecycle("version")?["status"], "running");
    Ok(())
}

#[test]
fn bootstrap_reports_updater_bind_failure() -> Result<()> {
    let daemon = TestDaemon::new()?;
    let state = daemon.home.path().join("app-server-daemon-vl");
    std::fs::write(
        state.join("settings.json"),
        br#"{"shutdownGraceSeconds":0}"#,
    )?;
    let socket = state.join("app-server-updater.sock");
    std::fs::write(&socket, b"not a socket")?;
    let result = daemon
        .command()
        .args(["app-server", "daemon", "bootstrap"])
        .output()?;
    assert!(!result.status.success());
    assert_eq!(std::fs::read(socket)?, b"not a socket");
    assert_eq!(daemon.lifecycle("version")?["status"], "running");
    Ok(())
}

#[test]
fn explicit_restart_applies_feature_overrides_and_preserves_saved_settings() -> Result<()> {
    let daemon = TestDaemon::new()?;
    let settings_path = daemon
        .home
        .path()
        .join("app-server-daemon-vl/settings.json");
    std::fs::write(
        &settings_path,
        serde_json::to_vec(&serde_json::json!({
            "shutdownGraceSeconds": 0,
            "updater": { "autoUpdateEnabled": false },
            "featureOverrides": {
                "api_key_model_discovery": false,
                "auth_elicitation": true,
                "connectors": true,
                "apps": true
            }
        }))?,
    )?;
    let mut previous_pid = None;
    // Start from a stopped daemon, change running settings, and restart again
    // with omitted flags. Every explicit restart replaces the process.
    for (args, discovery) in [
        (
            vec![
                "--enable",
                "api_key_model_discovery",
                "--disable",
                "connectors",
                "-c",
                "features.network_proxy.enabled=false",
            ],
            true,
        ),
        (vec!["--disable", "api_key_model_discovery"], false),
        (vec!["-c", "features.api_key_model_discovery=true"], true),
        (vec![], true),
    ] {
        let output = daemon
            .command()
            .args(["app-server", "daemon", "restart"])
            .args(args)
            .output()?;
        ensure!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output: Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(output["status"], "restarted");
        let pid = output["pid"].as_u64().context("restarted daemon PID")?;
        assert_ne!(Some(pid), previous_pid);
        previous_pid = Some(pid);
        let settings: Value = serde_json::from_slice(&std::fs::read(&settings_path)?)?;
        assert_eq!(
            settings["featureOverrides"],
            serde_json::json!({
                "api_key_model_discovery": discovery,
                "auth_elicitation": true,
                "apps": false,
                "network_proxy": false
            })
        );
        assert_eq!(settings["updater"]["autoUpdateEnabled"], false);
    }
    // Non-toggle config keys must fail before stopping the daemon or saving settings.
    let saved_settings = std::fs::read(&settings_path)?;
    let output = daemon
        .command()
        .args([
            "app-server",
            "daemon",
            "restart",
            "--enable",
            "tool_registry",
        ])
        .output()?;
    assert!(!output.status.success());
    insta::assert_snapshot!(
        String::from_utf8(output.stderr)?,
        @"Error: daemon restart requires a boolean feature toggle: tool_registry"
    );
    assert_eq!(std::fs::read(&settings_path)?, saved_settings);
    assert_eq!(Some(u64::from(daemon.pid("app-server.pid")?)), previous_pid);
    assert_eq!(daemon.lifecycle("version")?["status"], "running");
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn managed_identity_survives_locale_and_timezone_changes() -> Result<()> {
    let daemon = TestDaemon::new()?;
    let mut original = Vec::new();
    let mut updater_pid = 0;
    let pid_file = daemon
        .home
        .path()
        .join("app-server-daemon-vl/app-server.pid");
    for (action, locale, timezone, expected_status) in [
        ("start", "C", "UTC0", "started"),
        ("version", "en_AU.UTF-8", "PST8PDT", "running"),
        ("restart", "en_AU.UTF-8", "PST8PDT", "restarted"),
        ("stop", "C", "UTC0", "stopped"),
    ] {
        let output = daemon
            .command()
            .args(["app-server", "daemon", action])
            .env("LC_ALL", locale)
            .env("TZ", timezone)
            .output()?;
        ensure!(
            output.status.success(),
            "daemon {action} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output: Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(output["status"], expected_status);
        if action == "start" {
            updater_pid = daemon.pid("app-server-updater.pid")?;
            original = std::fs::read(&pid_file)?;
            let record: Value = serde_json::from_slice(&original)?;
            assert!(record["processIdentity"].is_object());
            // Older clients still receive the original, unnormalized ps value.
            let legacy = Command::new("ps")
                .args([
                    "-p",
                    &daemon.pid("app-server.pid")?.to_string(),
                    "-o",
                    "lstart=",
                ])
                .env("LC_ALL", locale)
                .env("TZ", timezone)
                .output()?;
            ensure!(legacy.status.success(), "failed to read legacy start time");
            assert_eq!(
                record["processStartTime"],
                String::from_utf8(legacy.stdout)?.trim()
            );
        } else if action == "version" {
            assert_eq!(output["backend"], "pid");
            assert_eq!(daemon.pid("app-server-updater.pid")?, updater_pid);
            assert_eq!(std::fs::read(&pid_file)?, original);

            // Finish startup promotion before deliberately restoring a legacy record.
            let updater_socket = daemon
                .home
                .path()
                .join("app-server-daemon-vl/app-server-updater.sock");
            let deadline = Instant::now() + Duration::from_secs(30);
            while !updater_socket.exists() {
                ensure!(Instant::now() < deadline, "updater did not become ready");
                std::thread::sleep(Duration::from_millis(50));
            }
            let mut legacy: Value = serde_json::from_slice(&original)?;
            legacy
                .as_object_mut()
                .context("PID record object")?
                .remove("processIdentity");
            let legacy_bytes = serde_json::to_vec(&legacy)?;
            std::fs::write(&pid_file, &legacy_bytes)?;
            let result = daemon
                .command()
                .args(["app-server", "daemon", "version"])
                .env("LC_ALL", locale)
                .env("TZ", timezone)
                .output();
            let preserved = std::fs::read(&pid_file);
            // Restore the native identity before assertions so Drop can stop the daemon.
            std::fs::write(&pid_file, &original)?;
            let result = result?;
            assert!(!result.status.success());
            assert_eq!(preserved?, legacy_bytes);
            let stderr =
                String::from_utf8(result.stderr)?.replace(&legacy["pid"].to_string(), "[PID]");
            insta::assert_snapshot!(stderr, @"Error: cannot verify pid-managed process [PID]: legacy start time changed; PID record retained. Retry with the locale and timezone used to start the daemon. If the system clock changed, stop the original process before restarting the daemon");
        }
        if action == "restart" {
            assert_eq!(daemon.pid("app-server-updater.pid")?, updater_pid);
        }
    }
    assert!(!pid_file.exists());
    Ok(())
}

#[test]
fn package_ownership_check_does_not_start_an_updater() -> Result<()> {
    let daemon = TestDaemon::new()?;
    let output = daemon
        .command()
        .args([
            "app-server",
            "daemon",
            "pid-update-loop",
            "--check-package-ownership",
        ])
        .output()?;
    ensure!(
        output.status.success(),
        "ownership check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let state = daemon.home.path().join("app-server-daemon-vl");
    assert!(!state.join("app-server-updater.pid").exists());
    assert!(!state.join("daemon-updater.pid").exists());
    Ok(())
}

#[test]
fn managed_starts_ensure_one_updater_and_recover_a_missing_one() -> Result<()> {
    let daemon = TestDaemon::new()?;
    assert_eq!(daemon.lifecycle("start")?["status"], "started");
    let backend_pid = daemon.pid("app-server.pid")?;
    let updater_pid = daemon.pid("app-server-updater.pid")?;
    assert_ne!(backend_pid, updater_pid);

    assert_eq!(daemon.lifecycle("start")?["status"], "alreadyRunning");
    assert_eq!(daemon.pid("app-server.pid")?, backend_pid);
    assert_eq!(daemon.pid("app-server-updater.pid")?, updater_pid);

    signal(updater_pid, libc::SIGTERM)?;
    wait_for_exit(updater_pid)?;
    // A replacement updater also upgrades still-verifiable records left by an old CLI.
    let server_record_path = daemon
        .home
        .path()
        .join("app-server-daemon-vl/app-server.pid");
    let mut legacy: Value = serde_json::from_slice(&std::fs::read(&server_record_path)?)?;
    let native = legacy.as_object_mut().unwrap().remove("processIdentity");
    std::fs::write(&server_record_path, serde_json::to_vec(&legacy)?)?;
    assert_eq!(daemon.lifecycle("start")?["status"], "alreadyRunning");
    assert_eq!(daemon.pid("app-server.pid")?, backend_pid);
    let replacement_pid = daemon.pid("app-server-updater.pid")?;
    assert_ne!(replacement_pid, updater_pid);
    if let Some(native) = native {
        legacy["processIdentity"] = native;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let record: Value = serde_json::from_slice(&std::fs::read(&server_record_path)?)?;
            if record == legacy {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "legacy record was not upgraded: {record}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    assert_eq!(daemon.lifecycle("restart")?["status"], "restarted");
    assert_ne!(daemon.pid("app-server.pid")?, backend_pid);
    assert_eq!(daemon.pid("app-server-updater.pid")?, replacement_pid);
    daemon.lifecycle("stop")?;
    assert!(
        !daemon
            .home
            .path()
            .join("app-server-daemon-vl/app-server.pid")
            .exists()
    );
    assert_eq!(daemon.lifecycle("start")?["status"], "started");
    assert!(
        !daemon
            .home
            .path()
            .join("packages/app-server-daemon-vl")
            .exists()
    );
    Ok(())
}

#[test]
fn managed_start_succeeds_when_updater_record_is_invalid() -> Result<()> {
    let daemon = TestDaemon::new()?;
    let state_dir = daemon.home.path().join("app-server-daemon-vl");
    std::fs::create_dir_all(&state_dir)?;
    std::fs::write(state_dir.join("app-server-updater.pid"), "not a PID record")?;

    assert_eq!(daemon.lifecycle("start")?["status"], "started");
    let server_pid = daemon.pid("app-server.pid")?;
    assert_eq!(daemon.lifecycle("start")?["status"], "alreadyRunning");
    assert_eq!(daemon.pid("app-server.pid")?, server_pid);
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn wall_clock_shift_keeps_server_and_updater_managed() -> Result<()> {
    let daemon = TestDaemon::new()?;
    assert_eq!(daemon.lifecycle("start")?["status"], "started");
    let server_pid = daemon.pid("app-server.pid")?;
    let updater_pid = daemon.pid("app-server-updater.pid")?;
    for name in ["app-server.pid", "app-server-updater.pid"] {
        let path = daemon.home.path().join("app-server-daemon-vl").join(name);
        let mut record: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        record["processStartTime"] = "historical wall-clock start time".into();
        let replacement = path.with_extension("replacement");
        std::fs::write(&replacement, serde_json::to_vec(&record)?)?;
        std::fs::rename(replacement, path)?;
    }

    let version = daemon.lifecycle("version")?;
    assert_eq!(version["status"], "running");
    assert_eq!(version["backend"], "pid");
    assert_eq!(daemon.lifecycle("start")?["status"], "alreadyRunning");
    assert_eq!(daemon.pid("app-server.pid")?, server_pid);
    assert_eq!(daemon.pid("app-server-updater.pid")?, updater_pid);
    assert_eq!(daemon.lifecycle("restart")?["status"], "restarted");
    wait_for_exit(server_pid)?;
    assert_ne!(daemon.pid("app-server.pid")?, server_pid);
    assert_eq!(daemon.pid("app-server-updater.pid")?, updater_pid);
    Ok(())
}

#[test]
fn managed_start_keeps_updater_on_marker_mismatch_but_stops_it_for_pin() -> Result<()> {
    let daemon = TestDaemon::new()?;
    assert_eq!(daemon.lifecycle("start")?["status"], "started");
    let updater_pid = daemon.pid("app-server-updater.pid")?;
    let marker = daemon
        .home
        .path()
        .join("packages/standalone/auto-update-version");
    std::fs::write(&marker, "0.1.0-other-target")?;
    assert_eq!(daemon.lifecycle("start")?["status"], "alreadyRunning");
    assert_eq!(daemon.pid("app-server-updater.pid")?, updater_pid);
    std::thread::sleep(Duration::from_millis(250));
    let updater_state = Command::new("/bin/ps")
        .args(["-p", &updater_pid.to_string(), "-o", "stat="])
        .output()?;
    ensure!(
        updater_state.status.success()
            && !String::from_utf8_lossy(&updater_state.stdout)
                .trim()
                .starts_with('Z'),
        "updater exited during the latest marker transition"
    );

    std::fs::remove_file(marker)?;

    assert_eq!(daemon.lifecycle("start")?["status"], "alreadyRunning");
    wait_for_exit(updater_pid)?;
    assert!(
        !daemon
            .home
            .path()
            .join("app-server-daemon-vl/app-server-updater.pid")
            .exists()
    );
    Ok(())
}

#[test]
fn restart_applies_saved_updater_preference() -> Result<()> {
    let daemon = TestDaemon::new()?;
    assert_eq!(daemon.lifecycle("start")?["status"], "started");
    let updater_pid = daemon.pid("app-server-updater.pid")?;
    let settings = daemon
        .home
        .path()
        .join("app-server-daemon-vl/settings.json");
    std::fs::write(
        &settings,
        serde_json::to_vec(&serde_json::json!({
            "updater": {"autoUpdateEnabled": false, "updateIntervalMinutes": 2},
        }))?,
    )?;
    assert_eq!(daemon.lifecycle("restart")?["status"], "restarted");
    wait_for_exit(updater_pid)?;
    assert!(daemon.pid("app-server-updater.pid").is_err());
    assert_eq!(daemon.lifecycle("bootstrap")?["autoUpdateEnabled"], false);
    assert!(daemon.pid("app-server-updater.pid").is_err());

    std::fs::write(
        &settings,
        serde_json::to_vec(&serde_json::json!({
            "updater": {"autoUpdateEnabled": true, "updateIntervalMinutes": 2},
        }))?,
    )?;
    assert_eq!(daemon.lifecycle("restart")?["status"], "restarted");
    assert_ne!(daemon.pid("app-server-updater.pid")?, updater_pid);

    std::fs::write(&settings, "{malformed")?;
    assert_eq!(daemon.lifecycle("stop")?["status"], "stopped");
    Ok(())
}

#[test]
fn unmanaged_app_server_does_not_launch_updater() -> Result<()> {
    let mut daemon = TestDaemon::new()?;
    daemon.unmanaged = Some(
        daemon
            .command()
            .args(["app-server", "--listen", "unix://"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !daemon
        .command()
        .args(["app-server", "daemon", "version"])
        .output()?
        .status
        .success()
    {
        ensure!(Instant::now() < deadline, "app server did not become ready");
        std::thread::sleep(Duration::from_millis(50));
    }

    let output = daemon.lifecycle("start")?;
    assert_eq!(output["status"], "alreadyRunning");
    assert_eq!(output["backend"], Value::Null);
    assert_eq!(daemon.lifecycle("update")?["status"], "unsupported");
    assert!(
        !daemon
            .home
            .path()
            .join("app-server-daemon-vl/app-server-updater.pid")
            .exists()
    );
    Ok(())
}

#[test]
fn manual_update_rejects_an_unowned_installation() -> Result<()> {
    let daemon = TestDaemon::new()?;
    std::fs::remove_file(
        daemon
            .home
            .path()
            .join("packages/standalone/current/bin/codex"),
    )?;

    assert_eq!(daemon.lifecycle("update")?["status"], "unsupported");
    assert!(daemon.pid("app-server.pid").is_err());
    assert!(daemon.pid("app-server-updater.pid").is_err());
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InitialDaemon {
    Missing,
    Legacy,
}

fn packaged_daemon_launch(action: &str, initial: InitialDaemon) -> Result<()> {
    let mut daemon = TestDaemon::new()?;
    let standalone = daemon.home.path().join("packages/standalone");
    let package = if action == "start" && initial == InitialDaemon::Missing {
        standalone.join("releases/caller")
    } else {
        daemon.home.path().join("cli-package")
    };
    for directory in ["bin", "codex-path", "codex-resources"] {
        std::fs::create_dir_all(package.join(directory))?;
    }
    copy_executable(&daemon.codex, &package.join("bin/codex"))?;
    daemon.codex = package.join("bin/codex");
    for helper in [
        "bin/codex-code-mode-host",
        "codex-path/rg",
        "codex-resources/bwrap",
    ] {
        codex_utils_cargo_bin::write_executable(&package.join(helper), "runtime fixture")?;
    }
    let target = format!(
        "{}-{}",
        std::env::consts::ARCH,
        if cfg!(target_os = "macos") {
            "apple-darwin"
        } else if cfg!(target_env = "gnu") {
            "unknown-linux-gnu"
        } else {
            "unknown-linux-musl"
        }
    );
    std::fs::write(
        package.join("codex-package.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"), "target": target, "entrypoint": "bin/codex"
        }))?,
    )?;
    if action == "start" && initial == InitialDaemon::Missing {
        std::fs::remove_file(standalone.join("current"))?;
        std::os::unix::fs::symlink(&package, standalone.join("current"))?;
    }
    let cli_selection = standalone.join("current").canonicalize()?;
    let state = daemon.home.path().join("app-server-daemon-vl");
    if initial == InitialDaemon::Missing {
        std::fs::remove_file(state.join("app-server.stderr.log"))?;
    }
    std::fs::write(
        state.join("settings.json"),
        br#"{"shutdownGraceSeconds":0}"#,
    )?;
    let cli_before = daemon.codex.canonicalize()?;
    let result = daemon
        .command()
        .args(["app-server", "daemon", action])
        .output()
        .context("failed to launch packaged daemon fixture")?;
    ensure!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stderr).contains("Installing daemon from CLI version"),
        initial == InitialDaemon::Missing
    );
    let output: Value = serde_json::from_slice(&result.stdout)?;
    let dedicated = daemon
        .home
        .path()
        .canonicalize()?
        .join("packages/app-server-daemon-vl");
    let (initial_root, initial_pid_file) = match initial {
        InitialDaemon::Missing => (&dedicated, "daemon.pid"),
        InitialDaemon::Legacy => (&standalone, "app-server.pid"),
    };
    assert_eq!(
        output["managedCodexPath"],
        initial_root
            .canonicalize()?
            .join("current/bin/codex")
            .to_str()
            .context("managed path is not UTF-8")?
    );
    assert_eq!(daemon.codex.canonicalize()?, cli_before);
    assert_eq!(standalone.join("current").canonicalize()?, cli_selection);
    assert!(state.join(initial_pid_file).exists());
    let legacy_updater = match initial {
        InitialDaemon::Missing => {
            assert!(!state.join("app-server-updater.pid").exists());
            None
        }
        InitialDaemon::Legacy => Some(daemon.pid("app-server-updater.pid")?),
    };
    if action == "start" {
        let initial_current = initial_root.join("current");
        let current = dedicated.join("current");
        let original = initial_current.canonicalize()?;
        let original_pid = daemon.pid(initial_pid_file)?;
        let settings_before = std::fs::read(state.join("settings.json"))?;
        let refused = daemon
            .command()
            .args(["app-server", "daemon", "update", "--from-cli"])
            .output()?;
        assert!(!refused.status.success());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("rerun with --yes"));
        assert_eq!(daemon.pid(initial_pid_file)?, original_pid);
        assert_eq!(initial_current.canonicalize()?, original);
        if initial == InitialDaemon::Legacy {
            assert!(!current.exists());
        }

        std::fs::remove_file(package.join("bin/codex-code-mode-host"))?;
        let invalid = daemon
            .command()
            .args(["app-server", "daemon", "update", "--from-cli", "--yes"])
            .output()?;
        assert!(!invalid.status.success());
        assert_eq!(daemon.pid(initial_pid_file)?, original_pid);
        assert_eq!(initial_current.canonicalize()?, original);
        if initial == InitialDaemon::Legacy {
            assert!(!current.exists());
        }
        codex_utils_cargo_bin::write_executable(
            &package.join("bin/codex-code-mode-host"),
            "replacement helper",
        )?;
        let replaced = daemon
            .command()
            .args(["app-server", "daemon", "update", "--from-cli", "--yes"])
            .output()?;
        ensure!(
            replaced.status.success(),
            "{}",
            String::from_utf8_lossy(&replaced.stderr)
        );
        let output: Value = serde_json::from_slice(&replaced.stdout)?;
        assert_eq!(output["status"], "updated");
        assert_ne!(daemon.pid("daemon.pid")?, original_pid);
        wait_for_exit(original_pid)?;
        if let Some(pid) = legacy_updater {
            wait_for_exit(pid)?;
        }
        assert!(!state.join("app-server.pid").exists());
        assert!(!state.join("app-server-updater.pid").exists());
        assert_eq!(
            output["managedCodexPath"],
            current
                .join("bin/codex")
                .to_str()
                .context("managed path is not UTF-8")?
        );
        assert_ne!(current.canonicalize()?, original);
        assert!(!dedicated.join("auto-update-version").exists());
        assert_eq!(std::fs::read(state.join("settings.json"))?, settings_before);
        assert_eq!(
            std::fs::read(current.join("bin/codex-code-mode-host"))?,
            b"replacement helper"
        );
        if initial == InitialDaemon::Missing {
            assert_eq!(
                std::fs::read(original.join("bin/codex-code-mode-host"))?,
                b"runtime fixture"
            );
        }
        assert_eq!(daemon.lifecycle("start")?["status"], "alreadyRunning");
        assert_eq!(daemon.lifecycle("stop")?["status"], "stopped");
        assert_eq!(standalone.join("current").canonicalize()?, cli_selection);
    }
    if action == "bootstrap" {
        assert_eq!(output["autoUpdateEnabled"], false);
    }
    Ok(())
}

#[test]
fn packaged_daemon_start_and_explicit_replacement() -> Result<()> {
    packaged_daemon_launch("start", InitialDaemon::Missing)
}

#[test]
fn packaged_daemon_restart_seeds_local_package() -> Result<()> {
    packaged_daemon_launch("restart", InitialDaemon::Missing)
}

#[test]
fn packaged_daemon_bootstrap_seeds_local_package() -> Result<()> {
    packaged_daemon_launch("bootstrap", InitialDaemon::Missing)
}

#[test]
fn packaged_daemon_explicit_replacement_migrates_running_legacy() -> Result<()> {
    packaged_daemon_launch("start", InitialDaemon::Legacy)
}
