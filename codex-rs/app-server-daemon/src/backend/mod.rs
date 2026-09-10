mod pid;

use std::path::Path;
use std::path::PathBuf;

use serde::Serialize;

use crate::DaemonLaunchGrant;

pub(crate) use pid::PidBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendKind {
    Pid,
}

#[derive(Debug, Clone)]
pub(crate) struct BackendPaths {
    pub(crate) codex_bin: PathBuf,
    pub(crate) pid_file: PathBuf,
    pub(crate) update_pid_file: PathBuf,
    pub(crate) remote_control_enabled: bool,
    pub(crate) launch_grant: Option<DaemonLaunchGrant>,
}

pub(crate) fn pid_backend(paths: BackendPaths) -> PidBackend {
    match paths.launch_grant {
        Some(grant) => PidBackend::new_with_launch_grant(
            paths.codex_bin,
            paths.pid_file,
            paths.remote_control_enabled,
            grant,
        ),
        None => PidBackend::new(
            paths.codex_bin,
            paths.pid_file,
            paths.remote_control_enabled,
        ),
    }
}

pub(crate) fn pid_update_loop_backend(paths: BackendPaths) -> PidBackend {
    PidBackend::new_update_loop(paths.codex_bin, paths.update_pid_file)
}

pub(crate) async fn append_stderr_log_tail_context(pid_file: &Path, context: &mut String) {
    match pid::read_stderr_log_tail(pid_file).await {
        Ok(Some(tail)) => tail.append_to_context(context),
        Ok(None) => {}
        Err(err) => {
            context.push_str(&format!(
                "\n\nFailed to read managed app-server stderr log: {err:#}"
            ));
        }
    }
}
