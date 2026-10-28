use std::io::IsTerminal;
use std::ops::Deref;
use std::process::Stdio;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use codex_model_provider_info::AwsAuthRefreshConfig;
use tokio::process::Command;
use tokio::sync::Semaphore;

/// Shares the configured AWS reauthentication command across Bedrock providers.
#[derive(Debug)]
pub(crate) struct AwsAuthRecovery {
    config: AwsAuthRefreshConfig,
    refresh_lock: Semaphore,
    generation: AtomicU64,
}

impl AwsAuthRecovery {
    pub(crate) fn new(config: AwsAuthRefreshConfig) -> Self {
        Self {
            config,
            refresh_lock: Semaphore::new(/*permits*/ 1),
            generation: AtomicU64::new(0),
        }
    }

    pub(crate) async fn refresh(&self) -> std::io::Result<()> {
        if self.config.command != "aws" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "AWS auth refresh command must be `aws`",
            ));
        }

        let generation = self.generation.load(Ordering::Acquire);
        let _refresh_lock = self
            .refresh_lock
            .acquire()
            .await
            .map_err(std::io::Error::other)?;
        if self.generation.load(Ordering::Acquire) != generation {
            return Ok(());
        }

        let interactive = std::io::stdin().is_terminal();
        let mut command =
            auth_refresh_command_for_target(&self.config, interactive, std::env::consts::OS);

        let status = tokio::time::timeout(self.config.timeout(), command.status())
            .await
            .map_err(|_| {
                std::io::Error::other(format!(
                    "AWS auth refresh command timed out after {} ms",
                    self.config.timeout_ms
                ))
            })?
            .map_err(|error| {
                std::io::Error::other(format!(
                    "failed to run AWS auth refresh command `{}`: {error}",
                    self.config.command
                ))
            })?;

        if status.success() {
            self.generation.fetch_add(1, Ordering::Release);
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "AWS auth refresh command exited with {status}"
            )))
        }
    }
}

/// Build the AWS reauthentication command and drop launcher metadata last.
///
/// [`AwsAuthRecovery::refresh`] only selects the compiling platform. Interactive
/// stdio is unchanged.
fn auth_refresh_command_for_target(
    config: &AwsAuthRefreshConfig,
    interactive: bool,
    target_os: &str,
) -> Command {
    let mut command = if interactive {
        Command::new(&config.command)
    } else {
        Command::from(codex_utils_process::background_command(&config.command))
    };
    command
        .args(config.args.iter().map(Deref::deref))
        .stdin(if interactive {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        // Keep interactive prompts visible without corrupting app-server's JSON-RPC stdout.
        .stdout(std::io::stderr())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    codex_utils_process::finalize_external_child_for(command.as_std_mut(), target_os);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU64;

    #[test]
    fn sentinel_auth_refresh_command_drops_launcher_metadata_for_android() {
        let config = AwsAuthRefreshConfig {
            command: "aws".to_string(),
            args: Vec::new(),
            timeout_ms: NonZeroU64::new(1_000).expect("timeout"),
        };
        let android =
            auth_refresh_command_for_target(&config, /*interactive*/ false, "android");
        for name in codex_utils_process::PRIVATE_LAUNCHER_ENV_VARS {
            let entry = android.as_std().get_envs().find(|(key, _)| *key == *name);
            assert_eq!(entry.map(|(_, value)| value), Some(None), "{name}");
        }
        let host = auth_refresh_command_for_target(&config, /*interactive*/ false, "linux");
        assert_eq!(host.as_std().get_envs().count(), 0);
    }
}
