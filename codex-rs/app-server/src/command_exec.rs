use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use codex_app_server_protocol::CommandExecOutputDeltaNotification;
use codex_app_server_protocol::CommandExecOutputStream;
use codex_app_server_protocol::CommandExecResizeParams;
use codex_app_server_protocol::CommandExecResizeResponse;
use codex_app_server_protocol::CommandExecResponse;
use codex_app_server_protocol::CommandExecTerminalSize;
use codex_app_server_protocol::CommandExecTerminateParams;
use codex_app_server_protocol::CommandExecTerminateResponse;
use codex_app_server_protocol::CommandExecWriteParams;
use codex_app_server_protocol::CommandExecWriteResponse;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ServerNotification;
use codex_core::config::StartedNetworkProxy;
use codex_core::exec::ExecExpiration;
use codex_core::exec::ExecExpirationOutcome;
use codex_core::exec::IO_DRAIN_TIMEOUT_MS;
use codex_core::sandboxing::ExecRequest;
use codex_protocol::exec_output::bytes_to_string_smart;
use codex_sandboxing::SandboxType;
use codex_utils_pty::DEFAULT_OUTPUT_BYTES_CAP;
use codex_utils_pty::ProcessHandle;
use codex_utils_pty::SpawnedProcess;
use codex_utils_pty::TerminalSize;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::watch;

use crate::error_code::internal_error;
use crate::error_code::invalid_params;
use crate::error_code::invalid_request;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::ConnectionRequestId;
use crate::outgoing_message::OutgoingMessageSender;

const EXEC_TIMEOUT_EXIT_CODE: i32 = 124;
const OUTPUT_CHUNK_SIZE_HINT: usize = 64 * 1024;

#[derive(Clone)]
pub(crate) struct CommandExecManager {
    sessions: Arc<Mutex<HashMap<ConnectionProcessId, CommandExecSession>>>,
    next_generated_process_id: Arc<AtomicI64>,
}

