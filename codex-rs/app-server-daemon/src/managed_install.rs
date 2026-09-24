//! Resolves both package and legacy standalone layouts and compares installed executables.

use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use serde::Deserialize;
use serde::Serialize;
use tokio::fs;
use tokio::process::Command;
use tokio::time::timeout;

use std::env;
use std::fs as std_fs;

use codex_install_context::InstallContext;
use codex_install_context::InstallMethod;

/// New daemons own their packages, regardless of how the calling CLI was installed.
/// Preserve legacy launch state, including logs left after a daemon is stopped;
/// settings, installer selections, and lock files alone do not prove a prior launch.
pub(crate) fn package_root(codex_home: &Path) -> PathBuf {
    let dedicated = codex_home.join("packages/app-server-daemon");
    if !matches!(dedicated.join("current").symlink_metadata(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    {
        return dedicated;
    }
    let state = codex_home.join("app-server-daemon");
    for (package, artifacts) in [
        (
            "app-server-daemon",
            [
                crate::DAEMON_PID_FILE_NAME,
                "daemon.stderr.log",
                crate::DAEMON_UPDATE_PID_FILE_NAME,
                "daemon-updater.stderr.log",
            ],
        ),
        (
            "standalone",
            [
                crate::LEGACY_PID_FILE_NAME,
                "app-server.stderr.log",
                crate::LEGACY_UPDATE_PID_FILE_NAME,
                "app-server-updater.stderr.log",
            ],
        ),
    ] {
        if artifacts.iter().any(|name| {
            !matches!(state.join(name).symlink_metadata(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        }) {
            return codex_home.join("packages").join(package);
        }
    }
    dedicated
}

/// Resolve both packaged and legacy binaries without requiring a valid install.
pub(crate) fn managed_codex_bin(codex_home: &Path) -> PathBuf {
    let root = package_root(codex_home);
    let current = root.join("current");
    let packaged = current.join("bin").join(managed_codex_file_name());
    let legacy = current.join(managed_codex_file_name());
    if packaged.is_file()
        || !legacy.is_file() && (cfg!(windows) || root.ends_with("app-server-daemon"))
    {
        packaged
    } else {
        legacy
    }
}

/// Only latest-channel stable releases may run the public latest-version updater.
pub(crate) fn is_stable_standalone_release(codex_home: &Path, codex_bin: &Path) -> bool {
    let standalone = package_root(codex_home);
    let Ok(releases) = std::fs::canonicalize(standalone.join("releases")) else {
        return false;
    };
    let Ok(release) = std::fs::canonicalize(standalone.join("current")) else {
        return false;
    };
    if release.parent() != Some(releases.as_path()) {
        return false;
    }
    let Some(release_name) = release.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    // GNU packages can seed the new directory; retain legacy updater eligibility.
    if standalone.ends_with("standalone") && release_name.ends_with("-gnu") {
        return false;
    }
    let targets = [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-musl",
        "x86_64-unknown-linux-musl",
        "aarch64-pc-windows-msvc",
        "x86_64-pc-windows-msvc",
    ];
    let Some(version) = targets
        .iter()
        .find_map(|target| release_name.strip_suffix(&format!("-{target}")))
    else {
        return false;
    };
    let components: Vec<_> = version.split('.').collect();
    components.len() == 3
        && components.iter().all(|component| {
            !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
        })
        && std::fs::read_to_string(standalone.join("auto-update-version"))
            .is_ok_and(|selected| selected == release_name)
        && std::fs::canonicalize(codex_bin).is_ok_and(|bin| bin.starts_with(&release))
}

/// Older managed binaries can serve app-server requests without owning an updater.
pub(crate) async fn supports_daemon_update_loop(codex_bin: &Path) -> bool {
    supports_daemon_command(codex_bin, &["pid-update-loop", "--help"]).await
}

/// Probe an internal daemon command without running a long-lived process.
pub(crate) async fn supports_daemon_command(codex_bin: &Path, args: &[&str]) -> bool {
    let mut command = Command::new(codex_bin);
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    timeout(
        Duration::from_secs(5),
        command
            .args(["app-server", "daemon"])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await
    .is_ok_and(|result| result.is_ok_and(|status| status.success()))
}

/// Returns the packaged executable when present, otherwise an existing legacy executable.
/// If neither exists, returns the expected packaged path on Windows and preserves the
/// historical legacy fallback on Unix. This is path selection, not existence validation:
/// launch operations reject a missing executable separately, while commands such as
/// stop can still run after the managed install has been removed.
pub(crate) fn resolve_managed_codex_bin_for_install_context(
    install_context: &InstallContext,
    codex_home: &Path,
) -> Result<PathBuf> {
    // codex-vl: post-merge `InstallContext` is a struct; route through
    // its `method` discriminant. Patch 16-style adapter from Termux merge.
    //
    // `InstallMethod::Other` is the catch-all when none of the known
    // shims (npm wrapper, bun wrapper, standalone release layout,
    // homebrew prefix) is detected. In practice this fires when the
    // user runs the fork binary directly — e.g. through a personal
    // symlink to the npm vendor path, or via a packaged-rebuild path
    // that does not match the upstream standalone install layout.
    // Falling back to `managed_codex_bin(codex_home)` would point the
    // daemon at the standalone install path, which the codex-vl fork
    // never ships, and the fork-protection guard in
    // `ensure_managed_codex_bin` would then surface a misleading
    // "managed standalone install not found" error.
    //
    // The safer behaviour is to resolve the binary through the same
    // `current_exe` / `CODEX_SELF_EXE` path used for Npm/Bun: the
    // process that just launched the daemon is, by definition, the
    // fork binary the user is running, so re-launching the daemon
    // from it stays on the fork. Brew is left on the standalone
    // path because a homebrew cask explicitly ships its own
    // standalone-compatible layout.
    match install_context.method {
        InstallMethod::Npm
        | InstallMethod::Bun
        | InstallMethod::VitePlus
        | InstallMethod::Pnpm
        | InstallMethod::Other => managed_package_current_exe(),
        InstallMethod::Standalone { .. } | InstallMethod::Brew => Ok(managed_codex_bin(codex_home)),
    }
}

fn managed_package_current_exe() -> Result<PathBuf> {
    if let Some(self_exe) = env::var_os("CODEX_SELF_EXE") {
        let self_exe = PathBuf::from(self_exe);
        if self_exe.is_file() {
            return std_fs::canonicalize(&self_exe).with_context(|| {
                format!("failed to resolve CODEX_SELF_EXE {}", self_exe.display())
            });
        }
    }

    let current_exe = env::current_exe().context("failed to resolve current executable")?;
    std_fs::canonicalize(&current_exe).with_context(|| {
        format!(
            "failed to resolve current executable {}",
            current_exe.display()
        )
    })
}

pub(crate) async fn resolved_managed_codex_bin(codex_bin: &Path) -> Result<PathBuf> {
    fs::canonicalize(codex_bin).await.with_context(|| {
        format!(
            "failed to resolve managed Codex binary {}",
            codex_bin.display()
        )
    })
}

pub(crate) async fn managed_codex_version(codex_bin: &Path) -> Result<String> {
    let mut command = Command::new(codex_bin);
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    let output = command
        .arg("--version")
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| {
            format!(
                "failed to invoke managed Codex binary {}",
                codex_bin.display()
            )
        })?;
    if !output.status.success() {
        return Err(anyhow!(
            "managed Codex binary {} exited with status {}",
            codex_bin.display(),
            output.status
        ));
    }

    let stdout = String::from_utf8(output.stdout).with_context(|| {
        format!(
            "managed Codex version was not utf-8: {}",
            codex_bin.display()
        )
    })?;
    parse_codex_version(&stdout)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExecutableIdentity {
    digest: [u8; 32],
}

pub(crate) async fn executable_identity(executable: &Path) -> Result<ExecutableIdentity> {
    let bytes = fs::read(executable)
        .await
        .with_context(|| format!("failed to read executable {}", executable.display()))?;
    Ok(executable_identity_from_bytes(&bytes))
}

pub(crate) fn executable_identity_from_bytes(bytes: &[u8]) -> ExecutableIdentity {
    ExecutableIdentity {
        digest: *blake3::hash(bytes).as_bytes(),
    }
}

fn managed_codex_file_name() -> &'static str {
    if cfg!(windows) { "codex.exe" } else { "codex" }
}

fn parse_codex_version(output: &str) -> Result<String> {
    let version = output
        .split_whitespace()
        .nth(1)
        .filter(|version| !version.is_empty())
        .ok_or_else(|| anyhow!("managed Codex version output was malformed"))?;
    Ok(version.to_string())
}

#[cfg(test)]
#[path = "managed_install_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "managed_install_path_tests.rs"]
mod path_tests;
