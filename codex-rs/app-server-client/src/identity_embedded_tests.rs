//! The embedded app-server reuses the channel this process already captured.
//!
//! The TUI captures the inherited identity descriptors once, at startup, and that
//! capture removes the declaration from the environment. The embedded app-server
//! runs in the same process, so its startup gate must accept the channel the
//! process already owns instead of re-reading an environment the capture emptied.
//! Each child test runs that exact sequence with real descriptors on 3 and 4.
//!
//! The same harness also drives the handshake the embedded client owes the
//! server: the initialize response carries the daemon challenge, the
//! client asks the authority for a proof on the channel this process captured,
//! and the bind must complete before the first protected request. The parent
//! side of the channel plays an adaptive authority, so a child can be run
//! against an approving, a refusing or a silent authority.

use super::*;
use codex_app_server_protocol::GetAccountParams;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

const CHILD_ENV: &str = "IDENTITY_EMBEDDED_CHILD";
const FD_ENV: &str = "NEXUSCREW_IDENTITY_FD";
const PAIR_ENV: &str = "IDENTITY_TEST_FD_PAIR";
const REQUIRED_ENV: &str = "CODEX_APP_SERVER_IDENTITY_REQUIRED";
const SESSION_ENV: &str = "NEXUSCREW_MCP_SESSION";
const HANDSHAKE_MODE_ENV: &str = "IDENTITY_EMBEDDED_HANDSHAKE_MODE";

/// Runs one child test with a clean launch environment and, when requested, a real
/// descriptor pair for 3 (write) and 4 (read).
///
/// The pair is inherited by number — its close-on-exec flag is cleared here — and the
/// child places it on the conventional positions itself. Remapping inside the child keeps
/// the hand-off independent of the descriptor table of the test runner, whose own pipes
/// are allocated during the spawn and can occupy the same numbers.
fn run_child(
    name: &str,
    environment: &[(&str, &str)],
    descriptors: Option<(i32, i32)>,
) -> std::process::Output {
    let mut command = Command::new(std::env::current_exe().expect("child test binary"));
    command
        .args(["--exact", name, "--nocapture"])
        .env(CHILD_ENV, "1")
        .env_remove(PAIR_ENV)
        .env_remove(FD_ENV)
        .env_remove(REQUIRED_ENV)
        .env_remove(SESSION_ENV);
    if let Some((write, read)) = descriptors {
        clear_close_on_exec(write);
        clear_close_on_exec(read);
        command.env(PAIR_ENV, format!("{write}:{read}"));
    }
    for (key, value) in environment {
        command.env(key, value);
    }
    command.output().expect("child test process")
}

/// The child receives the launcher's descriptors by number and places them itself.
///
/// This must run before the child creates any I/O driver: the runtime owns the
/// lowest free descriptor as soon as it exists, so remapping onto 3 and 4
/// afterwards would close the driver's own descriptor and every later poll would
/// fail with `EINVAL`. Child tests therefore place the descriptors first and only
/// then build the runtime they run on.
fn place_identity_fds_if_requested() {
    let Some(pair) = std::env::var_os(PAIR_ENV) else {
        return;
    };
    let pair = pair.to_str().expect("descriptor pair is text");
    let (write, read) = pair.split_once(':').expect("descriptor pair");
    let write = write.parse::<i32>().expect("write descriptor");
    let read = read.parse::<i32>().expect("read descriptor");
    for (from, to) in [(write, 3), (read, 4)] {
        if unsafe { libc::dup2(from, to) } < 0 {
            panic!(
                "cannot place descriptor {from} on {to}: {}",
                std::io::Error::last_os_error()
            );
        }
    }
}

/// Builds the child runtime after the inherited descriptors are in place.
fn child_runtime() -> tokio::runtime::Runtime {
    place_identity_fds_if_requested();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("child runtime")
}

fn clear_close_on_exec(fd: i32) {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    assert!(
        flags >= 0,
        "F_GETFD on {fd}: {}",
        std::io::Error::last_os_error()
    );
    assert!(
        unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } >= 0,
        "clear close-on-exec on {fd}: {}",
        std::io::Error::last_os_error()
    );
}