impl Default for CommandExecManager {
    fn default() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            next_generated_process_id: Arc::new(AtomicI64::new(1)),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ConnectionProcessId {
    connection_id: ConnectionId,
    process_id: InternalProcessId,
}

#[derive(Clone)]
enum CommandExecSession {
    Active {
        control_tx: mpsc::Sender<CommandControlRequest>,
    },
    UnsupportedWindowsSandbox,
}

enum CommandControl {
    Write { delta: Vec<u8>, close_stdin: bool },
    Resize { size: TerminalSize },
    Terminate,
}

struct CommandControlRequest {
    control: CommandControl,
    response_tx: Option<oneshot::Sender<Result<(), JSONRPCErrorError>>>,
}

pub(crate) struct StartCommandExecParams {
    pub(crate) outgoing: Arc<OutgoingMessageSender>,
    pub(crate) request_id: ConnectionRequestId,
    pub(crate) process_id: Option<String>,
    pub(crate) exec_request: ExecRequest,
    pub(crate) started_network_proxy: Option<StartedNetworkProxy>,
    pub(crate) tty: bool,
    pub(crate) stream_stdin: bool,
    pub(crate) stream_stdout_stderr: bool,
    pub(crate) output_bytes_cap: Option<usize>,
    pub(crate) size: Option<TerminalSize>,
}

struct RunCommandParams {
    outgoing: Arc<OutgoingMessageSender>,
    request_id: ConnectionRequestId,
    process_id: Option<String>,
    spawned: SpawnedProcess,
    control_rx: mpsc::Receiver<CommandControlRequest>,
    stream_stdin: bool,
    stream_stdout_stderr: bool,
    expiration: ExecExpiration,
    output_bytes_cap: Option<usize>,
}

struct SpawnProcessOutputParams {
    connection_id: ConnectionId,
    process_id: Option<String>,
    output_rx: mpsc::Receiver<Vec<u8>>,
    stdio_timeout_rx: watch::Receiver<bool>,
    outgoing: Arc<OutgoingMessageSender>,
    stream: CommandExecOutputStream,
    stream_output: bool,
    output_bytes_cap: Option<usize>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum InternalProcessId {
    Generated(i64),
    Client(String),
}

trait InternalProcessIdExt {
    fn error_repr(&self) -> String;
}

impl InternalProcessIdExt for InternalProcessId {
    fn error_repr(&self) -> String {
        match self {
            Self::Generated(id) => id.to_string(),
            Self::Client(id) => serde_json::to_string(id).unwrap_or_else(|_| format!("{id:?}")),
        }
    }
}

/// The environment map the local `command/exec` launch boundary actually hands
/// to the external command.
///
/// This is the last stop before the PTY/pipe spawn branches: by the time the
/// recomposed `ExecRequest` reaches this point, the shell policy, the protocol
/// overrides, and the sandbox transform have all had their say, so the shared
/// body finalizes the map for the target platform — scrubbing the
/// launcher-private install metadata that a late override could have
/// reintroduced after the earlier `create_env` boundary. Permissions, network,
/// and stdio configuration are not touched.
fn external_command_exec_env_for_target(
    mut env: HashMap<String, String>,
    target_os: &str,
) -> HashMap<String, String> {
    codex_utils_process::scrub_private_launcher_env(&mut env, target_os);
    env
}

impl CommandExecManager {
    pub(crate) async fn start(
        &self,
        params: StartCommandExecParams,
    ) -> Result<(), JSONRPCErrorError> {
        let StartCommandExecParams {
            outgoing,
            request_id,
            process_id,
            exec_request,
            started_network_proxy,
            tty,
            stream_stdin,
            stream_stdout_stderr,
            output_bytes_cap,
            size,
        } = params;
        if process_id.is_none() && (tty || stream_stdin || stream_stdout_stderr) {
            return Err(invalid_request(
                "command/exec tty or streaming requires a client-supplied processId",
            ));
        }
        let process_id = process_id.map_or_else(
            || {
                InternalProcessId::Generated(
                    self.next_generated_process_id
                        .fetch_add(1, Ordering::Relaxed),
                )
            },
            InternalProcessId::Client,
        );
        let process_key = ConnectionProcessId {
            connection_id: request_id.connection_id,
            process_id: process_id.clone(),
        };

        if matches!(exec_request.sandbox, SandboxType::WindowsRestrictedToken) {
            if tty || stream_stdin || stream_stdout_stderr {
                return Err(invalid_request(
                    "streaming command/exec is not supported with windows sandbox",
                ));
            }
            if output_bytes_cap != Some(DEFAULT_OUTPUT_BYTES_CAP) {
                return Err(invalid_request(
                    "custom outputBytesCap is not supported with windows sandbox",
                ));
            }
            if let InternalProcessId::Client(_) = &process_id {
                let mut sessions = self.sessions.lock().await;
                if sessions.contains_key(&process_key) {
                    return Err(invalid_request(format!(
                        "duplicate active command/exec process id: {}",
                        process_key.process_id.error_repr(),
                    )));
                }
                sessions.insert(
                    process_key.clone(),
                    CommandExecSession::UnsupportedWindowsSandbox,
                );
            }
            let sessions = Arc::clone(&self.sessions);
            tokio::spawn(async move {
                let _started_network_proxy = started_network_proxy;
                match codex_core::sandboxing::execute_env(exec_request, /*stdout_stream*/ None)
                    .await
                {
                    Ok(output) => {
                        outgoing
                            .send_response(
                                request_id,
                                CommandExecResponse {
                                    exit_code: output.exit_code,
                                    stdout: output.stdout.text,
                                    stderr: output.stderr.text,
                                },
                            )
                            .await;
                    }
                    Err(err) => {
                        outgoing
                            .send_error(request_id, internal_error(format!("exec failed: {err}")))
                            .await;
                    }
                }
                sessions.lock().await.remove(&process_key);
            });
            return Ok(());
        }

        let ExecRequest {
            command,
            cwd,
            env,
            expiration,
            sandbox: _sandbox,
            arg0,
            ..
        } = exec_request;
        // TODO(anp): Keep PathUri through the local command launch boundary.
        let cwd = cwd
            .to_abs_path()
            .map_err(|err| invalid_request(format!("invalid command cwd: {err}")))?;
        let env = external_command_exec_env_for_target(env, std::env::consts::OS);

        let stream_stdin = tty || stream_stdin;
        let stream_stdout_stderr = tty || stream_stdout_stderr;
        let (control_tx, control_rx) = mpsc::channel(32);
        let notification_process_id = match &process_id {
            InternalProcessId::Generated(_) => None,
            InternalProcessId::Client(process_id) => Some(process_id.clone()),
        };

        let sessions = Arc::clone(&self.sessions);
        let (program, args) = command
            .split_first()
            .ok_or_else(|| invalid_request("command must not be empty"))?;
        {
            let mut sessions = self.sessions.lock().await;
            if sessions.contains_key(&process_key) {
                return Err(invalid_request(format!(
                    "duplicate active command/exec process id: {}",
                    process_key.process_id.error_repr(),
                )));
            }
            sessions.insert(
                process_key.clone(),
                CommandExecSession::Active { control_tx },
            );
        }
        let spawned = if tty {
            codex_utils_pty::spawn_pty_process(
                program,
                args,
                cwd.as_path(),
                &env,
                &arg0,
                size.unwrap_or_default(),
                codex_utils_pty::ChildFds::Inherited(&[]),
            )
            .await
        } else if stream_stdin {
            codex_utils_pty::spawn_pipe_process(program, args, cwd.as_path(), &env, &arg0, &[])
                .await
        } else {
            codex_utils_pty::spawn_pipe_process_no_stdin(
                program,
                args,
                cwd.as_path(),
                &env,
                &arg0,
                &[],
            )
            .await
        };
        let spawned = match spawned {
            Ok(spawned) => spawned,
            Err(err) => {
                self.sessions.lock().await.remove(&process_key);
                return Err(internal_error(format!("failed to spawn command: {err}")));
            }
        };
        tokio::spawn(async move {
            let _started_network_proxy = started_network_proxy;
            run_command(RunCommandParams {
                outgoing,
                request_id: request_id.clone(),
                process_id: notification_process_id,
                spawned,
                control_rx,
                stream_stdin,
                stream_stdout_stderr,
                expiration,
                output_bytes_cap,
            })
            .await;
            sessions.lock().await.remove(&process_key);
        });
        Ok(())
    }

