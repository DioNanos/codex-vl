#![allow(clippy::expect_used)]

#[path = "common/identity_fixture.rs"]
mod identity_fixture;

use anyhow::Context;
use anyhow::Result;
use identity_fixture::IdentityFixture;
use serde_json::json;

#[tokio::test]
async fn new_tui_b_never_reuses_a_identity() -> Result<()> {
    let fixture = IdentityFixture::start().await?;
    let mut tui_a = fixture
        .connect("owner-a", "cell-a", "incarnation-a")
        .await?;
    let mut tui_b = fixture
        .connect("owner-b", "cell-b", "incarnation-b")
        .await?;

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
    let socket = fixture.socket_path()?;
    fixture.stop().await?;
    let stale_listener = std::os::unix::net::UnixListener::bind(&socket)?;
    drop(stale_listener);
    fixture.restart().await?;

    let mut client = fixture
        .connect(
            "owner-after-restart",
            "cell-after-restart",
            "incarnation-new",
        )
        .await?;
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
