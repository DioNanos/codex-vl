use super::*;
use codex_app_server_protocol::{IdentityClaims, IdentityKind, IdentityOrigin};
use std::io::{BufRead, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;

fn fixture() -> (FdAuthorityVerifier, UnixStream, IdentityProof) {
    let (channel, peer) = UnixStream::pair().unwrap();
    let output: OwnedFd = channel.try_clone().unwrap().into();
    let input: OwnedFd = channel.into();
    let challenge = IdentityChallenge {
        version: 1,
        connection_id: "connection".into(),
        daemon_boot_id: "boot".into(),
        audience: "daemon/connection".into(),
        nonce: "a".repeat(64),
        issued_at: 0,
        expires_at: 15_000,
    };
    let proof = IdentityProof {
        version: "1".into(),
        kind: IdentityKind::ConnectionV1,
        challenge: challenge.clone(),
        claims: IdentityClaims {
            issuer_owner: "owner".into(),
            audience: challenge.audience.clone(),
            owner_instance_id: "owner".into(),
            cell_id: "cell".into(),
            tmux_session: "session".into(),
            incarnation_id: "incarnation".into(),
            launch_epoch: "epoch".into(),
            daemon_boot_id: challenge.daemon_boot_id.clone(),
            connection_id: challenge.connection_id.clone(),
            binding_id: "binding".into(),
            origin: IdentityOrigin::LocalTui,
            scopes: vec!["thread/start".into()],
            issued_at: 0,
            not_before: 0,
            expires_at: 15_000,
            nonce: challenge.nonce.clone(),
            thread_id: None,
            cwd: None,
            live_host: None,
        },
        proof: json!({"generation":1,"proof":{}}).to_string(),
    };
    (
        FdAuthorityVerifier {
            inner: IdentityFdChannel::from_files(output.into(), input.into()).unwrap(),
        },
        peer,
        proof,
    )
}

/// Fake authority per i test N1: risponde sul peer, una riga JSON-RPC per
/// richiesta, rimandando indietro l'`id` ricevuto, e conta quante richieste ha
/// servito (`verifyCalls`). Le risposte sono i `result`/`error` da inviare,
/// in ordine.
fn spawn_fake_authority(
    peer: UnixStream,
    replies: Vec<Value>,
) -> (
    std::thread::JoinHandle<usize>,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let served = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&served);
    let handle = std::thread::spawn(move || {
        let mut writer = peer;
        let mut reader = std::io::BufReader::new(writer.try_clone().expect("clone peer"));
        for reply in replies {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let request: Value = serde_json::from_str(line.trim()).expect("request JSON");
            let mut response = reply;
            response["id"] = request.get("id").cloned().unwrap_or(Value::from(0));
            let mut bytes = serde_json::to_vec(&response).expect("response JSON");
            bytes.push(b'\n');
            writer.write_all(&bytes).expect("write response");
            counter.fetch_add(1, Ordering::SeqCst);
        }
        counter.load(Ordering::SeqCst)
    });
    (handle, served)
}

/// Well-formed v1.1 claims coherent with the challenge: the `ok:true`
/// response the daemon accepts.
fn verified_claims_json(challenge: &IdentityChallenge) -> Value {
    json!({
        "ownerInstanceId": "owner",
        "issuerOwner": "owner",
        "cellId": "cell",
        "audience": challenge.audience,
        "incarnationId": "incarnation",
        "launchEpoch": "epoch",
        "daemonBootId": challenge.daemon_boot_id,
        "connectionId": challenge.connection_id,
        "tmuxSession": "session",
        "bindingId": "binding",
        "scopes": ["thread/start"],
        "origin": "local_tui",
        "issuedAt": challenge.issued_at,
        "notBefore": challenge.issued_at,
        "expiresAt": challenge.expires_at,
        "nonce": challenge.nonce,
        "generation": 1,
    })
}