    pub(crate) async fn write(
        &self,
        request_id: ConnectionRequestId,
        params: CommandExecWriteParams,
    ) -> Result<CommandExecWriteResponse, JSONRPCErrorError> {
        if params.delta_base64.is_none() && !params.close_stdin {
            return Err(invalid_params(
                "command/exec/write requires deltaBase64 or closeStdin",
            ));
        }

        let delta = match params.delta_base64 {
            Some(delta_base64) => STANDARD
                .decode(delta_base64)
                .map_err(|err| invalid_params(format!("invalid deltaBase64: {err}")))?,
            None => Vec::new(),
        };

        let target_process_id = ConnectionProcessId {
            connection_id: request_id.connection_id,
            process_id: InternalProcessId::Client(params.process_id),
        };
        self.send_control(
            target_process_id,
            CommandControl::Write {
                delta,
                close_stdin: params.close_stdin,
            },
        )
        .await?;

        Ok(CommandExecWriteResponse {})
    }

    pub(crate) async fn terminate(
        &self,
        request_id: ConnectionRequestId,
        params: CommandExecTerminateParams,
    ) -> Result<CommandExecTerminateResponse, JSONRPCErrorError> {
        let target_process_id = ConnectionProcessId {
            connection_id: request_id.connection_id,
            process_id: InternalProcessId::Client(params.process_id),
        };
        self.send_control(target_process_id, CommandControl::Terminate)
            .await?;
        Ok(CommandExecTerminateResponse {})
    }

    pub(crate) async fn resize(
        &self,
        request_id: ConnectionRequestId,
        params: CommandExecResizeParams,
    ) -> Result<CommandExecResizeResponse, JSONRPCErrorError> {
        let target_process_id = ConnectionProcessId {
            connection_id: request_id.connection_id,
            process_id: InternalProcessId::Client(params.process_id),
        };
        self.send_control(
            target_process_id,
            CommandControl::Resize {
                size: terminal_size_from_protocol(params.size)?,
            },
        )
        .await?;
        Ok(CommandExecResizeResponse {})
    }

    pub(crate) async fn connection_closed(&self, connection_id: ConnectionId) {
        let controls = {
            let mut sessions = self.sessions.lock().await;
            let process_ids = sessions
                .keys()
                .filter(|process_id| process_id.connection_id == connection_id)
                .cloned()
                .collect::<Vec<_>>();
            let mut controls = Vec::with_capacity(process_ids.len());
            for process_id in process_ids {
                if let Some(control) = sessions.remove(&process_id) {
                    controls.push(control);
                }
            }
            controls
        };

        for control in controls {
            if let CommandExecSession::Active { control_tx } = control {
                let _ = control_tx
                    .send(CommandControlRequest {
                        control: CommandControl::Terminate,
                        response_tx: None,
                    })
                    .await;
            }
        }
    }