fn child_output_text(output: &std::process::Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

async fn build_embedded_test_config(codex_home: &std::path::Path) -> codex_core::config::Config {
    match codex_core::config::ConfigBuilder::default()
        .codex_home(codex_home.to_path_buf())
        .build()
        .await
    {
        Ok(config) => config,
        Err(_) => codex_core::config::Config::load_default_with_cli_overrides_for_codex_home(
            codex_home.to_path_buf(),
            Vec::new(),
        )
        .await
        .expect("default config should load"),
    }
}

/// Starts the embedded app-server exactly like the TUI does, with the channel the
/// process captured (if any) handed to the startup gate.
async fn start_embedded_test_client(
    identity_channel: Option<IdentityFdChannel>,
) -> std::io::Result<(tempfile::TempDir, crate::InProcessAppServerClient)> {
    let codex_home = tempfile::TempDir::new().expect("temporary codex home");
    let config = Arc::new(build_embedded_test_config(codex_home.path()).await);
    let state_db = codex_core::init_state_db(config.as_ref()).await;
    let client = crate::InProcessAppServerClient::start(crate::InProcessClientStartArgs {
        arg0_paths: codex_arg0::Arg0DispatchPaths::default(),
        config,
        cli_overrides: Vec::new(),
        loader_overrides: codex_config::LoaderOverrides::default(),
        strict_config: false,
        cloud_config_bundle: codex_config::CloudConfigBundleLoader::default(),
        feedback: codex_feedback::CodexFeedback::new(),
        log_db: None,
        state_db,
        environment_manager: Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
        config_warnings: Vec::new(),
        session_source: codex_protocol::protocol::SessionSource::Exec,
        enable_codex_api_key_env: false,
        client_name: "codex-app-server-client-embedded-identity-test".to_string(),
        client_version: "0.0.0-test".to_string(),
        experimental_api: true,
        mcp_server_openai_form_elicitation: false,
        opt_out_notification_methods: Vec::new(),
        identity_channel,
        channel_capacity: crate::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
    })
    .await?;
    Ok((codex_home, client))
}

#[test]
fn embedded_start_child_reuses_prepared_channel() {
    child_runtime().block_on(async {
        if std::env::var_os(CHILD_ENV).is_none() {
            return;
        }
        assert_eq!(
            std::env::var(FD_ENV).expect("declared identity channel"),
            "3:4"
        );
        assert!(prepare_nexuscrew_identity_channel().expect("prepare identity channel"));
        assert!(
            std::env::var_os(FD_ENV).is_none(),
            "the capture removes the declaration from the environment"
        );
        let (codex_home, client) = start_embedded_test_client(prepared_identity_channel())
            .await
            .expect("embedded startup must accept the channel this process captured");
        assert!(
            prepared_identity_channel().is_some(),
            "the capture stays owned by the process"
        );
        client.shutdown().await.expect("shutdown");
        drop(codex_home);
    });
}

#[test]
fn embedded_start_child_refuses_without_declaration() {
    child_runtime().block_on(async {
        if std::env::var_os(CHILD_ENV).is_none() {
            return;
        }
        assert!(std::env::var_os(FD_ENV).is_none());
        assert!(!prepare_nexuscrew_identity_channel().expect("prepare identity channel"));
        match start_embedded_test_client(prepared_identity_channel()).await {
            Ok((codex_home, client)) => {
                let _ = client.shutdown().await;
                drop(codex_home);
                panic!("identity required without a channel must fail closed");
            }
            Err(error) => {
                let message = error.to_string();
                assert!(message.contains("IDENTITY_CHANNEL_BROKEN"), "{message}");
                assert!(message.contains("verifier channel is absent"), "{message}");
            }
        }
    });
}

#[test]
fn embedded_start_child_standalone_keeps_the_declaration() {
    child_runtime().block_on(async {
        if std::env::var_os(CHILD_ENV).is_none() {
            return;
        }
        assert!(
            std::env::var_os(FD_ENV).is_some(),
            "a standalone start must not consume the declaration"
        );
        let (codex_home, client) = start_embedded_test_client(/*identity_channel*/ None)
            .await
            .expect("standalone embedded startup");
        client.shutdown().await.expect("shutdown");
        drop(codex_home);
    });
}

/// The channel is inherited and the capture is shared: embedded startup must
/// accept it, and the handshake the server demands travels on that same channel.
/// The parent plays the authority here, so the child only reports ready after the
/// proof and the server-side verify crossed the pair it was given.
#[test]
fn embedded_start_reuses_the_channel_captured_at_startup() {
    let (cell_write, relay_read) = UnixStream::pair().expect("identity write channel");
    let (relay_write, cell_read) = UnixStream::pair().expect("identity read channel");
    let mut authority = spawn_adaptive_authority(relay_read, relay_write, AuthorityMode::Approve);
    let output = run_child(
        "remote::identity_embedded_tests::embedded_start_child_reuses_prepared_channel",
        &[(FD_ENV, "3:4"), (REQUIRED_ENV, "1")],
        Some((cell_write.as_raw_fd(), cell_read.as_raw_fd())),
    );
    drop((cell_write, cell_read));
    authority.join();
    assert!(output.status.success(), "{}", child_output_text(&output));
    assert_eq!(
        authority.methods(),
        vec![
            "nexuscrew/identity/challengeProof".to_string(),
            "nexuscrew/identity/verify".to_string()
        ],
        "embedded startup must complete the handshake on the captured channel"
    );
}

#[test]
fn embedded_start_refuses_without_declaration() {
    let output = run_child(
        "remote::identity_embedded_tests::embedded_start_child_refuses_without_declaration",
        &[(REQUIRED_ENV, "1")],
        /*descriptors*/ None,
    );
    assert!(output.status.success(), "{}", child_output_text(&output));
}

/// Explicit standalone: the captured-channel path must be suppressed at the
/// source. The child proves the declaration is not consumed and the connection
/// is ready with zero identity messages; the parent proves the authority saw
/// nothing at all on the channel.
#[test]
fn embedded_start_child_explicit_standalone_never_handshakes() {
    child_runtime().block_on(async {
        if std::env::var_os(CHILD_ENV).is_none() {
            return;
        }
        assert_eq!(
            std::env::var(FD_ENV).expect("declared identity channel"),
            "3:4"
        );
        assert!(
            !prepare_nexuscrew_identity_channel().expect("prepare identity channel"),
            "an explicit standalone declaration must not be captured"
        );
        assert!(
            std::env::var_os(FD_ENV).is_some(),
            "an explicit standalone declaration must not be consumed"
        );
        let (codex_home, client) = start_embedded_test_client(/*identity_channel*/ None)
            .await
            .expect("standalone embedded startup");
        // The initialize handshake ran inside startup, so a handed-out client on
        // a standalone connection is ready by construction; the authority side
        // of this test proves zero identity messages crossed the channel.
        client.shutdown().await.expect("shutdown");
        drop(codex_home);
    });
}

#[test]
fn embedded_start_explicit_standalone_never_handshakes() {
    let (cell_write, relay_read) = UnixStream::pair().expect("identity write channel");
    let (relay_write, cell_read) = UnixStream::pair().expect("identity read channel");
    let mut authority = spawn_adaptive_authority(relay_read, relay_write, AuthorityMode::Approve);
    let output = run_child(
        "remote::identity_embedded_tests::embedded_start_child_explicit_standalone_never_handshakes",
        &[(FD_ENV, "3:4"), (REQUIRED_ENV, "0")],
        /*descriptors*/ None,
    );
    drop((cell_write, cell_read));
    authority.join();
    assert!(output.status.success(), "{}", child_output_text(&output));
    assert_eq!(
        authority.served(),
        0,
        "the authority must see zero identity messages on a standalone connection"
    );
}

#[test]
fn embedded_start_standalone_keeps_the_declaration() {
    let (write, read) = UnixStream::pair().expect("identity socket pair");
    let output = run_child(
        "remote::identity_embedded_tests::embedded_start_child_standalone_keeps_the_declaration",
        &[(FD_ENV, "3:4"), (REQUIRED_ENV, "0")],
        Some((write.as_raw_fd(), read.as_raw_fd())),
    );
    assert!(output.status.success(), "{}", child_output_text(&output));
}

/// A descriptor that is not a pipe or a socket cannot carry the identity channel.
/// The capture refuses the declaration, and the enforcing startup refuses to run
/// without a usable verifier instead of accepting a regular file.
#[test]
fn embedded_start_child_rejects_a_regular_file_channel() {
    child_runtime().block_on(async {
        if std::env::var_os(CHILD_ENV).is_none() {
            return;
        }
        assert_eq!(
            std::env::var(FD_ENV).expect("declared identity channel"),
            "3:4"
        );
        assert!(
            !prepare_nexuscrew_identity_channel().expect("prepare identity channel"),
            "a descriptor that is not a pipe or a socket is not a usable capture"
        );
        match start_embedded_test_client(prepared_identity_channel()).await {
            Ok((codex_home, client)) => {
                let _ = client.shutdown().await;
                drop(codex_home);
                panic!("a regular file must not carry the identity channel");
            }
            Err(error) => {
                let message = error.to_string();
                assert!(message.contains("IDENTITY_CHANNEL_BROKEN"), "{message}");
                assert!(
                    message.contains("descriptor is not a pipe or socket"),
                    "{message}"
                );
            }
        }
    });
}

#[test]
fn embedded_start_rejects_a_regular_file_channel() {
    let regular = tempfile::NamedTempFile::new().expect("regular file");
    let (write, read) = UnixStream::pair().expect("identity socket pair");
    let output = run_child(
        "remote::identity_embedded_tests::embedded_start_child_rejects_a_regular_file_channel",
        &[(FD_ENV, "3:4"), (REQUIRED_ENV, "1")],
        Some((regular.as_file().as_raw_fd(), read.as_raw_fd())),
    );
    assert!(output.status.success(), "{}", child_output_text(&output));
    drop((regular, write, read));
}

// ---------------------------------------------------------------------------
// Handshake: the initialize challenge must be answered before the first
// protected request, on the channel this process captured.
//
// The launcher keeps the relay side of the channel and hands the cell two
// one-way descriptors (3 write, 4 read), so the harness models it with two
// socket pairs: the child places the cell ends on 3 and 4, and the parent plays
// the authority on the relay ends.
// ---------------------------------------------------------------------------

const HANDSHAKE_CHALLENGE_METHOD: &str = "nexuscrew/identity/challengeProof";
const HANDSHAKE_VERIFY_METHOD: &str = "nexuscrew/identity/verify";
const FIXTURE_BINDING_ID: &str = "fixture-binding";
/// How long a silent authority stays alive without answering: the client's own
/// proof deadline is shorter, so the child must have failed closed by then.
const SILENT_AUTHORITY_HOLD: Duration = Duration::from_secs(6);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AuthorityMode {
    /// Answers the proof and confirms the verify: the handshake can complete.
    Approve,
    /// Refuses the proof request itself.
    RefuseProof,
    /// Returns a well-formed proof but refuses the server-side verify.
    RefuseVerify,
    /// Reads the proof request and never answers.
    Silent,
}

struct AdaptiveAuthority {
    served: Arc<AtomicUsize>,
    methods: Arc<Mutex<Vec<String>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl AdaptiveAuthority {
    fn served(&self) -> usize {
        self.served.load(Ordering::SeqCst)
    }

    fn methods(&self) -> Vec<String> {
        self.methods.lock().expect("authority methods").clone()
    }

    /// Waits for the authority thread to see the end of the conversation. Call it
    /// only after the cell side of the channel is closed, or it waits forever.
    fn join(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.join().expect("adaptive authority thread");
        }
    }
}

fn child_authority_mode() -> AuthorityMode {
    match std::env::var(HANDSHAKE_MODE_ENV)
        .expect("handshake mode")
        .as_str()
    {
        "approve" => AuthorityMode::Approve,
        "refuse-proof" => AuthorityMode::RefuseProof,
        "refuse-verify" => AuthorityMode::RefuseVerify,
        "silent" => AuthorityMode::Silent,
        other => panic!("unknown handshake mode {other}"),
    }
}

fn jsonrpc_result(id: serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn jsonrpc_error(id: serde_json::Value, message: &str) -> serde_json::Value {
    serde_json::json!({"jsonrpc": "2.0", "id": id, "error": {"message": message}})
}

/// The proof the authority hands the client: every challenge field the daemon
/// compares is echoed back, the rest are fixtures.
fn authority_proof_json(challenge: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "kind": "identity-proof",
        "ownerInstanceId": "fixture-owner",
        "cellId": "fixture-cell",
        "audience": challenge["audience"],
        "incarnationId": "fixture-incarnation",
        "launchEpoch": "fixture-epoch",
        "daemonBootId": challenge["daemonBootId"],
        "connectionId": challenge["connectionId"],
        "challenge": challenge["nonce"],
        "nonce": challenge["nonce"],
        "jti": "f".repeat(64),
        "issuedAt": challenge["issuedAt"],
        "expiresAt": challenge["expiresAt"],
        "authorityGeneration": "e".repeat(64),
        "generation": 1,
        "tmuxSession": "fixture-session",
        "bindingId": FIXTURE_BINDING_ID,
        "scopes": ["thread/start"],
        "proof": "d".repeat(64),
    })
}

