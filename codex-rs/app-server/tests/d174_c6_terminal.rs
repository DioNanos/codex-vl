#![allow(clippy::expect_used)]

#[path = "common/identity_fixture.rs"]
mod identity_fixture;

use anyhow::Context;
use anyhow::Result;
use identity_fixture::IdentityFixture;
use serde_json::json;

#[tokio::test]
async fn protected_endpoint_rejects_legacy_dispatch_without_effects() -> Result<()> {
    let fixture = IdentityFixture::start_protected().await?;
    let mut verified = fixture
        .connect("owner-a", "cell-a", "incarnation-a")
        .await?;
    let before = verified
        .request("thread/loaded/list", Some(json!({})))
        .await?;
    assert_eq!(before["data"], json!([]), "fresh daemon: {before}");
    let mut legacy = fixture.connect_unbound().await?;
    for (method, params) in [
        ("thread/start", json!({"ephemeral":true})),
        (
            "thread/resume",
            json!({"threadId":"00000000-0000-0000-0000-000000000001"}),
        ),
        (
            "thread/fork",
            json!({"threadId":"00000000-0000-0000-0000-000000000001"}),
        ),
        ("thread/loaded/list", json!({})),
    ] {
        let response = legacy.request(method, Some(params)).await?;
        assert_eq!(
            response["error"]["message"], "IDENTITY_UNVERIFIED",
            "{method}: {response}"
        );
    }
    let after = verified
        .request("thread/loaded/list", Some(json!({})))
        .await?;
    assert_eq!(
        after["data"],
        json!([]),
        "denied calls must create no threads: {after}"
    );
    let started = verified
        .request("thread/start", Some(json!({"ephemeral":true})))
        .await?;
    assert!(
        started["thread"]["id"].is_string(),
        "verified start: {started}"
    );
    fixture.stop().await?;
    Ok(())
}

#[tokio::test]
async fn standalone_endpoint_preserves_legacy_start() -> Result<()> {
    let fixture = IdentityFixture::start().await?;
    let mut legacy = fixture.connect_unbound().await?;
    let response = legacy
        .request("thread/start", Some(json!({"ephemeral":true})))
        .await?;
    fixture.stop().await?;
    assert!(
        response["thread"]["id"].is_string(),
        "standalone start: {response}"
    );
    Ok(())
}

#[tokio::test]
async fn auditor_first_mcp_spawn_on_bound_connection_scrubs_reserved_env() -> Result<()> {
    let capture = tempfile::tempdir()?;
    let capture_path = capture.path().join("spawn.jsonl");
    let script = r#"
const fs = require('fs');
fs.appendFileSync(process.env.AUDIT_CAPTURE, JSON.stringify({session:process.env.NEXUSCREW_MCP_SESSION ?? null,tmux:process.env.TMUX ?? null,pane:process.env.TMUX_PANE ?? null})+'\n');
require('readline').createInterface({input:process.stdin}).on('line',line=>{
 const m=JSON.parse(line); if(m.id===undefined)return;
 const result=m.method==='initialize'?{protocolVersion:m.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'audit-probe',version:'1'}}:m.method==='tools/list'?{tools:[]}:{};
 process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:m.id,result})+'\n');
});
"#;
    let fixture = IdentityFixture::start().await?;
    let mut client = fixture
        .connect("owner-a", "cell-a", "incarnation-a")
        .await?;
    let response = client.request("thread/start", Some(json!({
        "ephemeral": true,
        "config": {"mcp_servers": {"audit_probe": {
            "command":"/usr/bin/node", "args":["-e", script],
            "startup_timeout_sec": 2, "required": true,
            "env":{"AUDIT_CAPTURE":capture_path.to_string_lossy(),
                "NEXUSCREW_MCP_SESSION":"dummy-session", "TMUX":"dummy-tmux", "TMUX_PANE":"dummy-pane"}
        }}}
    }))).await?;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    fixture.stop().await?;
    assert!(
        response.get("error").is_none(),
        "thread/start failed: {response}"
    );
    let records =
        std::fs::read_to_string(capture_path).context("real MCP process must record startup")?;
    println!("mcp_spawn_records={records}");
    let values = records
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert!(!values.is_empty(), "vacuous observation");
    assert!(
        values
            .iter()
            .all(|v| *v == json!({"session":null,"tmux":null,"pane":null})),
        "every shared verified spawn must scrub config identity, including the first one"
    );
    Ok(())
}