    async fn send_control(
        &self,
        process_id: ConnectionProcessId,
        control: CommandControl,
    ) -> Result<(), JSONRPCErrorError> {
        let session = {
            self.sessions
                .lock()
                .await
                .get(&process_id)
                .cloned()
                .ok_or_else(|| {
                    invalid_request(format!(
                        "no active command/exec for process id {}",
                        process_id.process_id.error_repr(),
                    ))
                })?
        };
        let CommandExecSession::Active { control_tx } = session else {
            return Err(invalid_request(
                "command/exec/write, command/exec/terminate, and command/exec/resize are not supported for windows sandbox processes",
            ));
        };
        let (response_tx, response_rx) = oneshot::channel();
        let request = CommandControlRequest {
            control,
            response_tx: Some(response_tx),
        };
        control_tx
            .send(request)
            .await
            .map_err(|_| command_no_longer_running_error(&process_id.process_id))?;
        response_rx
            .await
            .map_err(|_| command_no_longer_running_error(&process_id.process_id))?
    }
}

async fn run_command(params: RunCommandParams) {
    let RunCommandParams {
        outgoing,
        request_id,
        process_id,
        spawned,
        control_rx,
        stream_stdin,
        stream_stdout_stderr,
        expiration,
        output_bytes_cap,
    } = params;
    let mut control_rx = control_rx;
    let mut control_open = true;
    let expiration = expiration.wait_with_outcome();
    tokio::pin!(expiration);
    let SpawnedProcess {
        session,
        stdout_rx,
        stderr_rx,
        exit_rx,
    } = spawned;
    tokio::pin!(exit_rx);
    let mut expiration_outcome = None;
    let (stdio_timeout_tx, stdio_timeout_rx) = watch::channel(false);

    let stdout_handle = spawn_process_output(SpawnProcessOutputParams {
        connection_id: request_id.connection_id,
        process_id: process_id.clone(),
        output_rx: stdout_rx,
        stdio_timeout_rx: stdio_timeout_rx.clone(),
        outgoing: Arc::clone(&outgoing),
        stream: CommandExecOutputStream::Stdout,
        stream_output: stream_stdout_stderr,
        output_bytes_cap,
    });
    let stderr_handle = spawn_process_output(SpawnProcessOutputParams {
        connection_id: request_id.connection_id,
        process_id: process_id.clone(),
        output_rx: stderr_rx,
        stdio_timeout_rx,
        outgoing: Arc::clone(&outgoing),
        stream: CommandExecOutputStream::Stderr,
        stream_output: stream_stdout_stderr,
        output_bytes_cap,
    });

    let exit_code = loop {
        tokio::select! {
            control = control_rx.recv(), if control_open => {
                match control {
                    Some(CommandControlRequest { control, response_tx }) => {
                        let result = match control {
                            CommandControl::Write { delta, close_stdin } => {
                                handle_process_write(
                                    &session,
                                    stream_stdin,
                                    delta,
                                    close_stdin,
                                ).await
                            }
                            CommandControl::Resize { size } => {
                                handle_process_resize(&session, size)
                            }
                            CommandControl::Terminate => {
                                session.request_terminate();
                                Ok(())
                            }
                        };
                        if let Some(response_tx) = response_tx {
                            let _ = response_tx.send(result);
                        }
                    },
                    None => {
                        control_open = false;
                        session.request_terminate();
                    }
                }
            }
            outcome = &mut expiration, if expiration_outcome.is_none() => {
                expiration_outcome = Some(outcome);
                session.request_terminate();
            }
            exit = &mut exit_rx => {
                if matches!(expiration_outcome, Some(ExecExpirationOutcome::TimedOut)) {
                    break EXEC_TIMEOUT_EXIT_CODE;
                } else {
                    break exit.unwrap_or(-1);
                }
            }
        }
    };

    let timeout_handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(IO_DRAIN_TIMEOUT_MS)).await;
        let _ = stdio_timeout_tx.send(true);
    });

    let stdout = stdout_handle.await.unwrap_or_default();
    let stderr = stderr_handle.await.unwrap_or_default();
    timeout_handle.abort();

    outgoing
        .send_response(
            request_id,
            CommandExecResponse {
                exit_code,
                stdout,
                stderr,
            },
        )
        .await;
}