/// The normalized claims the verify answers with: they must agree field by field
/// with the proof the client bound, because the bind builds its binding from them.
fn authority_claims_json(challenge: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "ownerInstanceId": "fixture-owner",
        "issuerOwner": "fixture-owner",
        "cellId": "fixture-cell",
        "audience": challenge["audience"],
        "incarnationId": "fixture-incarnation",
        "launchEpoch": "fixture-epoch",
        "daemonBootId": challenge["daemonBootId"],
        "connectionId": challenge["connectionId"],
        "tmuxSession": "fixture-session",
        "bindingId": FIXTURE_BINDING_ID,
        "scopes": ["thread/start"],
        "origin": "local_tui",
        "issuedAt": challenge["issuedAt"],
        "notBefore": challenge["issuedAt"],
        "expiresAt": challenge["expiresAt"],
        "nonce": challenge["nonce"],
        "generation": 1,
    })
}

/// Serves one JSON-RPC line per request on the relay side of the channel and
/// dispatches on the method, so the same fake answers both the client's proof
/// request and the embedded server's verify.
fn spawn_adaptive_authority(
    reader: UnixStream,
    writer: UnixStream,
    mode: AuthorityMode,
) -> AdaptiveAuthority {
    let served = Arc::new(AtomicUsize::new(0));
    let methods = Arc::new(Mutex::new(Vec::new()));
    let served_thread = Arc::clone(&served);
    let methods_thread = Arc::clone(&methods);
    let handle = std::thread::spawn(move || {
        let mut writer = writer;
        let mut reader = BufReader::new(reader);
        let mut challenge: Option<serde_json::Value> = None;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let Ok(request) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                continue;
            };
            let id = request
                .get("id")
                .cloned()
                .unwrap_or(serde_json::Value::from(0));
            let method = request
                .get("method")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            if let Ok(mut methods) = methods_thread.lock() {
                methods.push(method.clone());
            }
            if method == HANDSHAKE_CHALLENGE_METHOD {
                challenge = request
                    .get("params")
                    .and_then(|params| params.get("challenge"))
                    .cloned();
            }
            if mode == AuthorityMode::Silent {
                // The channel stays open and silent: the client has to time out on
                // its own deadline, and the peer must not disappear before it does.
                std::thread::sleep(SILENT_AUTHORITY_HOLD);
                break;
            }
            let response = match method.as_str() {
                HANDSHAKE_CHALLENGE_METHOD => match (mode, challenge.as_ref()) {
                    (_, None) => jsonrpc_error(id, "missing-challenge"),
                    (AuthorityMode::RefuseProof, _) => jsonrpc_error(id, "bad-proof"),
                    (_, Some(challenge)) => jsonrpc_result(
                        id,
                        serde_json::json!({"proof": authority_proof_json(challenge)}),
                    ),
                },
                HANDSHAKE_VERIFY_METHOD => match (mode, challenge.as_ref()) {
                    (_, None) => jsonrpc_error(id, "missing-challenge"),
                    (AuthorityMode::RefuseVerify, _) => {
                        jsonrpc_result(id, serde_json::json!({"ok": false, "reason": "bad-proof"}))
                    }
                    (_, Some(challenge)) => jsonrpc_result(
                        id,
                        serde_json::json!({"ok": true, "claims": authority_claims_json(challenge)}),
                    ),
                },
                _ => jsonrpc_error(id, "unsupported"),
            };
            let mut bytes = match serde_json::to_vec(&response) {
                Ok(bytes) => bytes,
                Err(_) => break,
            };
            bytes.push(b'\n');
            if writer.write_all(&bytes).is_err() {
                break;
            }
            served_thread.fetch_add(1, Ordering::SeqCst);
        }
    });
    AdaptiveAuthority {
        served,
        methods,
        handle: Some(handle),
    }
}