#[tokio::test]
async fn new_tui_b_never_reuses_a_identity() -> Result<()> {
    let fixture = IdentityFixture::start().await?;
    let (mut tui_a, proof_a) = fixture
        .connect_record("owner-a", "cell-a", "incarnation-a", None)
        .await?;
    let mut tui_b = fixture
        .connect("owner-b", "cell-b", "incarnation-b")
        .await?;

    let reused = fixture.connect_with_proof(proof_a).await;
    assert!(reused.is_err(), "proof from TUI A must not bind TUI B");

    assert_ne!(tui_a.binding_id(), tui_b.binding_id());
    assert!(
        tui_a
            .request("server/diagnostics", Some(json!({})))
            .await?
            .is_object()
    );
    assert!(
        tui_b
            .request("server/diagnostics", Some(json!({})))
            .await?
            .is_object()
    );
    fixture.stop().await?;
    Ok(())
}

#[tokio::test]
async fn resume_fork_and_new_incarnation_reauthorize() -> Result<()> {
    let fixture = IdentityFixture::start().await?;
    let (_first, stale_proof) = fixture
        .connect_record("owner-a", "cell-a", "incarnation-a", None)
        .await?;
    fixture.restart().await?;

    let stale = fixture.connect_with_proof(stale_proof).await;
    assert!(
        stale.is_err(),
        "stale proof must not bind after daemon restart"
    );

    let (mut resumed, fresh_proof) = fixture
        .connect_record("owner-a", "cell-a", "incarnation-b", None)
        .await?;
    assert_ne!(fresh_proof.claims.incarnation_id, "incarnation-a");
    assert!(
        resumed
            .request("server/diagnostics", Some(json!({})))
            .await?
            .is_object()
    );
    fixture.stop().await?;
    Ok(())
}

#[tokio::test]
async fn cwd_does_not_become_identity() -> Result<()> {
    let fixture = IdentityFixture::start().await?;
    let (mut tui_a, proof_a) = fixture
        .connect_record(
            "owner-a",
            "cell-a",
            "incarnation-a",
            Some("/tmp/worktree-a"),
        )
        .await?;
    let (mut tui_b, proof_b) = fixture
        .connect_record(
            "owner-b",
            "cell-b",
            "incarnation-b",
            Some("/tmp/worktree-b"),
        )
        .await?;

    assert_eq!(proof_a.claims.cwd.as_deref(), Some("/tmp/worktree-a"));
    assert_eq!(proof_b.claims.cwd.as_deref(), Some("/tmp/worktree-b"));
    assert_ne!(
        proof_a.claims.owner_instance_id,
        proof_b.claims.owner_instance_id
    );
    assert_ne!(tui_a.binding_id(), tui_b.binding_id());
    let mut wrong_audience = fixture.connect_identity_unbound().await?;
    let mut wrong_audience_proof = proof_a.clone();
    wrong_audience_proof.claims.audience = "daemon/wrong-audience".to_string();
    let audience_rejected = wrong_audience.bind_raw(wrong_audience_proof).await?;
    assert_eq!(
        audience_rejected["error"]["message"],
        "identity bind failed: AudienceMismatch"
    );
    let mut wrong_boot = fixture.connect_identity_unbound().await?;
    let mut wrong_boot_proof = proof_b.clone();
    wrong_boot_proof.claims.daemon_boot_id = "wrong-daemon-boot".to_string();
    let boot_rejected = wrong_boot.bind_raw(wrong_boot_proof).await?;
    assert_eq!(
        boot_rejected["error"]["message"],
        "identity bind failed: AudienceMismatch"
    );
    assert!(
        tui_a
            .request("server/diagnostics", Some(json!({})))
            .await?
            .is_object()
    );
    assert!(
        tui_b
            .request("server/diagnostics", Some(json!({})))
            .await?
            .is_object()
    );
    fixture.stop().await?;
    Ok(())
}

