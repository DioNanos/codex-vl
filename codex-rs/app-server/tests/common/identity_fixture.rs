use anyhow::Context;
use anyhow::Result;
use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::IdentityChallenge;
use codex_app_server_protocol::IdentityClaims;
use codex_app_server_protocol::IdentityKind;
use codex_app_server_protocol::IdentityOrigin;
use codex_app_server_protocol::IdentityProof;
use codex_app_server_protocol::InitializeCapabilities;
use codex_app_server_protocol::InitializeParams;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use futures::SinkExt;
use futures::StreamExt;
use serde_json::Value;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tempfile::TempDir;
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::time::Duration;
use tokio::time::sleep;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::client_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

type UnixWebSocket = WebSocketStream<UnixStream>;

const IDENTITY_FD_ENV: &str = "NEXUSCREW_IDENTITY_FD";

/// Parent-side half of the verifier channel handed to a protected daemon,
/// plus the thread that answers `nexuscrew/identity/verify` on it. Dropping
/// the parent half closes the pipe and lets the serving thread exit.
struct ChannelServe {
    _parent: std::os::unix::net::UnixStream,
    server: std::thread::JoinHandle<()>,
}

/// Hand a fresh fd3/fd4 verifier channel to the daemon process tree and keep
/// the answering side in this process. The descriptors are duplicated onto
/// 3/4 before exec and their close-on-exec flag is cleared, so they survive
/// the shim and the daemon manager into the real app-server, mirroring how a
/// cell supervisor hands the channel to a managed child.
fn add_identity_channel(command: &mut Command) -> Result<ChannelServe> {
    use std::os::fd::AsRawFd;
    let (parent, child) =
        std::os::unix::net::UnixStream::pair().context("create verifier channel pair")?;
    let write_fd = child
        .try_clone()
        .context("clone verifier write descriptor")?;
    let read_fd = child
        .try_clone()
        .context("clone verifier read descriptor")?;
    let peer = parent.try_clone().context("clone verifier peer")?;
    unsafe {
        command.pre_exec(move || {
            for (source, target) in [(&write_fd, 3i32), (&read_fd, 4i32)] {
                if libc::dup2(source.as_raw_fd(), target) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::fcntl(target, libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    command.env(IDENTITY_FD_ENV, "3:4");
    let server = std::thread::spawn(move || serve_verify_requests(peer));
    Ok(ChannelServe {
        _parent: parent,
        server,
    })
}

/// Minimal authority-side responder: one JSON-RPC line per request, echoing
/// the proof claims back as the normalized `ok:true` claims. The C6 stub
/// authority already derives those claims from the daemon-issued challenge,
/// so the echo keeps the daemon-side challenge-tuple check meaningful.
fn serve_verify_requests(peer: std::os::unix::net::UnixStream) {
    use std::io::BufRead;
    use std::io::Write;
    let mut writer = match peer.try_clone() {
        Ok(writer) => writer,
        Err(_) => return,
    };
    let mut reader = std::io::BufReader::new(peer);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let Ok(request) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        let claims = request
            .pointer("/params/proof/claims")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request.get("id").cloned().unwrap_or(serde_json::Value::from(0)),
            "result": {"ok": true, "v": 1, "claims": claims},
        });
        if writeln!(writer, "{response}").is_err() {
            break;
        }
    }
}

fn scrub_shared_identity_env(command: &mut Command) {
    command
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env_remove("NEXUSCREW_MCP_SESSION");
}

/// In-repo authority substitute for the C6 process fixture. It only issues a
/// proof after reading the daemon-issued challenge; C6 therefore exercises the
/// real app-server/daemon/client transport, while NC-5 still covers the real
/// NexusCrew authority and bus.
#[derive(Clone, Copy)]
pub struct IdentityAuthorityStub;

impl IdentityAuthorityStub {
    pub fn issue(
        &self,
        challenge: &IdentityChallenge,
        owner: &str,
        cell: &str,
        incarnation: &str,
        cwd: Option<&str>,
    ) -> Result<IdentityProof> {
        let claims = IdentityClaims {
            issuer_owner: owner.to_string(),
            audience: challenge.audience.clone(),
            owner_instance_id: owner.to_string(),
            cell_id: cell.to_string(),
            tmux_session: format!("{cell}-session"),
            incarnation_id: incarnation.to_string(),
            launch_epoch: format!("epoch-{incarnation}"),
            daemon_boot_id: challenge.daemon_boot_id.clone(),
            connection_id: challenge.connection_id.clone(),
            binding_id: format!("binding-{owner}-{incarnation}"),
            origin: IdentityOrigin::LocalTui,
            scopes: vec!["thread/start".to_string(), "thread/resume".to_string()],
            issued_at: challenge.issued_at,
            not_before: challenge.issued_at,
            expires_at: challenge.expires_at,
            nonce: challenge.nonce.clone(),
            thread_id: None,
            cwd: cwd.map(str::to_string),
            live_host: None,
        };
        // The verifier reads the opaque envelope as JSON and forwards the raw
        // inner proof to the authority; keep a generation plus the claims so
        // the fixture authority can normalize them back.
        let proof = serde_json::json!({
            "generation": 1,
            "proof": {"claims": serde_json::to_value(&claims)?},
        })
        .to_string();
        Ok(IdentityProof {
            version: "1".to_string(),
            kind: IdentityKind::ConnectionV1,
            challenge: challenge.clone(),
            claims,
            proof,
        })
    }
}

pub struct IdentityFixture {
    _home: TempDir,
    challenge_file: PathBuf,
    shim: PathBuf,
    app_server: PathBuf,
    authority: IdentityAuthorityStub,
    stopped: AtomicBool,
    challenge_lock: tokio::sync::Mutex<()>,
    channel: tokio::sync::Mutex<Option<ChannelServe>>,
    protected: bool,
}

impl IdentityFixture {
    pub async fn start() -> Result<Self> {
        Self::start_with_policy(false).await
    }

    pub async fn start_protected() -> Result<Self> {
        Self::start_with_policy(true).await
    }

    async fn start_with_policy(protected: bool) -> Result<Self> {
        let home = TempDir::new().context("create C6 CODEX_HOME")?;
        let challenge_file = home.path().join("identity-challenges.jsonl");
        let shim = codex_utils_cargo_bin::cargo_bin("d174-daemon-shim")
            .context("locate C6 daemon shim")?;
        let app_server = codex_utils_cargo_bin::cargo_bin("codex-app-server")
            .context("locate real app-server binary")?;
        let mut command = Command::new(&shim);
        command
            .arg("daemon-start")
            .env("CODEX_HOME", home.path())
            .env("D174_APP_SERVER_BIN", &app_server)
            .env("D174_IDENTITY_CHALLENGE_FILE", &challenge_file);
        if protected {
            command.env("CODEX_APP_SERVER_IDENTITY_REQUIRED", "1");
        } else {
            command.env_remove("CODEX_APP_SERVER_IDENTITY_REQUIRED");
        }
        scrub_shared_identity_env(&mut command);
        let channel = if protected {
            Some(add_identity_channel(&mut command)?)
        } else {
            None
        };
        let output = command
            .output()
            .await
            .context("start real app-server through daemon lifecycle")?;
        if !output.status.success() {
            anyhow::bail!(
                "daemon start failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let socket = codex_app_server_transport::app_server_control_socket_path(home.path())?;
        for _ in 0..100 {
            if socket.as_path().exists() {
                return Ok(Self {
                    _home: home,
                    challenge_file,
                    shim,
                    app_server,
                    authority: IdentityAuthorityStub,
                    stopped: AtomicBool::new(false),
                    challenge_lock: tokio::sync::Mutex::new(()),
                    channel: tokio::sync::Mutex::new(channel),
                    protected,
                });
            }
            sleep(Duration::from_millis(20)).await;
        }
        anyhow::bail!("daemon did not create app-server socket")
    }

    pub fn socket_path(&self) -> Result<PathBuf> {
        Ok(
            codex_app_server_transport::app_server_control_socket_path(self._home.path())?
                .into_path_buf(),
        )
    }

    pub async fn restart(&self) -> Result<()> {
        self.stop().await?;
        let mut command = Command::new(&self.shim);
        command
            .arg("daemon-start")
            .env("CODEX_HOME", self._home.path())
            .env("D174_APP_SERVER_BIN", &self.app_server)
            .env("D174_IDENTITY_CHALLENGE_FILE", &self.challenge_file);
        if self.protected {
            command.env("CODEX_APP_SERVER_IDENTITY_REQUIRED", "1");
        } else {
            command.env_remove("CODEX_APP_SERVER_IDENTITY_REQUIRED");
        }
        scrub_shared_identity_env(&mut command);
        if self.protected {
            let serve = add_identity_channel(&mut command)?;
            *self.channel.lock().await = Some(serve);
        }
        let output = command.output().await?;
        if !output.status.success() {
            anyhow::bail!(
                "daemon restart failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        self.stopped.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let output = Command::new(&self.shim)
            .arg("daemon-stop")
            .env("CODEX_HOME", self._home.path())
            .env("D174_APP_SERVER_BIN", &self.app_server)
            .output()
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "daemon stop failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        self.stopped.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub async fn connect(
        &self,
        owner: &str,
        cell: &str,
        incarnation: &str,
    ) -> Result<HeadlessTuiClient> {
        let _challenge_guard = self.challenge_lock.lock().await;
        let mut client = HeadlessTuiClient::connect_unbound(
            self.socket_path()?,
            true,
            Some(&self.challenge_file),
        )
        .await?;
        let proof = self.authority.issue(
            client.challenge().context("connection challenge")?,
            owner,
            cell,
            incarnation,
            None,
        )?;
        client.bind(proof).await?;
        Ok(client)
    }

    pub async fn connect_unbound(&self) -> Result<HeadlessTuiClient> {
        HeadlessTuiClient::connect_unbound(self.socket_path()?, false, None).await
    }

    pub async fn connect_identity_unbound(&self) -> Result<HeadlessTuiClient> {
        let _challenge_guard = self.challenge_lock.lock().await;
        HeadlessTuiClient::connect_unbound(self.socket_path()?, true, Some(&self.challenge_file))
            .await
    }

    pub async fn connect_record(
        &self,
        owner: &str,
        cell: &str,
        incarnation: &str,
        cwd: Option<&str>,
    ) -> Result<(HeadlessTuiClient, IdentityProof)> {
        let _challenge_guard = self.challenge_lock.lock().await;
        let mut client = HeadlessTuiClient::connect_unbound(
            self.socket_path()?,
            true,
            Some(&self.challenge_file),
        )
        .await?;
        let proof = self.authority.issue(
            client.challenge().context("connection challenge")?,
            owner,
            cell,
            incarnation,
            cwd,
        )?;
        client.bind(proof.clone()).await?;
        Ok((client, proof))
    }

    pub async fn connect_with_proof(&self, proof: IdentityProof) -> Result<HeadlessTuiClient> {
        let _challenge_guard = self.challenge_lock.lock().await;
        let mut client = HeadlessTuiClient::connect_unbound(
            self.socket_path()?,
            true,
            Some(&self.challenge_file),
        )
        .await?;
        client.bind(proof).await?;
        Ok(client)
    }
}

impl Drop for IdentityFixture {
    fn drop(&mut self) {
        if !self.stopped.swap(true, Ordering::SeqCst) {
            let mut command = std::process::Command::new(&self.shim);
            command
                .arg("daemon-stop")
                .env("CODEX_HOME", self._home.path())
                .env("D174_APP_SERVER_BIN", &self.app_server);
            scrub_shared_identity_env_std(&mut command);
            let _ = command.output();
        }
    }
}

pub struct HeadlessTuiClient {
    stream: UnixWebSocket,
    next_id: i64,
    binding_id: Option<String>,
    challenge: Option<IdentityChallenge>,
}

impl HeadlessTuiClient {
    async fn connect_unbound(
        socket: PathBuf,
        identity: bool,
        challenge_file: Option<&Path>,
    ) -> Result<Self> {
        let known_challenges = challenge_file
            .map(read_challenge_fingerprints)
            .transpose()?
            .unwrap_or_default();
        let stream = UnixStream::connect(&socket)
            .await
            .with_context(|| format!("connect to app-server socket {}", socket.display()))?;
        let request = "ws://localhost/rpc"
            .into_client_request()
            .context("build UDS websocket handshake")?;
        let (mut stream, _) = client_async(request, stream)
            .await
            .context("upgrade app-server UDS to websocket")?;
        let extensions = identity.then(|| {
            std::collections::HashMap::from([(
                "nexuscrew.identity.v1".to_string(),
                Value::Object(serde_json::Map::new()),
            )])
        });
        let params = InitializeParams {
            client_info: ClientInfo {
                name: "codex-tui-c6-headless".to_string(),
                title: Some("C6 headless TUI".to_string()),
                version: "0.153.2".to_string(),
            },
            capabilities: Some(InitializeCapabilities {
                experimental_api: true,
                extensions,
                ..Default::default()
            }),
        };
        send_request(
            &mut stream,
            "initialize",
            1,
            Some(serde_json::to_value(params)?),
        )
        .await?;
        let response = read_response(&mut stream, 1).await?;
        if response.get("error").is_some() {
            anyhow::bail!("initialize failed: {response}");
        }
        send_notification(&mut stream, "initialized", None).await?;
        let challenge = challenge_file
            .map(|path| wait_for_new_challenge(path, &known_challenges))
            .transpose()?
            .flatten();
        if identity && challenge.is_none() {
            anyhow::bail!("identity challenge was not recorded for connection");
        }
        Ok(Self {
            stream,
            next_id: 2,
            binding_id: None,
            challenge,
        })
    }

    async fn bind(&mut self, proof: IdentityProof) -> Result<()> {
        let response = self.bind_raw(proof).await?;
        self.next_id += 1;
        if response.get("error").is_some() {
            anyhow::bail!("identity bind failed: {response}");
        }
        self.binding_id = response["binding"]["bindingId"]
            .as_str()
            .map(str::to_string);
        Ok(())
    }

    pub async fn bind_raw(&mut self, proof: IdentityProof) -> Result<Value> {
        send_request(
            &mut self.stream,
            "nexuscrew/identity/bind",
            self.next_id,
            Some(serde_json::to_value(
                codex_app_server_protocol::IdentityBindParams { proof },
            )?),
        )
        .await?;
        let response = read_response(&mut self.stream, self.next_id).await?;
        self.next_id += 1;
        Ok(response)
    }

    fn challenge(&self) -> Option<&IdentityChallenge> {
        self.challenge.as_ref()
    }

    pub fn binding_id(&self) -> Option<&str> {
        self.binding_id.as_deref()
    }

    pub async fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        send_request(&mut self.stream, method, id, params).await?;
        read_response(&mut self.stream, id).await
    }
}

async fn send_request(
    stream: &mut UnixWebSocket,
    method: &str,
    id: i64,
    params: Option<Value>,
) -> Result<()> {
    let message = JSONRPCMessage::Request(JSONRPCRequest {
        id: RequestId::Integer(id),
        method: method.to_string(),
        params,
        trace: None,
    });
    stream
        .send(Message::Text(serde_json::to_string(&message)?.into()))
        .await
        .context("send JSON-RPC request")?;
    Ok(())
}

async fn send_notification(
    stream: &mut UnixWebSocket,
    method: &str,
    params: Option<Value>,
) -> Result<()> {
    let notification = JSONRPCMessage::Notification(JSONRPCNotification {
        method: method.to_string(),
        params,
    });
    stream
        .send(Message::Text(serde_json::to_string(&notification)?.into()))
        .await
        .context("send JSON-RPC notification")?;
    Ok(())
}

async fn read_response(stream: &mut UnixWebSocket, id: i64) -> Result<Value> {
    loop {
        let message = stream
            .next()
            .await
            .context("app-server websocket closed")??;
        let Message::Text(text) = message else {
            continue;
        };
        let message: JSONRPCMessage = serde_json::from_str(&text)?;
        match message {
            JSONRPCMessage::Response(JSONRPCResponse {
                id: response_id,
                result,
            }) if response_id == RequestId::Integer(id) => return Ok(result),
            JSONRPCMessage::Error(error) if error.id == RequestId::Integer(id) => {
                return Ok(serde_json::json!({ "error": error.error }));
            }
            _ => {}
        }
    }
}

fn read_challenge_fingerprints(path: &Path) -> Result<std::collections::HashSet<String>> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("read identity challenge file {}", path.display()));
        }
    };
    contents
        .lines()
        .map(|line| {
            serde_json::from_str::<IdentityChallenge>(line).context("decode identity challenge")?;
            Ok(line.to_string())
        })
        .collect()
}

fn wait_for_new_challenge(
    path: &Path,
    known_challenges: &std::collections::HashSet<String>,
) -> Result<Option<IdentityChallenge>> {
    for _ in 0..200 {
        if let Ok(contents) = std::fs::read_to_string(path) {
            for line in contents.lines() {
                let challenge: IdentityChallenge =
                    serde_json::from_str(line).context("decode identity challenge")?;
                if !known_challenges.contains(line) {
                    return Ok(Some(challenge));
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Ok(None)
}

fn scrub_shared_identity_env_std(command: &mut std::process::Command) {
    command
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env_remove("NEXUSCREW_MCP_SESSION");
}