fn spawn_process_output(params: SpawnProcessOutputParams) -> tokio::task::JoinHandle<String> {
    let SpawnProcessOutputParams {
        connection_id,
        process_id,
        mut output_rx,
        mut stdio_timeout_rx,
        outgoing,
        stream,
        stream_output,
        output_bytes_cap,
    } = params;
    tokio::spawn(async move {
        let mut buffer: Vec<u8> = Vec::new();
        let mut observed_num_bytes = 0usize;
        loop {
            let mut chunk = tokio::select! {
                chunk = output_rx.recv() => match chunk {
                    Some(chunk) => chunk,
                    None => break,
                },
                _ = stdio_timeout_rx.wait_for(|&v| v) => break,
            };
            // Individual chunks are at most 8KiB, so overshooting a bit is acceptable.
            while chunk.len() < OUTPUT_CHUNK_SIZE_HINT
                && let Ok(next_chunk) = output_rx.try_recv()
            {
                chunk.extend_from_slice(&next_chunk);
            }
            let capped_chunk = match output_bytes_cap {
                Some(output_bytes_cap) => {
                    let capped_chunk_len = output_bytes_cap
                        .saturating_sub(observed_num_bytes)
                        .min(chunk.len());
                    observed_num_bytes += capped_chunk_len;
                    &chunk[0..capped_chunk_len]
                }
                None => chunk.as_slice(),
            };
            let cap_reached = Some(observed_num_bytes) == output_bytes_cap;
            if let (true, Some(process_id)) = (stream_output, process_id.as_ref()) {
                outgoing
                    .send_server_notification_to_connection_and_wait(
                        connection_id,
                        ServerNotification::CommandExecOutputDelta(
                            CommandExecOutputDeltaNotification {
                                process_id: process_id.clone(),
                                stream,
                                delta_base64: STANDARD.encode(capped_chunk),
                                cap_reached,
                            },
                        ),
                    )
                    .await;
            } else if !stream_output {
                buffer.extend_from_slice(capped_chunk);
            }
            if cap_reached {
                break;
            }
        }
        bytes_to_string_smart(&buffer)
    })
}

async fn handle_process_write(
    session: &ProcessHandle,
    stream_stdin: bool,
    delta: Vec<u8>,
    close_stdin: bool,
) -> Result<(), JSONRPCErrorError> {
    if !stream_stdin {
        return Err(invalid_request(
            "stdin streaming is not enabled for this command/exec",
        ));
    }
    if !delta.is_empty() {
        session
            .writer_sender()
            .send(delta)
            .await
            .map_err(|_| invalid_request("stdin is already closed"))?;
    }
    if close_stdin {
        session.close_stdin();
    }
    Ok(())
}

fn handle_process_resize(
    session: &ProcessHandle,
    size: TerminalSize,
) -> Result<(), JSONRPCErrorError> {
    session
        .resize(size)
        .map_err(|err| invalid_request(format!("failed to resize PTY: {err}")))
}

pub(crate) fn terminal_size_from_protocol(
    size: CommandExecTerminalSize,
) -> Result<TerminalSize, JSONRPCErrorError> {
    if size.rows == 0 || size.cols == 0 {
        return Err(invalid_params(
            "command/exec size rows and cols must be greater than 0",
        ));
    }
    Ok(TerminalSize {
        rows: size.rows,
        cols: size.cols,
    })
}