/// Runs one child against an approving authority: startup must complete the
/// handshake, and the first protected request must then be served.
#[test]
fn embedded_handshake_child_serves_protected_requests() {
    child_runtime().block_on(async {
        if std::env::var_os(CHILD_ENV).is_none() {
            return;
        }
        assert_eq!(child_authority_mode(), AuthorityMode::Approve);
        assert!(prepare_nexuscrew_identity_channel().expect("prepare identity channel"));
        match start_embedded_test_client(prepared_identity_channel()).await {
            Ok((codex_home, client)) => {
                let response = client
                    .request(ClientRequest::GetAccount {
                        request_id: RequestId::Integer(7),
                        params: GetAccountParams {
                            refresh_token: false,
                        },
                    })
                    .await;
                let outcome = match response {
                    Ok(Ok(_)) => "ok".to_string(),
                    Ok(Err(error)) => format!("error:{}:{}", error.code, error.message),
                    Err(error) => format!("transport:{}:{error}", error.kind()),
                };
                println!("EMBEDDED_ACCOUNT_READ={outcome}");
                let _ = client.shutdown().await;
                drop(codex_home);
                assert_eq!(
                    outcome, "ok",
                    "the first protected request must be served after the identity handshake"
                );
            }
            Err(error) => panic!("embedded startup must complete the identity handshake: {error}"),
        }
    });
}