#[tokio::test]
async fn stale_socket_and_restart_are_scoped() -> Result<()> {
    let fixture = IdentityFixture::start().await?;
    let (mut bound, proof) = fixture
        .connect_record(
            "owner-before-restart",
            "cell-before-restart",
            "incarnation-a",
            None,
        )
        .await
        .context("connect_record before replay")?;
    let replay = bound.bind_raw(proof).await?;
    assert_eq!(replay["error"]["message"], "identity bind failed: Replay");
    let socket = fixture.socket_path()?;
    fixture.stop().await?;
    let stale_listener = std::os::unix::net::UnixListener::bind(&socket)?;
    drop(stale_listener);
    fixture.restart().await?;

    let mut client = fixture.connect_unbound().await?;
    assert!(
        client
            .request("server/diagnostics", Some(json!({})))
            .await?
            .is_object()
    );
    fixture.stop().await?;
    Ok(())
}

#[tokio::test]
async fn thread_binding_rejects_cross_owner_resume_and_preserves_owner_on_fork() -> Result<()> {
    let fixture = IdentityFixture::start().await?;
    let mut tui_a = fixture
        .connect("owner-a", "cell-a", "incarnation-a")
        .await?;
    let mut tui_b = fixture
        .connect("owner-b", "cell-b", "incarnation-b")
        .await?;

    let started = tui_a
        .request(
            "thread/start",
            Some(json!({"ephemeral": false, "historyMode": "legacy"})),
        )
        .await?;
    let thread_id = started["thread"]["id"]
        .as_str()
        .context("thread/start did not return a thread id")?
        .to_string();
    let rollout_path = started["thread"]["path"]
        .as_str()
        .context("thread/start did not return a rollout path")?;
    tokio::fs::create_dir_all(
        std::path::Path::new(rollout_path)
            .parent()
            .context("rollout path has no parent")?,
    )
    .await?;
    tokio::fs::write(
        rollout_path,
        format!(
            "{{\"timestamp\":\"2026-09-06T03:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"session_id\":\"{thread_id}\",\"id\":\"{thread_id}\",\"timestamp\":\"2026-09-06T03:00:00Z\",\"cwd\":\"/tmp\",\"originator\":\"d174-c5-test\",\"cli_version\":\"0.153.2\",\"model_provider\":\"openai\"}}}}\n"
        ),
    )
    .await?;

    let rejected = tui_b
        .request(
            "thread/resume",
            Some(json!({"threadId": thread_id, "excludeTurns": true})),
        )
        .await?;
    assert_eq!(
        rejected["error"]["message"],
        "thread identity binding owner mismatch"
    );

    let resumed = tui_a
        .request(
            "thread/resume",
            Some(json!({"threadId": thread_id, "excludeTurns": true})),
        )
        .await?;
    assert!(
        resumed.get("error").is_none(),
        "owner resume must not be rejected: {resumed}"
    );

    let forked = tui_a
        .request(
            "thread/fork",
            Some(json!({"threadId": thread_id, "excludeTurns": true})),
        )
        .await?;
    let fork_id = forked["thread"]["id"]
        .as_str()
        .context("thread/fork did not return a thread id")?
        .to_string();
    assert_ne!(fork_id, thread_id);

    let fork_rejected = tui_b
        .request(
            "thread/resume",
            Some(json!({"threadId": fork_id, "excludeTurns": true})),
        )
        .await?;
    assert_eq!(
        fork_rejected["error"]["message"],
        "thread identity binding owner mismatch"
    );

    fixture.stop().await?;
    Ok(())
}

