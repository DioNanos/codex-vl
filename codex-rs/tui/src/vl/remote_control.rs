use std::env;
use std::ffi::OsStr;

use ratatui::style::Stylize;
use ratatui::text::Line;
use serde_json::Value;
use tokio::process::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoteControlAction {
    Status,
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum RemoteControlParseError {
    UnsupportedToggle,
    Usage,
}

#[derive(Debug, Eq, PartialEq)]
struct CliOutput {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

pub(crate) fn parse_action(args: &str) -> Result<RemoteControlAction, RemoteControlParseError> {
    match args.trim().to_ascii_lowercase().as_str() {
        "" | "status" => Ok(RemoteControlAction::Status),
        "start" => Ok(RemoteControlAction::Start),
        "stop" => Ok(RemoteControlAction::Stop),
        "restart" => Ok(RemoteControlAction::Restart),
        "on" | "off" | "enable" | "disable" => Err(RemoteControlParseError::UnsupportedToggle),
        _ => Err(RemoteControlParseError::Usage),
    }
}

pub(crate) fn parse_error_message(error: RemoteControlParseError) -> (&'static str, &'static str) {
    match error {
        RemoteControlParseError::UnsupportedToggle => (
            "Usage: /remote-control [status|start|stop|restart]",
            "`on` and `off` need the upstream remote-control client and are not enabled in this build.",
        ),
        RemoteControlParseError::Usage => (
            "Usage: /remote-control [status|start|stop|restart]",
            "V1 manages daemon lifecycle only.",
        ),
    }
}

pub(crate) fn action_label(action: RemoteControlAction) -> &'static str {
    match action {
        RemoteControlAction::Status => "status",
        RemoteControlAction::Start => "start",
        RemoteControlAction::Stop => "stop",
        RemoteControlAction::Restart => "restart",
    }
}

pub(crate) async fn run_action(action: RemoteControlAction) -> String {
    match action {
        RemoteControlAction::Status => {
            let output = run_current_exe(["app-server", "daemon", "version"]).await;
            format_single_output(RemoteControlAction::Status, output)
        }
        RemoteControlAction::Start => {
            let output = run_current_exe(["remote-control", "start"]).await;
            format_single_output(RemoteControlAction::Start, output)
        }
        RemoteControlAction::Stop => {
            let output = run_current_exe(["remote-control", "stop"]).await;
            format_single_output(RemoteControlAction::Stop, output)
        }
        RemoteControlAction::Restart => {
            let stop_output = run_current_exe(["remote-control", "stop"]).await;
            if stop_output.status.is_some_and(|code| code != 0) {
                return format_single_output(RemoteControlAction::Restart, stop_output);
            }
            let start_output = run_current_exe(["remote-control", "start"]).await;
            format_single_output(RemoteControlAction::Restart, start_output)
        }
    }
}

async fn run_current_exe<I, S>(args: I) -> CliOutput
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let exe = match env::current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            return CliOutput {
                status: None,
                stdout: String::new(),
                stderr: format!("failed to resolve current executable: {err}"),
            };
        }
    };
    match Command::new(exe).args(args).output().await {
        Ok(output) => CliOutput {
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        },
        Err(err) => CliOutput {
            status: None,
            stdout: String::new(),
            stderr: err.to_string(),
        },
    }
}

fn format_single_output(action: RemoteControlAction, output: CliOutput) -> String {
    let heading = format!("Remote control {}", action_label(action));
    if output.status.is_some_and(|code| code == 0) {
        if let Ok(json) = serde_json::from_str::<Value>(&output.stdout) {
            return format_success(&heading, action, &json);
        }
        return format!("{heading}\nStatus: ok\n{}", output.stdout);
    }

    let detail = if output.stderr.is_empty() {
        output.stdout
    } else {
        output.stderr
    };
    if action == RemoteControlAction::Status {
        format!("{heading}\nDaemon: down\nDetail: {detail}")
    } else {
        format!("{heading}\nStatus: failed\nDetail: {detail}")
    }
}

fn format_success(heading: &str, action: RemoteControlAction, json: &Value) -> String {
    let mut lines = vec![heading.to_string()];
    let status = string_field(json, "status").unwrap_or("ok");
    let daemon = match action {
        RemoteControlAction::Status => "running".to_string(),
        _ => status.to_string(),
    };
    lines.push(format!("Daemon: {daemon}"));
    if matches!(
        action,
        RemoteControlAction::Start | RemoteControlAction::Restart
    ) {
        lines.push("Remote control: enabled".to_string());
    } else if let Some(enabled) = bool_field(json, "remoteControlEnabled") {
        lines.push(format!(
            "Remote control: {}",
            if enabled { "enabled" } else { "disabled" }
        ));
    }
    if let Some(socket_path) = string_field(json, "socketPath") {
        lines.push(format!("Socket: {socket_path}"));
    }
    if let Some(cli_version) = string_field(json, "cliVersion") {
        lines.push(format!("CLI: {cli_version}"));
    }
    if let Some(app_server_version) = string_field(json, "appServerVersion") {
        lines.push(format!("App server: {app_server_version}"));
    }
    lines.join("\n")
}

fn string_field<'a>(json: &'a Value, field: &str) -> Option<&'a str> {
    json.get(field).and_then(Value::as_str)
}

fn bool_field(json: &Value, field: &str) -> Option<bool> {
    json.get(field).and_then(Value::as_bool)
}

pub(crate) fn render_output(text: &str) -> Vec<Line<'static>> {
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                vec!["Remote: ".dim(), line.to_string().bold()].into()
            } else {
                vec!["        ".dim(), line.to_string().into()].into()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_action_accepts_v1_lifecycle_commands() {
        assert_eq!(parse_action(""), Ok(RemoteControlAction::Status));
        assert_eq!(parse_action("status"), Ok(RemoteControlAction::Status));
        assert_eq!(parse_action("start"), Ok(RemoteControlAction::Start));
        assert_eq!(parse_action("stop"), Ok(RemoteControlAction::Stop));
        assert_eq!(parse_action("restart"), Ok(RemoteControlAction::Restart));
    }

    #[test]
    fn parse_action_gates_client_enrollment_toggles() {
        assert_eq!(
            parse_action("on"),
            Err(RemoteControlParseError::UnsupportedToggle)
        );
        assert_eq!(
            parse_action("off"),
            Err(RemoteControlParseError::UnsupportedToggle)
        );
    }

    #[test]
    fn format_start_output_reports_enabled_remote_control() {
        let text = format_single_output(
            RemoteControlAction::Start,
            CliOutput {
                status: Some(0),
                stdout: r#"{"status":"alreadyRunning","socketPath":"/tmp/codex.sock","cliVersion":"0.132.0-alpha.1","appServerVersion":"0.132.0-alpha.1"}"#.to_string(),
                stderr: String::new(),
            },
        );
        assert!(text.contains("Daemon: alreadyRunning"));
        assert!(text.contains("Remote control: enabled"));
        assert!(text.contains("Socket: /tmp/codex.sock"));
    }

    #[test]
    fn format_status_failure_reports_daemon_down() {
        let text = format_single_output(
            RemoteControlAction::Status,
            CliOutput {
                status: Some(1),
                stdout: String::new(),
                stderr: "connection refused".to_string(),
            },
        );
        assert!(text.contains("Daemon: down"));
        assert!(text.contains("connection refused"));
    }
}