/// Runs one child against an authority that refuses the proof, refuses the
/// verify or never answers: startup must fail closed with the reason that
/// describes the fault, and never hand out a half-ready client.
#[test]
fn embedded_handshake_child_fails_closed() {
    child_runtime().block_on(async {
        if std::env::var_os(CHILD_ENV).is_none() {
            return;
        }
        let mode = child_authority_mode();
        assert!(prepare_nexuscrew_identity_channel().expect("prepare identity channel"));
        match start_embedded_test_client(prepared_identity_channel()).await {
            Ok((codex_home, client)) => {
                let _ = client.shutdown().await;
                drop(codex_home);
                panic!("an unanswered identity handshake must not produce a client ({mode:?})");
            }
            Err(error) => {
                let message = error.to_string();
                println!("EMBEDDED_HANDSHAKE=failed:{:?}:{message}", error.kind());
                assert!(message.contains("IDENTITY_HANDSHAKE_FAILED"), "{message}");
                match mode {
                    AuthorityMode::RefuseProof => {
                        assert!(message.contains("identity proof unavailable"), "{message}");
                        assert!(message.contains("bad-proof"), "{message}");
                        assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{message}");
                    }
                    AuthorityMode::RefuseVerify => {
                        assert!(message.contains("identity bind rejected"), "{message}");
                        assert!(message.contains("IdentityUnverified"), "{message}");
                    }
                    AuthorityMode::Silent => {
                        assert!(message.contains("identity proof unavailable"), "{message}");
                        assert_eq!(error.kind(), ErrorKind::TimedOut, "{message}");
                    }
                    AuthorityMode::Approve => panic!("approve mode must not fail closed"),
                }
            }
        }
    });
}