fn command_no_longer_running_error(process_id: &InternalProcessId) -> JSONRPCErrorError {
    invalid_request(format!(
        "command/exec {} is no longer running",
        process_id.error_repr(),
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::error_code::INVALID_REQUEST_ERROR_CODE;
    use codex_protocol::config_types::WindowsSandboxLevel;
    use codex_protocol::models::PermissionProfile;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;
    #[cfg(not(target_os = "windows"))]
    use tokio::time::Duration;
    #[cfg(not(target_os = "windows"))]
    use tokio::time::timeout;
    #[cfg(not(target_os = "windows"))]
    use tokio_util::sync::CancellationToken;

    use super::*;
    #[cfg(not(target_os = "windows"))]
    use crate::outgoing_message::OutgoingEnvelope;
    #[cfg(not(target_os = "windows"))]
    use crate::outgoing_message::OutgoingMessage;

    fn windows_sandbox_exec_request() -> ExecRequest {
        let cwd = AbsolutePathBuf::current_dir().expect("current dir");
        ExecRequest::new(
            vec!["cmd".to_string()],
            cwd.clone(),
            HashMap::new(),
            /*network*/ None,
            /*network_environment_id*/ None,
            ExecExpiration::DefaultTimeout,
            codex_core::exec::ExecCapturePolicy::ShellTool,
            SandboxType::WindowsRestrictedToken,
            vec![cwd],
            WindowsSandboxLevel::Disabled,
            PermissionProfile::read_only(),
            /*arg0*/ None,
        )
    }

    #[tokio::test]
    async fn windows_sandbox_streaming_exec_is_rejected() {
        let (tx, _rx) = mpsc::channel(1);
        let manager = CommandExecManager::default();
        let err = manager
            .start(StartCommandExecParams {
                outgoing: Arc::new(OutgoingMessageSender::new(
                    tx,
                    codex_analytics::AnalyticsEventsClient::disabled(),
                )),
                request_id: ConnectionRequestId {
                    connection_id: ConnectionId(1),
                    request_id: codex_app_server_protocol::RequestId::Integer(42),
                },
                process_id: Some("proc-42".to_string()),
                exec_request: windows_sandbox_exec_request(),
                started_network_proxy: None,
                tty: false,
                stream_stdin: false,
                stream_stdout_stderr: true,
                output_bytes_cap: None,
                size: None,
            })
            .await
            .expect_err("streaming windows sandbox exec should be rejected");

        assert_eq!(err.code, INVALID_REQUEST_ERROR_CODE);
        assert_eq!(
            err.message,
            "streaming command/exec is not supported with windows sandbox"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn windows_sandbox_non_streaming_exec_uses_execution_path() {
        let (tx, mut rx) = mpsc::channel(1);
        let manager = CommandExecManager::default();
        let request_id = ConnectionRequestId {
            connection_id: ConnectionId(7),
            request_id: codex_app_server_protocol::RequestId::Integer(99),
        };

        manager
            .start(StartCommandExecParams {
                outgoing: Arc::new(OutgoingMessageSender::new(
                    tx,
                    codex_analytics::AnalyticsEventsClient::disabled(),
                )),
                request_id: request_id.clone(),
                process_id: Some("proc-99".to_string()),
                exec_request: windows_sandbox_exec_request(),
                started_network_proxy: None,
                tty: false,
                stream_stdin: false,
                stream_stdout_stderr: false,
                output_bytes_cap: Some(DEFAULT_OUTPUT_BYTES_CAP),
                size: None,
            })
            .await
            .expect("non-streaming windows sandbox exec should start");

        let envelope = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for outgoing message")
            .expect("channel closed before outgoing message");
        let OutgoingEnvelope::ToConnection {
            connection_id,
            message,
            ..
        } = envelope
        else {
            panic!("expected connection-scoped outgoing message");
        };
        assert_eq!(connection_id, request_id.connection_id);
        let OutgoingMessage::Error(error) = message else {
            panic!("expected execution failure to be reported as an error");
        };
        assert_eq!(error.id, request_id.request_id);
        assert!(error.error.message.starts_with("exec failed:"));
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn cancellation_expiration_keeps_process_alive_until_terminated() {
        let (tx, mut rx) = mpsc::channel(4);
        let manager = CommandExecManager::default();
        let request_id = ConnectionRequestId {
            connection_id: ConnectionId(8),
            request_id: codex_app_server_protocol::RequestId::Integer(100),
        };
        let cwd = AbsolutePathBuf::current_dir().expect("current dir");

        manager
            .start(StartCommandExecParams {
                outgoing: Arc::new(OutgoingMessageSender::new(
                    tx,
                    codex_analytics::AnalyticsEventsClient::disabled(),
                )),
                request_id: request_id.clone(),
                process_id: Some("proc-100".to_string()),
                exec_request: ExecRequest::new(
                    vec!["sh".to_string(), "-lc".to_string(), "sleep 30".to_string()],
                    cwd.clone(),
                    HashMap::new(),
                    /*network*/ None,
                    /*network_environment_id*/ None,
                    ExecExpiration::Cancellation(CancellationToken::new()),
                    codex_core::exec::ExecCapturePolicy::ShellTool,
                    SandboxType::None,
                    vec![cwd.clone()],
                    WindowsSandboxLevel::Disabled,
                    PermissionProfile::read_only(),
                    /*arg0*/ None,
                ),
                started_network_proxy: None,
                tty: false,
                stream_stdin: false,
                stream_stdout_stderr: false,
                output_bytes_cap: Some(DEFAULT_OUTPUT_BYTES_CAP),
                size: None,
            })
            .await
            .expect("cancellation-based exec should start");

        assert!(
            timeout(Duration::from_millis(250), rx.recv())
                .await
                .is_err(),
            "command/exec should remain active until explicit termination",
        );

        manager
            .terminate(
                request_id.clone(),
                CommandExecTerminateParams {
                    process_id: "proc-100".to_string(),
                },
            )
            .await
            .expect("terminate should succeed");

        let envelope = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for outgoing message")
            .expect("channel closed before outgoing message");
        let OutgoingEnvelope::ToConnection {
            connection_id,
            message,
            ..
        } = envelope
        else {
            panic!("expected connection-scoped outgoing message");
        };
        assert_eq!(connection_id, request_id.connection_id);
        let OutgoingMessage::Response(response) = message else {
            panic!("expected execution response after termination");
        };
        assert_eq!(response.id, request_id.request_id);
        let codex_app_server_protocol::ClientResponsePayload::OneOffCommandExec(response) =
            *response.result
        else {
            panic!("expected command/exec response");
        };
        assert_ne!(response.exit_code, 0);
        assert_eq!(response.stdout, "");
        // The deferred response now drains any already-emitted stderr before
        // replying, so shell startup noise is allowed here.
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn timeout_or_cancellation_reports_cancellation_without_timeout_exit_code() {
        let (tx, mut rx) = mpsc::channel(4);
        let manager = CommandExecManager::default();
        let request_id = ConnectionRequestId {
            connection_id: ConnectionId(9),
            request_id: codex_app_server_protocol::RequestId::Integer(101),
        };
        let cancellation = CancellationToken::new();
        let cancel = cancellation.clone();
        let cwd = AbsolutePathBuf::current_dir().expect("current dir");

        manager
            .start(StartCommandExecParams {
                outgoing: Arc::new(OutgoingMessageSender::new(
                    tx,
                    codex_analytics::AnalyticsEventsClient::disabled(),
                )),
                request_id: request_id.clone(),
                process_id: Some("proc-101".to_string()),
                exec_request: ExecRequest::new(
                    vec!["sh".to_string(), "-lc".to_string(), "sleep 30".to_string()],
                    cwd.clone(),
                    HashMap::new(),
                    /*network*/ None,
                    /*network_environment_id*/ None,
                    ExecExpiration::TimeoutOrCancellation {
                        timeout: Duration::from_secs(30),
                        cancellation,
                    },
                    codex_core::exec::ExecCapturePolicy::ShellTool,
                    SandboxType::None,
                    vec![cwd],
                    WindowsSandboxLevel::Disabled,
                    PermissionProfile::read_only(),
                    /*arg0*/ None,
                ),
                started_network_proxy: None,
                tty: false,
                stream_stdin: false,
                stream_stdout_stderr: false,
                output_bytes_cap: Some(DEFAULT_OUTPUT_BYTES_CAP),
                size: None,
            })
            .await
            .expect("timeout-or-cancellation exec should start");

        cancel.cancel();

        let envelope = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for outgoing message")
            .expect("channel closed before outgoing message");
        let OutgoingEnvelope::ToConnection {
            connection_id,
            message,
            ..
        } = envelope
        else {
            panic!("expected connection-scoped outgoing message");
        };
        assert_eq!(connection_id, request_id.connection_id);
        let OutgoingMessage::Response(response) = message else {
            panic!("expected execution response after cancellation");
        };
        assert_eq!(response.id, request_id.request_id);
        let codex_app_server_protocol::ClientResponsePayload::OneOffCommandExec(response) =
            *response.result
        else {
            panic!("expected command/exec response");
        };
        assert_ne!(response.exit_code, EXEC_TIMEOUT_EXIT_CODE);
    }

    #[tokio::test]
    async fn windows_sandbox_process_ids_reject_write_requests() {
        let manager = CommandExecManager::default();
        let request_id = ConnectionRequestId {
            connection_id: ConnectionId(11),
            request_id: codex_app_server_protocol::RequestId::Integer(1),
        };
        let process_id = ConnectionProcessId {
            connection_id: request_id.connection_id,
            process_id: InternalProcessId::Client("proc-11".to_string()),
        };
        manager
            .sessions
            .lock()
            .await
            .insert(process_id, CommandExecSession::UnsupportedWindowsSandbox);

        let err = manager
            .write(
                request_id,
                CommandExecWriteParams {
                    process_id: "proc-11".to_string(),
                    delta_base64: Some(STANDARD.encode("hello")),
                    close_stdin: false,
                },
            )
            .await
            .expect_err("windows sandbox process ids should reject command/exec/write");

        assert_eq!(err.code, INVALID_REQUEST_ERROR_CODE);
        assert_eq!(
            err.message,
            "command/exec/write, command/exec/terminate, and command/exec/resize are not supported for windows sandbox processes"
        );
    }

    #[tokio::test]
    async fn windows_sandbox_process_ids_reject_terminate_requests() {
        let manager = CommandExecManager::default();
        let request_id = ConnectionRequestId {
            connection_id: ConnectionId(12),
            request_id: codex_app_server_protocol::RequestId::Integer(2),
        };
        let process_id = ConnectionProcessId {
            connection_id: request_id.connection_id,
            process_id: InternalProcessId::Client("proc-12".to_string()),
        };
        manager
            .sessions
            .lock()
            .await
            .insert(process_id, CommandExecSession::UnsupportedWindowsSandbox);

        let err = manager
            .terminate(
                request_id,
                CommandExecTerminateParams {
                    process_id: "proc-12".to_string(),
                },
            )
            .await
            .expect_err("windows sandbox process ids should reject command/exec/terminate");

        assert_eq!(err.code, INVALID_REQUEST_ERROR_CODE);
        assert_eq!(
            err.message,
            "command/exec/write, command/exec/terminate, and command/exec/resize are not supported for windows sandbox processes"
        );
    }

    #[tokio::test]
    async fn dropped_control_request_is_reported_as_not_running() {
        let manager = CommandExecManager::default();
        let request_id = ConnectionRequestId {
            connection_id: ConnectionId(13),
            request_id: codex_app_server_protocol::RequestId::Integer(3),
        };
        let process_id = InternalProcessId::Client("proc-13".to_string());
        let (control_tx, mut control_rx) = mpsc::channel(1);
        manager.sessions.lock().await.insert(
            ConnectionProcessId {
                connection_id: request_id.connection_id,
                process_id: process_id.clone(),
            },
            CommandExecSession::Active { control_tx },
        );

        tokio::spawn(async move {
            let _request = control_rx
                .recv()
                .await
                .expect("expected queued control request");
        });

        let err = manager
            .terminate(
                request_id,
                CommandExecTerminateParams {
                    process_id: "proc-13".to_string(),
                },
            )
            .await
            .expect_err("dropped control request should be treated as not running");

        assert_eq!(err.code, INVALID_REQUEST_ERROR_CODE);
        assert_eq!(err.message, "command/exec \"proc-13\" is no longer running");
    }

    /// Sentinel through the shared production body at the local launch
    /// boundary: the map the PTY/pipe branches actually receive drops the six
    /// launcher names even when a late override reintroduced them after the
    /// earlier create_env boundary, in every case variant; an explicitly unset
    /// name stays absent, and a foreign entry survives intact. The non-Android
    /// target keeps the names; the test never scrubs the result afterwards.
    #[test]
    fn sentinel_command_exec_env_drops_late_launcher_overrides_for_android() {
        let mut env = HashMap::new();
        for name in codex_utils_process::PRIVATE_LAUNCHER_ENV_VARS {
            env.insert((*name).to_string(), "late-override".to_string());
        }
        env.insert(
            "codex_managed_by_npm".to_string(),
            "case-variant".to_string(),
        );
        env.insert("COMMAND_EXEC_KEEP_ME".to_string(), "kept".to_string());

        let android = external_command_exec_env_for_target(
            env.clone(),
            codex_utils_process::ANDROID_TARGET_OS,
        );
        for name in codex_utils_process::PRIVATE_LAUNCHER_ENV_VARS {
            assert!(!android.contains_key(*name), "{name} leaked");
        }
        assert!(
            !android.contains_key("codex_managed_by_npm"),
            "case-variant of a reserved name leaked"
        );
        assert!(
            !android.contains_key("CODEX_SELF_EXE"),
            "an explicitly unset name must not come back"
        );
        assert_eq!(
            android.get("COMMAND_EXEC_KEEP_ME").map(String::as_str),
            Some("kept")
        );
        assert_eq!(android.len(), 1);

        let host = external_command_exec_env_for_target(env, "linux");
        assert_eq!(
            host.get("CODEX_SELF_EXE").map(String::as_str),
            Some("late-override"),
            "the non-Android target must not scrub"
        );
        assert_eq!(
            host.len(),
            codex_utils_process::PRIVATE_LAUNCHER_ENV_VARS.len() + 2
        );
    }
}
