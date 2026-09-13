use super::*;
use std::io::{BufRead, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::time::Instant;

fn fixture() -> (IdentityProofChannel, UnixStream) {
    let (channel, peer) = UnixStream::pair().unwrap();
    let output: OwnedFd = channel.try_clone().unwrap().into();
    let input: OwnedFd = channel.into();
    (
        IdentityProofChannel {
            inner: IdentityFdChannel::from_files(output.into(), input.into()).unwrap(),
        },
        peer,
    )
}

fn challenge() -> IdentityChallenge {
    IdentityChallenge {
        version: 1,
        connection_id: "connection".into(),
        daemon_boot_id: "boot".into(),
        audience: "daemon/connection".into(),
        nonce: "a".repeat(64),
        issued_at: 0,
        expires_at: 15_000,
    }
}

// This test is a guard, not a benchmark. The fake authority lives on an OS
// thread and may stay queued under the parallel harness; the channel timeout,
// instead, stays deliberately tight to exercise the failure branch.
const CLIENT_TEST_BUDGET: Duration = Duration::from_secs(5);
const CLIENT_TEST_OUTER_BUDGET: Duration = Duration::from_secs(15);
const CLIENT_TEST_EOF_BUDGET: Duration = Duration::from_secs(2);

async fn wait_for_peer_eof(peer: &mut UnixStream, budget: Duration) -> Result<(), String> {
    peer.set_nonblocking(true)
        .map_err(|error| format!("peer set_nonblocking failed: {error}"))?;
    let deadline = Instant::now() + budget;
    let mut bytes = [0u8; 4096];
    while Instant::now() < deadline {
        match peer.read(&mut bytes) {
            Ok(0) => return Ok(()),
            Ok(_) => continue,
            Err(error)
                if error.kind() == ErrorKind::WouldBlock
                    || error.kind() == ErrorKind::Interrupted =>
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            Err(error) => return Err(format!("peer read failed: {error}")),
        }
    }
    Err(format!(
        "peer EOF did not arrive within {budget:?}; fd still open"
    ))
}

#[tokio::test]
#[allow(clippy::print_stderr)]
async fn client_timeout_closes_fds_without_live_tasks() {
    // No counting of `/proc/self/task` — it is a process-GLOBAL counter, not
    // a property of this test, and under the parallel harness it is a false
    // red (exact equality). The local invariants are: the exchange finishes
    // within the budget (no blocked worker), the peer sees EOF (the fds are
    // closed), and the test runtime leaves no live tasks behind.
    let tasks_before = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();
    let (channel, mut peer) = fixture();
    let result = tokio::time::timeout(
        CLIENT_TEST_OUTER_BUDGET,
        channel.request_with_timeout(&challenge(), Duration::from_millis(50)),
    )
    .await
    .unwrap_or_else(|_| {
        panic!("lo scambio identity deve terminare entro {CLIENT_TEST_OUTER_BUDGET:?}")
    });
    drop(channel);
    peer.set_nonblocking(true).unwrap();
    let eof = wait_for_peer_eof(&mut peer, CLIENT_TEST_EOF_BUDGET).await;
    let tasks_after = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();
    eprintln!("client timeout: tasks {tasks_before}->{tasks_after}, peer EOF={eof:?}");
    // Release the legacy blocking read even on red, so the test runtime exits.
    let _ = peer.shutdown(std::net::Shutdown::Both);
    assert_eq!(result.unwrap_err().kind(), ErrorKind::TimedOut);
    eof.unwrap_or_else(|error| {
        panic!("timeout left the channel open; tasks {tasks_before}->{tasks_after}; {error}")
    });
    assert!(
        tasks_after <= tasks_before,
        "tasks leaked: {tasks_before}->{tasks_after}"
    );
}

#[tokio::test]
async fn client_channel_eof_is_rejected() {
    let (channel, peer) = fixture();
    peer.shutdown(std::net::Shutdown::Write).unwrap();
    let result = channel
        .request_with_timeout(&challenge(), Duration::from_millis(200))
        .await;
    assert_eq!(result.unwrap_err().kind(), ErrorKind::UnexpectedEof);
}

#[tokio::test]
async fn client_channel_malformed_json_is_rejected() {
    let (channel, mut peer) = fixture();
    peer.write_all(b"not-json\n").unwrap();
    let result = channel
        .request_with_timeout(&challenge(), Duration::from_millis(100))
        .await;
    let _ = peer.shutdown(std::net::Shutdown::Both);
    assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidData);
}