#[tokio::test]
async fn authority_refusal_keeps_channel_alive_for_next_proof() {
    // A semantic refusal is a RESPONSE. The channel must stay alive and the
    // next valid proof must reach the authority (verifyCalls=2).
    let (verifier, peer, proof) = fixture();
    let (handle, served) = spawn_fake_authority(
        peer,
        vec![
            json!({"jsonrpc":"2.0","result":{"ok":false,"reason":"bad-proof"}}),
            json!({"jsonrpc":"2.0","result":{"ok":true,"claims":verified_claims_json(&proof.challenge)}}),
        ],
    );
    let refused = verifier.verify(&proof, &proof.challenge).await;
    assert_eq!(
        refused.unwrap_err(),
        AuthorityRefusal::Rejected("bad-proof")
    );
    let accepted = verifier.verify(&proof, &proof.challenge).await;
    assert!(
        accepted.is_ok(),
        "un proof valido dopo un rifiuto deve essere verificato: {accepted:?}"
    );
    assert_eq!(accepted.unwrap().nonce, proof.challenge.nonce);
    assert_eq!(
        served.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "verifyCalls: il secondo bind non ha raggiunto l'authority"
    );
    handle.join().expect("fake authority thread");
}

#[tokio::test]
#[allow(clippy::print_stderr)]
async fn authority_timeout_closes_channel_without_live_tasks() {
    // The `/proc/self/task` count used here is a process-GLOBAL counter
    // while the harness runs hundreds of tests in parallel (`threads
    // leaked: 7->66`): it is not a property of this test. The local,
    // verifiable invariants are three: (a) the exchange finishes within
    // the budget (no
    // worker bloccato sull'I/O), (b) il peer vede EOF — i fd sono chiusi,
    // (c) la runtime del test non lascia task vivi in piu'.
    let tasks = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();
    let (verifier, mut peer, proof) = fixture();
    let outcome = tokio::time::timeout(
        Duration::from_secs(5),
        verifier.verify(&proof, &proof.challenge),
    )
    .await
    .expect("lo scambio identity deve terminare entro il budget");
    assert!(matches!(outcome, Err(AuthorityRefusal::Unreachable)));
    peer.set_nonblocking(true).unwrap();
    let mut bytes = [0; 4096];
    loop {
        match peer.read(&mut bytes) {
            Ok(0) => break,
            Ok(_) => continue,
            Err(e) => panic!("channel remained open: {e}"),
        }
    }
    let after_tasks = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();
    eprintln!("server timeout: tasks {tasks}->{after_tasks}, peer EOF=true");
    assert!(after_tasks <= tasks, "tasks leaked: {tasks}->{after_tasks}");
}

#[tokio::test]
async fn authority_eof_is_rejected() {
    let (verifier, peer, proof) = fixture();
    peer.shutdown(std::net::Shutdown::Write).unwrap();
    assert!(matches!(
        verifier.verify(&proof, &proof.challenge).await,
        Err(AuthorityRefusal::Unreachable)
    ));
    // EOF is a transport failure: the channel stays invalidated, and the
    // next request fails immediately with the close message (not with the
    // peer's EPIPE). This discriminates for real: if `invalidate()` had not
    // been called, the error would be the system "Broken pipe".
    let after = verifier
        .inner
        .request("probe", json!({}), Duration::from_millis(200))
        .await
        .expect_err("channel must stay invalidated after EOF");
    assert_eq!(after.kind(), std::io::ErrorKind::BrokenPipe);
    assert!(
        after.to_string().contains("invalidated"),
        "canale non invalidato dopo EOF: {after}"
    );
}

#[tokio::test]
async fn authority_malformed_response_is_rejected() {
    let (verifier, mut peer, proof) = fixture();
    peer.write_all(b"not-json\n").unwrap();
    assert!(matches!(
        verifier.verify(&proof, &proof.challenge).await,
        Err(AuthorityRefusal::Malformed)
    ));
}
