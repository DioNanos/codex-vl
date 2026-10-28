//! Coordinates package selection with the shell and PowerShell installers.

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use std::fs::OpenOptions;
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

pub(crate) struct InstallLock {
    _file: Option<tokio::fs::File>,
    directory: Option<std::path::PathBuf>,
    _directory_owner: Option<tempfile::TempDir>,
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        if let Some(directory) = &self.directory {
            let _ = std::fs::remove_file(directory);
        }
    }
}

/// Build the external shell probe that detects which lock utility the shell
/// installer coordinates on.
///
/// The shared body owns composition and finalization: the probe script and
/// its exit-code protocol are verbatim, and the `Command` is finalized for an
/// external child on the target platform — `/bin/sh` here is an external
/// process, not one of the fork's own re-execs, so the launcher-private
/// metadata must not reach it. Locking protocol, paths, and the choice
/// handling stay with the caller.
#[cfg(unix)]
fn install_lock_probe_command_for_target(target_os: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("/bin/sh");
    command.args(["-c", if cfg!(target_os = "macos") {
        "if command -v lockf >/dev/null 2>&1; then exit 0; elif command -v flock >/dev/null 2>&1; then exit 1; else exit 2; fi"
    } else {
        "if command -v flock >/dev/null 2>&1; then exit 1; else exit 2; fi"
    }]);
    codex_utils_process::finalize_external_child_for(command.as_std_mut(), target_os);
    command
}

pub(crate) async fn acquire_install_lock(root: &Path) -> Result<InstallLock> {
    let lock = root.join("install.lock");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    #[cfg(unix)]
    let use_lockf = {
        // Match the installer so both processes coordinate on the same lock.
        let choice = install_lock_probe_command_for_target(std::env::consts::OS)
            .status()
            .await?;
        match choice.code() {
            Some(0) => true,
            Some(1) => false,
            Some(2) => {
                let directory = root.join("install.lock.d");
                // Publish complete metadata atomically. Older shell installers
                // immediately remove an empty lock directory as stale.
                let owner = tempfile::Builder::new()
                    .prefix("install-lock-")
                    .tempdir_in(root)?;
                std::fs::write(owner.path().join("pid"), std::process::id().to_string())?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs();
                std::fs::write(owner.path().join("started_at"), now.to_string())?;
                loop {
                    match std::os::unix::fs::symlink(owner.path(), &directory) {
                        Ok(()) => {
                            return Ok(InstallLock {
                                _file: None,
                                directory: Some(directory),
                                _directory_owner: Some(owner),
                            });
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(error) => {
                            return Err(error)
                                .context("failed to acquire daemon installer directory lock");
                        }
                    }
                    let pid = std::fs::read_to_string(directory.join("pid"))
                        .ok()
                        .and_then(|pid| pid.trim().parse::<i32>().ok())
                        .filter(|pid| *pid > 0);
                    let dead = pid.is_none_or(|pid| unsafe { libc::kill(pid, 0) } != 0
                        && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH));
                    let age = std::fs::metadata(&directory)
                        .and_then(|metadata| metadata.modified())
                        .ok()
                        .and_then(|modified| modified.elapsed().ok())
                        .unwrap_or_default();
                    // Allow the creator time to publish its PID; match the installer's
                    // ten-minute stale-lock grace instead of stealing an active lock.
                    if dead && age >= Duration::from_secs(600) {
                        std::fs::remove_dir_all(&directory)?;
                        continue;
                    }
                    anyhow::ensure!(
                        tokio::time::Instant::now() < deadline,
                        "timed out waiting for daemon installer lock {}",
                        directory.display()
                    );
                    sleep(Duration::from_millis(50)).await;
                }
            }
            _ => return Err(anyhow!("failed to detect daemon installer lock tools")),
        }
    };
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    loop {
        let acquired = options.open(&lock);
        #[cfg(unix)]
        let acquired = acquired.and_then(|file| {
            {
                use std::os::fd::AsRawFd;
                let result = if use_lockf {
                    unsafe { libc::lockf(file.as_raw_fd(), libc::F_TLOCK, 0) }
                } else {
                    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }
                };
                if result != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(file)
        });
        match acquired {
            Ok(file) => {
                return Ok(InstallLock {
                    _file: Some(tokio::fs::File::from_std(file)),
                    directory: None,
                    _directory_owner: None,
                });
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || cfg!(windows)
                        && (error.raw_os_error() == Some(32)
                            || error.kind() == std::io::ErrorKind::PermissionDenied) => {}
            Err(error) => return Err(error).context("failed to acquire daemon installer lock"),
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "timed out waiting for daemon installer lock {}",
                lock.display()
            ));
        }
        sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(all(test, unix))]
mod launcher_env_tests {
    use super::*;

    /// Sentinel through the shared production body: the installer lock probe
    /// is an external shell invocation, so on the Android target every
    /// launcher-private name is recorded as an unset over inheritance and
    /// overrides, while program and probe script stay verbatim and the
    /// non-Android target is untouched. The Command is only inspected — no
    /// installer runs and no fleet lock is acquired.
    #[test]
    fn sentinel_install_lock_probe_is_finalized_for_android() {
        let command = install_lock_probe_command_for_target(codex_utils_process::ANDROID_TARGET_OS);

        assert_eq!(command.as_std().get_program(), "/bin/sh");
        let args = command
            .as_std()
            .get_args()
            .map(std::ffi::OsStr::to_os_string)
            .collect::<Vec<_>>();
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], std::ffi::OsString::from("-c"));
        let script = args[1].to_string_lossy();
        assert!(
            script.contains("command -v"),
            "probe script must stay verbatim"
        );

        for name in codex_utils_process::PRIVATE_LAUNCHER_ENV_VARS {
            let leaked = command
                .as_std()
                .get_envs()
                .any(|(env_name, value)| env_name == *name && value.is_some());
            assert!(!leaked, "{name} must not carry a value");
        }
        let unset_names = command
            .as_std()
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_os_string())
            .collect::<Vec<_>>();
        for name in codex_utils_process::PRIVATE_LAUNCHER_ENV_VARS {
            assert!(
                unset_names.contains(&std::ffi::OsString::from(*name)),
                "{name} must be recorded as an unset for the Android target"
            );
        }

        let host = install_lock_probe_command_for_target("linux");
        let host_unsets = host
            .as_std()
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .count();
        assert_eq!(host_unsets, 0, "the non-Android target must not scrub");
    }
}