#[tokio::test]
async fn client_channel_missing_proof_is_rejected() {
    let (channel, mut peer) = fixture();
    peer.write_all(b"{\"id\":1,\"result\":{}}\n").unwrap();
    let result = channel
        .request_with_timeout(&challenge(), Duration::from_millis(200))
        .await;
    assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidData);
}

/// Fake authority on the client side: answers on the peer by echoing back
/// the `id` and counts the served requests.
fn spawn_fake_authority(
    peer: UnixStream,
    replies: Vec<serde_json::Value>,
) -> (
    std::thread::JoinHandle<usize>,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let served = std::sync::Arc::new(AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&served);
    let handle = std::thread::spawn(move || {
        let mut writer = peer;
        let mut reader = std::io::BufReader::new(writer.try_clone().expect("clone peer"));
        for reply in replies {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let request: serde_json::Value = serde_json::from_str(line.trim()).expect("request");
            let mut response = reply;
            response["id"] = request
                .get("id")
                .cloned()
                .unwrap_or(serde_json::Value::from(0));
            let mut bytes = serde_json::to_vec(&response).expect("response");
            bytes.push(b'\n');
            writer.write_all(&bytes).expect("write response");
            counter.fetch_add(1, Ordering::SeqCst);
        }
        counter.load(Ordering::SeqCst)
    });
    (handle, served)
}

fn valid_proof_json(challenge: &IdentityChallenge) -> serde_json::Value {
    serde_json::json!({
        "kind": "identity-proof",
        "ownerInstanceId": "owner-1",
        "cellId": "fixture-cell",
        "audience": challenge.audience,
        "incarnationId": "incarnation-1",
        "launchEpoch": "epoch-1",
        "daemonBootId": challenge.daemon_boot_id,
        "connectionId": challenge.connection_id,
        "challenge": challenge.nonce,
        "nonce": challenge.nonce,
        "parentJti": "b".repeat(64),
        "jti": "c".repeat(64),
        "issuedAt": challenge.issued_at,
        "expiresAt": challenge.expires_at,
        "authorityGeneration": "d".repeat(64),
        "generation": 3,
        "tmuxSession": "cloud-fixture-cell",
        "bindingId": "c".repeat(64),
        "scopes": ["thread/start"],
        "proof": "e".repeat(64),
    })
}

#[tokio::test]
async fn client_refusal_keeps_channel_alive_for_next_challenge() {
    // As on the daemon side, an authority refusal is a response and must
    // not invalidate the TUI channel: the next proof must go through (2
    // served requests) instead of failing on a dead channel.
    let (channel, peer) = fixture();
    let (handle, served) = spawn_fake_authority(
        peer,
        vec![
            serde_json::json!({"jsonrpc":"2.0","error":{"message":"refused"}}),
            serde_json::json!({"jsonrpc":"2.0","result":{"proof": valid_proof_json(&challenge())}}),
        ],
    );
    let refused = channel
        .request_with_timeout(&challenge(), CLIENT_TEST_BUDGET)
        .await;
    assert_eq!(refused.unwrap_err().kind(), ErrorKind::PermissionDenied);
    let accepted = channel
        .request_with_timeout(&challenge(), CLIENT_TEST_BUDGET)
        .await;
    assert!(
        accepted.is_ok(),
        "un proof valido dopo un rifiuto deve arrivare all'authority: {accepted:?}"
    );
    assert_eq!(
        served.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "verifyCalls: il secondo challengeProof non ha raggiunto l'authority"
    );
    handle.join().expect("fake authority thread");
}