#[test]
fn embedded_handshake_serves_the_first_protected_request() {
    let (cell_write, relay_read) = UnixStream::pair().expect("identity write channel");
    let (relay_write, cell_read) = UnixStream::pair().expect("identity read channel");
    let mut authority = spawn_adaptive_authority(relay_read, relay_write, AuthorityMode::Approve);
    let output = run_child(
        "remote::identity_embedded_tests::embedded_handshake_child_serves_protected_requests",
        &[
            (FD_ENV, "3:4"),
            (REQUIRED_ENV, "1"),
            (HANDSHAKE_MODE_ENV, "approve"),
        ],
        Some((cell_write.as_raw_fd(), cell_read.as_raw_fd())),
    );
    drop((cell_write, cell_read));
    authority.join();
    assert!(output.status.success(), "{}", child_output_text(&output));
    assert_eq!(
        authority.served(),
        2,
        "proof and verify must both be exchanged"
    );
    assert_eq!(
        authority.methods(),
        vec![
            HANDSHAKE_CHALLENGE_METHOD.to_string(),
            HANDSHAKE_VERIFY_METHOD.to_string()
        ]
    );
}

#[test]
fn embedded_handshake_fails_closed_when_the_proof_is_refused() {
    let (cell_write, relay_read) = UnixStream::pair().expect("identity write channel");
    let (relay_write, cell_read) = UnixStream::pair().expect("identity read channel");
    let mut authority =
        spawn_adaptive_authority(relay_read, relay_write, AuthorityMode::RefuseProof);
    let output = run_child(
        "remote::identity_embedded_tests::embedded_handshake_child_fails_closed",
        &[
            (FD_ENV, "3:4"),
            (REQUIRED_ENV, "1"),
            (HANDSHAKE_MODE_ENV, "refuse-proof"),
        ],
        Some((cell_write.as_raw_fd(), cell_read.as_raw_fd())),
    );
    drop((cell_write, cell_read));
    authority.join();
    assert!(output.status.success(), "{}", child_output_text(&output));
    assert_eq!(
        authority.methods(),
        vec![HANDSHAKE_CHALLENGE_METHOD.to_string()],
        "a refused proof must stop the handshake before any bind"
    );
}

