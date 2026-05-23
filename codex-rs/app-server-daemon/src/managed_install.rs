use std::env;
use std::fs as std_fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
#[cfg(unix)]
use anyhow::anyhow;
use codex_install_context::InstallContext;
use codex_install_context::InstallMethod;
#[cfg(unix)]
use sha2::Digest;
#[cfg(unix)]
use sha2::Sha256;
#[cfg(unix)]
use tokio::fs;
#[cfg(unix)]
use tokio::process::Command;

pub(crate) fn managed_codex_bin(codex_home: &Path) -> PathBuf {
    codex_home
        .join("packages")
        .join("standalone")
        .join("current")
        .join(managed_codex_file_name())
}

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
        InstallMethod::Npm | InstallMethod::Bun | InstallMethod::Other => {
            managed_package_current_exe()
        }
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

#[cfg(unix)]
pub(crate) async fn resolved_managed_codex_bin(codex_bin: &Path) -> Result<PathBuf> {
    fs::canonicalize(codex_bin).await.with_context(|| {
        format!(
            "failed to resolve managed Codex binary {}",
            codex_bin.display()
        )
    })
}

#[cfg(unix)]
pub(crate) async fn managed_codex_version(codex_bin: &Path) -> Result<String> {
    let output = Command::new(codex_bin)
        .arg("--version")
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

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutableIdentity {
    digest: [u8; 32],
}

#[cfg(unix)]
pub(crate) async fn executable_identity(executable: &Path) -> Result<ExecutableIdentity> {
    let bytes = fs::read(executable)
        .await
        .with_context(|| format!("failed to read executable {}", executable.display()))?;
    Ok(executable_identity_from_bytes(&bytes))
}

#[cfg(unix)]
pub(crate) fn executable_identity_from_bytes(bytes: &[u8]) -> ExecutableIdentity {
    ExecutableIdentity {
        digest: Sha256::digest(bytes).into(),
    }
}

fn managed_codex_file_name() -> &'static str {
    if cfg!(windows) { "codex.exe" } else { "codex" }
}

#[cfg(unix)]
fn parse_codex_version(output: &str) -> Result<String> {
    let version = output
        .split_whitespace()
        .nth(1)
        .filter(|version| !version.is_empty())
        .ok_or_else(|| anyhow!("managed Codex version output was malformed"))?;
    Ok(version.to_string())
}

#[cfg(all(test, unix))]
#[path = "managed_install_tests.rs"]
mod tests;
