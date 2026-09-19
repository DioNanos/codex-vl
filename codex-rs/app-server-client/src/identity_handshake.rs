/*
The client half of the NexusCrew identity handshake.

Both transports run the same sequence once the server announces identity: the
initialize response carries the daemon challenge, the client asks the authority
for a proof on the inherited channel, and that proof is bound to the connection
before the first protected request. The remote transport always did this; the
in-process (embedded) transport must do the same. The wire contract lives here so
both transports share one implementation instead of two.

Channel discipline: proof (`nexuscrew/identity/challengeProof`, sent by the
client) and verify (`nexuscrew/identity/verify`, sent by the app-server) travel
over the same inherited descriptor pair, so they are serialized by the channel's
single exchange mutex (`codex_app_server::identity_channel::IdentityFdChannel`)
and its monotonic request ids. The bind, and therefore the server-side verify,
starts only after the client's proof exchange returned: there is never more than
one live exchange on the pair.
*/

use codex_app_server::identity_channel::IdentityFdChannel;
use codex_app_server_protocol::IdentityChallenge;
use codex_app_server_protocol::IdentityClaims;
use codex_app_server_protocol::IdentityKind;
use codex_app_server_protocol::IdentityOrigin;
use codex_app_server_protocol::IdentityProof;
use std::io::Error as IoError;
use std::io::ErrorKind;
use std::io::Result as IoResult;
use std::time::Duration;

const IDENTITY_CHALLENGE_METHOD: &str = "nexuscrew/identity/challengeProof";
/// Proof deadline shared with the remote transport.
pub(crate) const IDENTITY_CHALLENGE_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Clone)]
pub(crate) struct IdentityProofChannel {
    pub(crate) inner: IdentityFdChannel,
}

impl IdentityProofChannel {
    /// Reuses a channel this process already captured.
    pub(crate) fn from_channel(inner: IdentityFdChannel) -> Self {
        Self { inner }
    }

    pub(crate) fn open_from_env() -> IoResult<Option<Self>> {
        IdentityFdChannel::from_env().map(|channel| channel.map(Self::from_channel))
    }

    /// Exchanges one proof request on the channel. Crate-visible because the
    /// channel tests exercise the timeout and refusal branches directly.
    pub(crate) async fn request_with_timeout(
        &self,
        challenge: &IdentityChallenge,
        duration: Duration,
    ) -> IoResult<serde_json::Value> {
        // Mirroring the daemon: the channel is invalidated ONLY on a
        // transport fault (timeout, EOF, invalid JSON, WriteZero). An
        // authority refusal (a JSON-RPC `error` object) is a response, not
        // a fault: the channel stays alive and later reconnects retry
        // instead of staying silent for the whole process.
        let response = match self
            .inner
            .request(
                IDENTITY_CHALLENGE_METHOD,
                serde_json::json!({"challenge":challenge}),
                duration,
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                self.inner.invalidate();
                return Err(error);
            }
        };
        if let Some(error) = response.get("error") {
            Err(IoError::new(
                ErrorKind::PermissionDenied,
                format!("identity challenge proof rejected: {error}"),
            ))
        } else {
            response
                .get("result")
                .and_then(|result| result.get("proof"))
                .filter(|proof| proof.is_object())
                .cloned()
                .ok_or_else(|| IoError::new(ErrorKind::InvalidData, "identity proof missing"))
        }
    }
}

/// Ask the authority for a proof of the challenge the server issued, using the
/// timeout the remote transport uses. Absent, refused, malformed or timed-out
/// answers are transport-level failures: the caller must fail closed.
pub(crate) async fn identity_proof_for_challenge(
    channel: &IdentityProofChannel,
    challenge: &IdentityChallenge,
) -> IoResult<IdentityProof> {
    let raw = channel
        .request_with_timeout(challenge, IDENTITY_CHALLENGE_TIMEOUT)
        .await?;
    identity_proof_from_authority(challenge, raw)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthorityChallengeProof {
    owner_instance_id: String,
    cell_id: String,
    audience: String,
    incarnation_id: String,
    launch_epoch: String,
    daemon_boot_id: String,
    connection_id: String,
    challenge: String,
    nonce: String,
    jti: String,
    issued_at: i64,
    expires_at: i64,
    authority_generation: String,
    generation: u64,
    tmux_session: String,
    binding_id: String,
    scopes: Vec<String>,
}

pub(crate) fn identity_proof_from_authority(
    challenge: &IdentityChallenge,
    raw_value: serde_json::Value,
) -> IoResult<IdentityProof> {
    let raw: AuthorityChallengeProof =
        serde_json::from_value(raw_value.clone()).map_err(|err| {
            IoError::new(
                ErrorKind::InvalidData,
                format!("invalid authority proof: {err}"),
            )
        })?;
    if raw.challenge != challenge.nonce
        || raw.audience != challenge.audience
        || raw.daemon_boot_id != challenge.daemon_boot_id
        || raw.connection_id != challenge.connection_id
        || raw.nonce != challenge.nonce
        || raw.issued_at < challenge.issued_at
        || raw.expires_at > challenge.expires_at
        || raw.expires_at <= raw.issued_at
        || raw.tmux_session.is_empty()
        || raw.binding_id.is_empty()
        || raw.scopes.is_empty()
        || raw.authority_generation.is_empty()
    {
        return Err(IoError::new(
            ErrorKind::PermissionDenied,
            "authority proof does not match daemon challenge",
        ));
    }
    let claims = IdentityClaims {
        issuer_owner: raw.owner_instance_id.clone(),
        audience: raw.audience.clone(),
        owner_instance_id: raw.owner_instance_id,
        cell_id: raw.cell_id,
        tmux_session: raw.tmux_session,
        incarnation_id: raw.incarnation_id,
        launch_epoch: raw.launch_epoch,
        daemon_boot_id: raw.daemon_boot_id,
        connection_id: raw.connection_id,
        binding_id: raw.binding_id,
        origin: IdentityOrigin::LocalTui,
        scopes: raw.scopes,
        issued_at: raw.issued_at,
        not_before: raw.issued_at,
        expires_at: raw.expires_at,
        nonce: raw.nonce,
        thread_id: None,
        cwd: None,
        live_host: None,
    };
    claims.validate().map_err(|error| {
        IoError::new(
            ErrorKind::InvalidData,
            format!("invalid identity claims: {error:?}"),
        )
    })?;
    Ok(IdentityProof {
        version: "1".to_string(),
        kind: IdentityKind::ConnectionV1,
        challenge: challenge.clone(),
        claims,
        proof: serde_json::to_string(&serde_json::json!({
            "generation": raw.generation,
            "proof": raw_value,
        }))
        .map_err(IoError::other)?,
    })
}