#[test]
fn embedded_handshake_fails_closed_when_the_verify_is_refused() {
    let (cell_write, relay_read) = UnixStream::pair().expect("identity write channel");
    let (relay_write, cell_read) = UnixStream::pair().expect("identity read channel");
    let mut authority =
        spawn_adaptive_authority(relay_read, relay_write, AuthorityMode::RefuseVerify);
    let output = run_child(
        "remote::identity_embedded_tests::embedded_handshake_child_fails_closed",
        &[
            (FD_ENV, "3:4"),
            (REQUIRED_ENV, "1"),
            (HANDSHAKE_MODE_ENV, "refuse-verify"),
        ],
        Some((cell_write.as_raw_fd(), cell_read.as_raw_fd())),
    );
    drop((cell_write, cell_read));
    authority.join();
    assert!(output.status.success(), "{}", child_output_text(&output));
    assert_eq!(
        authority.methods(),
        vec![
            HANDSHAKE_CHALLENGE_METHOD.to_string(),
            HANDSHAKE_VERIFY_METHOD.to_string()
        ],
        "the server must verify the proof before the bind is answered"
    );
}

#[test]
fn embedded_handshake_fails_closed_on_a_silent_authority() {
    let (cell_write, relay_read) = UnixStream::pair().expect("identity write channel");
    let (relay_write, cell_read) = UnixStream::pair().expect("identity read channel");
    let mut authority = spawn_adaptive_authority(relay_read, relay_write, AuthorityMode::Silent);
    let output = run_child(
        "remote::identity_embedded_tests::embedded_handshake_child_fails_closed",
        &[
            (FD_ENV, "3:4"),
            (REQUIRED_ENV, "1"),
            (HANDSHAKE_MODE_ENV, "silent"),
        ],
        Some((cell_write.as_raw_fd(), cell_read.as_raw_fd())),
    );
    drop((cell_write, cell_read));
    authority.join();
    assert!(output.status.success(), "{}", child_output_text(&output));
    assert_eq!(authority.served(), 0, "a silent authority answers nothing");
    assert_eq!(
        authority.methods(),
        vec![HANDSHAKE_CHALLENGE_METHOD.to_string()]
    );
}
