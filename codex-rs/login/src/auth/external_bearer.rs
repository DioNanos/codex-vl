use super::manager::CodexAuth;
use super::manager::ExternalAuth;
use super::manager::ExternalAuthFuture;
use super::manager::ExternalAuthRefreshContext;
use codex_protocol::config_types::ModelProviderAuthInfo;
use std::fmt;
use std::io;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::process::Command;
use tokio::sync::Mutex;

#[derive(Clone)]
pub(crate) struct BearerTokenRefresher {
    state: Arc<ExternalBearerAuthState>,
}

impl BearerTokenRefresher {
    pub(crate) fn new(config: ModelProviderAuthInfo) -> Self {
        Self {
            state: Arc::new(ExternalBearerAuthState::new(config)),
        }
    }

    #[expect(
        clippy::await_holding_invalid_type,
        reason = "external bearer cache misses intentionally hold cached_token across the provider command to avoid duplicate refreshes"
    )]
    async fn resolve(&self) -> io::Result<CodexAuth> {
        let access_token = {
            let mut cached = self.state.cached_token.lock().await;
            if let Some(cached_token) = cached.as_ref() {
                let should_use_cached_token = match self.state.config.refresh_interval() {
                    Some(refresh_interval) => cached_token.fetched_at.elapsed() < refresh_interval,
                    None => true,
                };
                if should_use_cached_token {
                    return Ok(CodexAuth::from_api_key(cached_token.access_token.as_str()));
                }
            }

            let access_token = run_provider_auth_command(&self.state.config).await?;
            *cached = Some(CachedExternalBearerToken {
                access_token: access_token.clone(),
                fetched_at: Instant::now(),
            });
            access_token
        };
        Ok(CodexAuth::from_api_key(access_token.as_str()))
    }

    async fn refresh(&self, _context: ExternalAuthRefreshContext) -> io::Result<CodexAuth> {
        let access_token = run_provider_auth_command(&self.state.config).await?;
        let mut cached = self.state.cached_token.lock().await;
        *cached = Some(CachedExternalBearerToken {
            access_token: access_token.clone(),
            fetched_at: Instant::now(),
        });
        Ok(CodexAuth::from_api_key(access_token.as_str()))
    }
}

impl ExternalAuth for BearerTokenRefresher {
    fn resolve(&self) -> ExternalAuthFuture<'_, CodexAuth> {
        Box::pin(BearerTokenRefresher::resolve(self))
    }

    fn refresh(&self, context: ExternalAuthRefreshContext) -> ExternalAuthFuture<'_, CodexAuth> {
        Box::pin(BearerTokenRefresher::refresh(self, context))
    }
}

impl fmt::Debug for BearerTokenRefresher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BearerTokenRefresher")
            .finish_non_exhaustive()
    }
}

struct ExternalBearerAuthState {
    config: ModelProviderAuthInfo,
    cached_token: Mutex<Option<CachedExternalBearerToken>>,
}

impl ExternalBearerAuthState {
    fn new(config: ModelProviderAuthInfo) -> Self {
        Self {
            config,
            cached_token: Mutex::new(None),
        }
    }
}

struct CachedExternalBearerToken {
    access_token: String,
    fetched_at: Instant,
}

async fn run_provider_auth_command(config: &ModelProviderAuthInfo) -> io::Result<String> {
    let mut command = provider_auth_command_for_target(config, std::env::consts::OS)?;

    let output = tokio::time::timeout(config.timeout(), command.output())
        .await
        .map_err(|_| {
            io::Error::other(format!(
                "provider auth command `{}` timed out after {} ms",
                config.command,
                config.timeout_ms.get()
            ))
        })?
        .map_err(|err| {
            io::Error::other(format!(
                "provider auth command `{}` failed to start: {err}",
                config.command
            ))
        })?;

    if !output.status.success() {
        let status = output.status;
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stderr_suffix = if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        };
        return Err(io::Error::other(format!(
            "provider auth command `{}` exited with status {status}{stderr_suffix}",
            config.command
        )));
    }

    let stdout = String::from_utf8(output.stdout).map_err(|_| {
        io::Error::other(format!(
            "provider auth command `{}` wrote non-UTF-8 data to stdout",
            config.command
        ))
    })?;
    let access_token = stdout.trim().to_string();
    if access_token.is_empty() {
        return Err(io::Error::other(format!(
            "provider auth command `{}` produced an empty token",
            config.command
        )));
    }

    Ok(access_token)
}

/// Build the external provider-auth command and drop launcher metadata last.
///
/// The wrapper [`run_provider_auth_command`] only selects the compiling
/// platform. Stdio and the working directory stay as they were.
fn provider_auth_command_for_target(
    config: &ModelProviderAuthInfo,
    target_os: &str,
) -> io::Result<Command> {
    let program = resolve_provider_auth_program(&config.command, &config.cwd)?;
    let mut command = Command::from(codex_utils_process::background_command(&program));
    command
        .args(config.args.iter().map(Deref::deref))
        .current_dir(config.cwd.as_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    codex_utils_process::finalize_external_child_for(command.as_std_mut(), target_os);
    Ok(command)
}

fn resolve_provider_auth_program(command: &str, cwd: &Path) -> io::Result<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    if path.components().count() > 1 {
        return Ok(cwd.join(path));
    }

    Ok(PathBuf::from(command))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::num::NonZeroU64;

    fn auth_config() -> ModelProviderAuthInfo {
        ModelProviderAuthInfo {
            command: "token-fetcher".to_string(),
            args: Vec::new(),
            timeout_ms: NonZeroU64::new(5_000).expect("timeout"),
            refresh_interval_ms: 0,
            cwd: std::env::current_dir()
                .expect("current directory")
                .try_into()
                .expect("absolute cwd"),
        }
    }

    fn valued_names(command: &Command) -> Vec<OsString> {
        command
            .as_std()
            .get_envs()
            .filter(|(_, value)| value.is_some())
            .map(|(name, _)| name.to_os_string())
            .collect()
    }

    /// The shared body finalizes. Android records the six names as unset;
    /// the host records nothing. The test does not finalize afterwards.
    #[test]
    fn sentinel_provider_auth_command_drops_launcher_metadata_for_android() {
        let config = auth_config();
        let android = provider_auth_command_for_target(&config, "android").expect("command");
        for name in codex_utils_process::PRIVATE_LAUNCHER_ENV_VARS {
            let entry = android.as_std().get_envs().find(|(key, _)| *key == *name);
            assert_eq!(entry.map(|(_, value)| value), Some(None), "{name}");
        }
        assert!(!valued_names(&android).iter().any(|name| {
            name.to_str()
                .is_some_and(|name| codex_utils_process::PRIVATE_LAUNCHER_ENV_VARS.contains(&name))
        }));

        let host = provider_auth_command_for_target(&config, "linux").expect("command");
        assert!(
            valued_names(&host).is_empty(),
            "the host target must not scrub: {:?}",
            valued_names(&host)
        );
    }
}