#[tokio::test]
async fn thread_binding_rejects_unbound_resume_and_fork() -> Result<()> {
    let fixture = IdentityFixture::start().await?;
    let mut tui_a = fixture
        .connect("owner-a", "cell-a", "incarnation-a")
        .await?;
    let mut unbound = fixture.connect_unbound().await?;

    let started = tui_a
        .request(
            "thread/start",
            Some(json!({"ephemeral": false, "historyMode": "legacy"})),
        )
        .await?;
    let thread_id = started["thread"]["id"]
        .as_str()
        .context("thread/start did not return a thread id")?
        .to_string();
    let rollout_path = started["thread"]["path"]
        .as_str()
        .context("thread/start did not return a rollout path")?;
    tokio::fs::create_dir_all(
        std::path::Path::new(rollout_path)
            .parent()
            .context("rollout path has no parent")?,
    )
    .await?;
    tokio::fs::write(
        rollout_path,
        format!(
            "{{\"timestamp\":\"2026-09-06T03:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"session_id\":\"{thread_id}\",\"id\":\"{thread_id}\",\"timestamp\":\"2026-09-06T03:00:00Z\",\"cwd\":\"/tmp\",\"originator\":\"d174-c5-unbound-test\",\"cli_version\":\"0.153.2\",\"model_provider\":\"openai\"}}}}\n"
        ),
    )
    .await?;

    let resume_rejected = unbound
        .request(
            "thread/resume",
            Some(json!({"threadId": thread_id, "excludeTurns": true})),
        )
        .await?;
    assert_eq!(
        resume_rejected["error"]["message"],
        "identity required for bound thread"
    );

    let resumed = tui_a
        .request(
            "thread/resume",
            Some(json!({"threadId": thread_id, "excludeTurns": true})),
        )
        .await?;
    assert!(
        resumed.get("error").is_none(),
        "owner resume must not be rejected: {resumed}"
    );

    let fork_rejected = unbound
        .request(
            "thread/fork",
            Some(json!({"threadId": thread_id, "excludeTurns": true})),
        )
        .await?;
    assert_eq!(
        fork_rejected["error"]["message"],
        "identity required for bound thread"
    );

    fixture.stop().await?;
    Ok(())
}

#[tokio::test]
async fn thread_binding_persistence_rejects_other_owner_and_unbound_after_restart() -> Result<()> {
    let fixture = IdentityFixture::start().await?;
    let mut tui_a = fixture
        .connect("owner-a", "cell-a", "incarnation-a")
        .await?;
    let started = tui_a
        .request(
            "thread/start",
            Some(json!({"ephemeral": false, "historyMode": "legacy"})),
        )
        .await?;
    let thread_id = started["thread"]["id"]
        .as_str()
        .context("thread/start did not return a thread id")?
        .to_string();
    let rollout_path = started["thread"]["path"]
        .as_str()
        .context("thread/start did not return a rollout path")?;
    tokio::fs::create_dir_all(
        std::path::Path::new(rollout_path)
            .parent()
            .context("rollout path has no parent")?,
    )
    .await?;
    tokio::fs::write(
        rollout_path,
        format!(
            "{{\"timestamp\":\"2026-09-06T03:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"session_id\":\"{thread_id}\",\"id\":\"{thread_id}\",\"timestamp\":\"2026-09-06T03:00:00Z\",\"cwd\":\"/tmp\",\"originator\":\"d174-c5-persisted-test\",\"cli_version\":\"0.153.2\",\"model_provider\":\"openai\"}}}}\n"
        ),
    )
    .await?;
    drop(tui_a);

    fixture.restart().await?;
    let mut resumed_a = fixture
        .connect("owner-a", "cell-a", "incarnation-a")
        .await?;
    let mut tui_b = fixture
        .connect("owner-b", "cell-b", "incarnation-b")
        .await?;
    let mut unbound = fixture.connect_unbound().await?;

    let resumed = resumed_a
        .request(
            "thread/resume",
            Some(json!({"threadId": thread_id, "excludeTurns": true})),
        )
        .await?;
    assert!(
        resumed.get("error").is_none(),
        "persisted owner resume must not be rejected: {resumed}"
    );

    let rejected_b = tui_b
        .request(
            "thread/resume",
            Some(json!({"threadId": thread_id, "excludeTurns": true})),
        )
        .await?;
    assert_eq!(
        rejected_b["error"]["message"],
        "thread identity binding owner mismatch"
    );

    let rejected_unbound = unbound
        .request(
            "thread/resume",
            Some(json!({"threadId": thread_id, "excludeTurns": true})),
        )
        .await?;
    assert_eq!(
        rejected_unbound["error"]["message"],
        "identity required for bound thread"
    );

    fixture.stop().await?;
    Ok(())
}

