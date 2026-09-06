//! Process fixture shim for the D174 terminal identity tests.
//!
//! The daemon lifecycle deliberately launches a `codex app-server` command.
//! This test-only binary supplies that command surface while delegating the
//! actual server process to the real `codex-app-server` binary.

use std::env;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = env::args_os();
    let _program = args.next();
    let first = args.next();
    match first.as_deref() {
        Some(value) if value == "daemon-start" => {
            codex_app_server_daemon::run(codex_app_server_daemon::LifecycleCommand::Start).await?;
            Ok(())
        }
        Some(value) if value == "daemon-stop" => {
            codex_app_server_daemon::run(codex_app_server_daemon::LifecycleCommand::Stop).await?;
            Ok(())
        }
        _ => {
            let app_server =
                PathBuf::from(env::var_os("D174_APP_SERVER_BIN").ok_or_else(|| {
                    anyhow::anyhow!("D174_APP_SERVER_BIN is required by the daemon shim")
                })?);
            let forwarded = if first.as_deref() == Some(std::ffi::OsStr::new("app-server")) {
                args.collect::<Vec<OsString>>()
            } else {
                let mut forwarded = Vec::new();
                if let Some(first) = first {
                    forwarded.push(first);
                }
                forwarded.extend(args);
                forwarded
            };
            Err(Command::new(app_server).args(forwarded).exec().into())
        }
    }
}