async fn start_owned_thread_for_auditor_probe(
    fixture: &IdentityFixture,
) -> Result<(String, String)> {
    let mut owner = fixture
        .connect("owner-a", "cell-a", "incarnation-a")
        .await?;
    let started = owner
        .request(
            "thread/start",
            Some(json!({"ephemeral": false, "historyMode": "legacy"})),
        )
        .await?;
    let id = started["thread"]["id"]
        .as_str()
        .context("thread id")?
        .to_owned();
    let path = started["thread"]["path"]
        .as_str()
        .context("rollout path")?
        .to_owned();
    tokio::fs::create_dir_all(std::path::Path::new(&path).parent().context("parent")?).await?;
    tokio::fs::write(
        &path,
        format!(
            "{{\"timestamp\":\"2026-09-06T03:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"session_id\":\"{id}\",\"id\":\"{id}\",\"timestamp\":\"2026-09-06T03:00:00Z\",\"cwd\":\"/tmp\",\"originator\":\"audit-probe\",\"cli_version\":\"0.153.2\",\"model_provider\":\"openai\"}}}}\n"
        ),
    )
    .await?;
    Ok((id, path))
}

#[tokio::test]
async fn auditor_bound_thread_path_resume_requires_owner() -> Result<()> {
    let fixture = IdentityFixture::start().await?;
    let (id, path) = start_owned_thread_for_auditor_probe(&fixture).await?;
    fixture.restart().await?;
    let mut other = fixture
        .connect("owner-b", "cell-b", "incarnation-b")
        .await?;
    let control = other
        .request(
            "thread/resume",
            Some(json!({"threadId": id, "excludeTurns": true})),
        )
        .await?;
    let via_path = other
        .request(
            "thread/resume",
            Some(json!({"threadId": "not-a-uuid", "path": path, "excludeTurns": true})),
        )
        .await?;
    let mut unbound = fixture.connect_unbound().await?;
    let via_path_unbound = unbound
        .request(
            "thread/resume",
            Some(json!({"threadId": "not-a-uuid", "path": path, "excludeTurns": true})),
        )
        .await?;
    fixture.stop().await?;
    println!(
        "control_error={:?}; path_error={:?}; path_thread={:?}; unbound_path_error={:?}; unbound_thread={:?}",
        control["error"]["message"],
        via_path["error"]["message"],
        via_path["thread"]["id"],
        via_path_unbound["error"]["message"],
        via_path_unbound["thread"]["id"]
    );
    assert_eq!(
        control["error"]["message"],
        "thread identity binding owner mismatch"
    );
    assert_eq!(
        via_path["error"]["message"], "thread identity binding owner mismatch",
        "path must authorize the resolved owner before resume"
    );
    assert_eq!(
        via_path_unbound["error"]["message"],
        "identity required for bound thread"
    );
    Ok(())
}

#[tokio::test]
async fn auditor_new_incarnation_reauthorizes_existing_owned_thread() -> Result<()> {
    let fixture = IdentityFixture::start().await?;
    let (id, _) = start_owned_thread_for_auditor_probe(&fixture).await?;
    fixture.restart().await?;
    let mut owner = fixture
        .connect("owner-a", "cell-a", "incarnation-new")
        .await?;
    let resumed = owner
        .request(
            "thread/resume",
            Some(json!({"threadId": id, "excludeTurns": true})),
        )
        .await?;
    fixture.stop().await?;
    println!(
        "fresh_binding={:?}; resume_error={:?}",
        owner.binding_id(),
        resumed["error"]["message"]
    );
    assert!(
        resumed.get("error").is_none(),
        "fresh owner authorization must permit existing owned thread resume"
    );
    Ok(())
}
