use chrono::Utc;
use codex_app_server_protocol::{
    IDENTITY_SCHEMA_VERSION, IDENTITY_VERIFY_VERSION, IdentityBindResponse, IdentityBinding,
    IdentityChallenge, IdentityClaims, IdentityErrorCode, IdentityOrigin, IdentityProof,
};
use std::ffi::OsStr;
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

#[cfg(feature = "d174-test-fixture")]
use std::io::Write;

use crate::outgoing_message::ConnectionId;

/// The launcher-owned identity flag. Any value other than `0`/`false` requests
/// identity enforcement, including an empty string. This rule is unchanged from
/// the first verifier gate; only the failure behaviour moved to startup.
pub(crate) fn identity_required_value(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| value != "0" && value != "false")
}

/// Read the launcher flag exactly once, before any descriptor is captured.
pub(crate) fn identity_required_from_env() -> bool {
    identity_required_value(std::env::var_os("CODEX_APP_SERVER_IDENTITY_REQUIRED").as_deref())
}
/// Verifier success result: the claims RENORMALIZED by the authority. The
/// bind builds the binding ONLY from these, never from the client's claims.
/// NORMALIZED claims returned by the authority (verify v1): the fields
/// signed and confirmed by the authority, without nonce/parentJti/jti/challenge/proof
/// (those stay server-side). Deserializable from the v1 `claims` payload.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedIdentityClaims {
    pub owner_instance_id: String,
    pub issuer_owner: String,
    pub cell_id: String,
    pub audience: String,
    pub incarnation_id: String,
    pub launch_epoch: String,
    pub daemon_boot_id: String,
    pub connection_id: String,
    pub tmux_session: String,
    pub binding_id: String,
    pub scopes: Vec<String>,
    pub origin: IdentityOrigin,
    pub issued_at: i64,
    pub not_before: i64,
    pub expires_at: i64,
    pub nonce: String,
    #[serde(default)]
    pub generation: u64,
}

/// Authority refusal. The `Rejected("challenge_mismatch")` variant
/// produces `AudienceMismatch` at bind time, every other variant maps to
/// `IdentityUnverified` — see `identity_error_for_refusal`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityRefusal {
    /// authority absent/down/timed out (fail-closed)
    Unreachable,
    /// the authority refused: bad-proof, expired, replay, revoked,
    /// generation, subject mismatch (closed reason enum)
    Rejected(&'static str),
    /// protocol violation (response without valid claims)
    Malformed,
}

/// The daemon does not verify the HMAC itself — it queries the NexusCrew
/// authority online, the only holder of the verifier key. The real
/// implementation travels over the verify v1 channel (spec:
/// `docs/identity/verify-channel-v1.md`, part 2); tests use fixtures.
/// A mismatch between the claims and the expected challenge is an AUDIENCE
/// mismatch and the bind rejects it with `AudienceMismatch`; every other
/// refusal stays `IdentityUnverified` (fail-closed unchanged). The
/// distinction matters to whoever reads the error: a proof issued for a
/// different connection is not an unverifiable proof.
pub(crate) fn identity_error_for_refusal(refusal: &AuthorityRefusal) -> IdentityErrorCode {
    match refusal {
        AuthorityRefusal::Rejected("challenge_mismatch") => IdentityErrorCode::AudienceMismatch,
        _ => IdentityErrorCode::IdentityUnverified,
    }
}

pub trait AuthorityVerifier: Send + Sync {
    fn verify<'a>(
        &'a self,
        proof: &'a IdentityProof,
        challenge: &'a IdentityChallenge,
    ) -> Pin<Box<dyn Future<Output = Result<VerifiedIdentityClaims, AuthorityRefusal>> + Send + 'a>>;
}

/// Daemon clock tolerance: 30s — the NC authority still verifies expiry
/// online; the tolerance avoids false refusals between hosts with skewed
/// clocks.
pub const CLOCK_TOLERANCE_SECS: i64 = 30;

/// Per-connection identity handshake state. Authority verification is injected
/// at the bind boundary; this type never trusts process environment metadata.
#[derive(Debug, Default)]
pub(crate) struct ConnectionIdentityState {
    required: bool,
    challenge: Option<IdentityChallenge>,
    binding: Option<IdentityBinding>,
}

impl ConnectionIdentityState {
    pub(crate) fn advertise(
        &mut self,
        connection_id: ConnectionId,
        required: bool,
        _supported: bool,
    ) {
        self.required |= required;
        // The launcher latch alone decides enforcement: a challenge is issued
        // only when identity is required. Client capability announcement never
        // triggers one, so an explicit standalone declaration produces a
        // connection with no identity messages and no channel consumption.
        if required && self.challenge.is_none() {
            let issued_at = Utc::now().timestamp_millis();
            let challenge = IdentityChallenge {
                version: IDENTITY_VERIFY_VERSION,
                connection_id: connection_id.0.to_string(),
                daemon_boot_id: Uuid::now_v7().to_string(),
                audience: format!("daemon/{}", connection_id.0),
                nonce: format!("{}{}", Uuid::now_v7().simple(), Uuid::now_v7().simple()),
                issued_at,
                expires_at: issued_at + 15_000,
            };
            #[cfg(feature = "d174-test-fixture")]
            if let Ok(path) = std::env::var("D174_IDENTITY_CHALLENGE_FILE")
                && let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                && let Ok(line) = serde_json::to_string(&challenge)
            {
                let _ = writeln!(file, "{line}");
            }
            self.challenge = Some(challenge);
        }
    }

    pub(crate) fn required(&self) -> bool {
        self.required
    }

    pub(crate) fn ready(&self) -> bool {
        !self.required || self.binding.is_some()
    }

    pub(crate) fn challenge(&self) -> Option<IdentityChallenge> {
        self.challenge.clone()
    }

    pub(crate) fn binding(&self) -> Option<IdentityBinding> {
        self.binding.clone()
    }
}

/// State prepared by the synchronous part of `prepare`: all structural and
/// validity checks already passed; only the online verification remains.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PreparedBind {
    proof: IdentityProof,
    challenge: IdentityChallenge,
}

impl PreparedBind {
    pub(crate) fn proof(&self) -> &IdentityProof {
        &self.proof
    }

    pub(crate) fn challenge(&self) -> &IdentityChallenge {
        &self.challenge
    }
}

impl ConnectionIdentityState {
    /// Synchronous phase: wire contract, challenge binding, validity and —
    /// with identity_required ON — a time window against the daemon clock
    /// (tolerance `CLOCK_TOLERANCE_SECS`, default 0 = fail-closed).
    /// Does not mutate state: the commit happens only after the verify.
    pub(crate) fn prepare(&self, proof: &IdentityProof) -> Result<PreparedBind, IdentityErrorCode> {
        // The NC normalizer remains the authority for clock-now, owner/cell/session,
        // and liveHost coherence. These checks only enforce the wire contract and
        // challenge binding; they never turn an unnormalized proof into authority.
        if self.binding.is_some() {
            return Err(IdentityErrorCode::Replay);
        }
        let Some(challenge) = self.challenge.as_ref() else {
            return Err(IdentityErrorCode::IdentityUnverified);
        };
        if proof.version != IDENTITY_SCHEMA_VERSION || proof.challenge != *challenge {
            return Err(IdentityErrorCode::AudienceMismatch);
        }
        proof.claims.validate()?;
        if proof.claims.connection_id != challenge.connection_id
            || proof.claims.daemon_boot_id != challenge.daemon_boot_id
            || proof.claims.audience != challenge.audience
            || proof.claims.nonce != challenge.nonce
        {
            return Err(IdentityErrorCode::AudienceMismatch);
        }
        if self.required {
            let now = Utc::now().timestamp_millis();
            let tolerance = CLOCK_TOLERANCE_SECS * 1_000;
            if now < proof.claims.not_before - tolerance
                || now >= proof.claims.expires_at + tolerance
            {
                return Err(IdentityErrorCode::InvalidTime);
            }
        }
        Ok(PreparedBind {
            proof: proof.clone(),
            challenge: challenge.clone(),
        })
    }

    /// Commit: only the authority can produce the binding claims. With
    /// identity_required ON a `verified: None` is fail-closed
    /// (IdentityUnverified): no production path accepts an unauthenticated
    /// proof.
    pub(crate) fn commit(
        &mut self,
        prepared: PreparedBind,
        verified: Option<VerifiedIdentityClaims>,
    ) -> Result<IdentityBindResponse, IdentityErrorCode> {
        if self.binding.is_some() {
            return Err(IdentityErrorCode::Replay);
        }
        if self.required {
            let Some(normalized) = verified else {
                return Err(IdentityErrorCode::IdentityUnverified);
            };
            // The binding is built exclusively from the claims the
            // authority normalized. No field may come from the client's
            // outer claims: threadId/cwd/liveHost stay None (not requested
            // for the local_tui origin), and validate() completes the
            // check. The outer claims must MATCH the signed claims: a proof
            // with rewritten outer claims is tampered and is rejected
            // instead of silently normalized.
            let outer = &prepared.proof.claims;
            if outer.owner_instance_id != normalized.owner_instance_id
                || outer.issuer_owner != normalized.issuer_owner
                || outer.cell_id != normalized.cell_id
                || outer.audience != normalized.audience
                || outer.incarnation_id != normalized.incarnation_id
                || outer.launch_epoch != normalized.launch_epoch
                || outer.daemon_boot_id != normalized.daemon_boot_id
                || outer.connection_id != normalized.connection_id
                || outer.tmux_session != normalized.tmux_session
                || outer.binding_id != normalized.binding_id
                || outer.origin != normalized.origin
                || outer.scopes != normalized.scopes
                || outer.issued_at != normalized.issued_at
                || outer.not_before != normalized.not_before
                || outer.expires_at != normalized.expires_at
                || outer.nonce != normalized.nonce
            {
                return Err(IdentityErrorCode::IdentityUnverified);
            }
            let claims = IdentityClaims {
                issuer_owner: normalized.issuer_owner,
                audience: normalized.audience,
                owner_instance_id: normalized.owner_instance_id,
                cell_id: normalized.cell_id,
                tmux_session: normalized.tmux_session,
                incarnation_id: normalized.incarnation_id,
                launch_epoch: normalized.launch_epoch,
                daemon_boot_id: normalized.daemon_boot_id,
                connection_id: normalized.connection_id,
                binding_id: normalized.binding_id,
                origin: normalized.origin,
                scopes: normalized.scopes,
                issued_at: normalized.issued_at,
                not_before: normalized.not_before,
                expires_at: normalized.expires_at,
                nonce: normalized.nonce,
                thread_id: None,
                cwd: None,
                live_host: None,
            };
            claims.validate()?;
            let binding = IdentityBinding {
                binding_id: claims.binding_id.clone(),
                claims,
            };
            self.binding = Some(binding.clone());
            return Ok(IdentityBindResponse { binding });
        }
        // Standalone (identity_required OFF): structural path only, with no
        // verifier and no privileges.
        let binding = IdentityBinding {
            binding_id: prepared.proof.claims.binding_id.clone(),
            claims: prepared.proof.claims,
        };
        self.binding = Some(binding.clone());
        Ok(IdentityBindResponse { binding })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_server_protocol::{IdentityClaims, IdentityKind, IdentityOrigin};
    use std::sync::Arc;

    #[test]
    fn launcher_flag_truthy_values_request_identity() {
        for value in ["1", "true", "yes", ""] {
            assert!(
                identity_required_value(Some(std::ffi::OsStr::new(value))),
                "value {value:?} must request identity"
            );
        }
    }

    #[test]
    fn launcher_flag_absent_or_false_is_standalone() {
        for value in [
            None,
            Some(std::ffi::OsStr::new("0")),
            Some(std::ffi::OsStr::new("false")),
        ] {
            assert!(
                !identity_required_value(value),
                "value {value:?} must stay standalone"
            );
        }
    }
    use serde_json::json;

    struct FixtureVerifier {
        mode: FixtureMode,
        /// Claims signed by the authority: the model is that the HMAC covers
        /// the ORIGINAL claims; client-rewritten outer claims do not change them.
        signed: Option<IdentityClaims>,
    }

    #[derive(Clone, Copy)]
    enum FixtureMode {
        /// authority verifies and normalizes (happy path)
        Accept,
        /// authority refuses: bad-proof / wrong key / tampered claims
        Reject,
        /// authority unreachable
        Unreachable,
    }

    impl AuthorityVerifier for FixtureVerifier {
        fn verify<'a>(
            &'a self,
            proof: &'a IdentityProof,
            _challenge: &'a IdentityChallenge,
        ) -> Pin<
            Box<dyn Future<Output = Result<VerifiedIdentityClaims, AuthorityRefusal>> + Send + 'a>,
        > {
            match self.mode {
                FixtureMode::Accept => {
                    // Fixture v1.1: the authority verifies the HMAC on the
                    // SIGNED claims (not the client's outer ones) and returns
                    // the v1.1 superset from its records.
                    let normalized = self.signed.clone().unwrap_or_else(|| proof.claims.clone());
                    let verified = crate::identity::VerifiedIdentityClaims {
                        issuer_owner: normalized.issuer_owner.clone(),
                        owner_instance_id: normalized.owner_instance_id.clone(),
                        cell_id: normalized.cell_id.clone(),
                        audience: normalized.audience.clone(),
                        incarnation_id: normalized.incarnation_id.clone(),
                        launch_epoch: normalized.launch_epoch.clone(),
                        daemon_boot_id: normalized.daemon_boot_id.clone(),
                        connection_id: normalized.connection_id.clone(),
                        tmux_session: normalized.tmux_session.clone(),
                        binding_id: normalized.binding_id.clone(),
                        scopes: normalized.scopes.clone(),
                        origin: normalized.origin,
                        issued_at: normalized.issued_at,
                        not_before: normalized.not_before,
                        expires_at: normalized.expires_at,
                        nonce: normalized.nonce.clone(),
                        generation: 0,
                    };
                    Box::pin(async move { Ok(verified) })
                }
                FixtureMode::Reject => {
                    Box::pin(async { Err(AuthorityRefusal::Rejected("bad-proof")) })
                }
                FixtureMode::Unreachable => Box::pin(async { Err(AuthorityRefusal::Unreachable) }),
            }
        }
    }

    fn proof(state: &ConnectionIdentityState) -> IdentityProof {
        let challenge = state.challenge().expect("challenge");
        let issued_at = Utc::now().timestamp_millis() - 60_000;
        let expires_at = Utc::now().timestamp_millis() + 300_000;
        IdentityProof {
            version: "1".to_string(),
            kind: IdentityKind::ConnectionV1,
            challenge: challenge.clone(),
            claims: IdentityClaims {
                issuer_owner: "owner".to_string(),
                audience: challenge.audience.clone(),
                owner_instance_id: "owner".to_string(),
                cell_id: "cell".to_string(),
                tmux_session: "session".to_string(),
                incarnation_id: "incarnation".to_string(),
                launch_epoch: "epoch".to_string(),
                daemon_boot_id: challenge.daemon_boot_id.clone(),
                connection_id: challenge.connection_id.clone(),
                binding_id: "binding".to_string(),
                origin: IdentityOrigin::LocalTui,
                scopes: vec!["thread/start".to_string()],
                issued_at,
                not_before: issued_at,
                expires_at,
                nonce: challenge.nonce.clone(),
                thread_id: None,
                cwd: None,
                live_host: None,
            },
            proof: "authority-proof".to_string(),
        }
    }

    fn expired_proof(state: &ConnectionIdentityState) -> IdentityProof {
        let mut proof = proof(state);
        proof.claims.issued_at = 1_757_030_400_000;
        proof.claims.not_before = 1_757_030_400_000;
        proof.claims.expires_at = 1_757_116_800_000;
        proof
    }

    fn required_state() -> ConnectionIdentityState {
        let mut state = ConnectionIdentityState::default();
        state.advertise(
            ConnectionId(7),
            /*required*/ true,
            /*supported*/ true,
        );
        state
    }

    async fn run_bind(
        state: &mut ConnectionIdentityState,
        proof: &IdentityProof,
        mode: Option<FixtureMode>,
    ) -> Result<IdentityBindResponse, IdentityErrorCode> {
        run_bind_with_signed(state, proof, mode, None).await
    }

    async fn run_bind_with_signed(
        state: &mut ConnectionIdentityState,
        proof: &IdentityProof,
        mode: Option<FixtureMode>,
        signed: Option<IdentityClaims>,
    ) -> Result<IdentityBindResponse, IdentityErrorCode> {
        let prepared = state.prepare(proof)?;
        let verified = match mode {
            Some(mode) => {
                let verifier = FixtureVerifier {
                    mode,
                    signed: Some(signed.unwrap_or_else(|| proof.claims.clone())),
                };
                Some(
                    verifier
                        .verify(prepared.proof(), prepared.challenge())
                        .await
                        .map_err(|_| IdentityErrorCode::IdentityUnverified)?,
                )
            }
            None => None,
        };
        state.commit(prepared, verified)
    }

    #[test]
    fn required_connection_is_blocked_until_bind() {
        let mut state = required_state();
        assert!(!state.ready());
        let prepared = state.prepare(&proof(&state)).expect("prepare");
        let _ = prepared;
        assert!(!state.ready(), "il prepare non fa il bind: serve il commit");
    }

    #[test]
    fn verify_v1_wire_uses_numeric_challenge_and_claim_timestamps() {
        let state = required_state();
        let challenge = state.challenge().expect("challenge");
        let wire = serde_json::to_value(&challenge).expect("challenge json");
        assert!(wire["version"].is_u64(), "version deve essere numerica");
        assert!(wire["nonce"].as_str().is_some_and(|value| {
            value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        }));
        assert!(wire["issuedAt"].is_i64());
        assert!(wire["expiresAt"].is_i64());

        let claims = serde_json::json!({
            "ownerInstanceId": "owner",
            "issuerOwner": "owner",
            "cellId": "cell",
            "audience": challenge.audience,
            "incarnationId": "incarnation",
            "launchEpoch": "epoch",
            "daemonBootId": challenge.daemon_boot_id,
            "connectionId": challenge.connection_id,
            "tmuxSession": "session",
            "bindingId": "binding-id",
            "scopes": ["thread/start"],
            "origin": "local_tui",
            "issuedAt": 1_700_000_000_000_i64,
            "notBefore": 1_700_000_000_000_i64,
            "expiresAt": 1_700_000_005_000_i64,
            "nonce": challenge.nonce,
            "generation": 3,
        });
        let result = serde_json::json!({"ok": true, "v": 1, "claims": claims});
        let verified = crate::authority::map_verify_result(&result, &challenge, 3)
            .expect("numeric verify claims devono essere mappabili");
        assert_eq!(verified.issued_at, 1_700_000_000_000);
        assert_eq!(verified.expires_at, 1_700_000_005_000);
    }

    #[tokio::test]
    async fn verified_bind_binds_once_with_authority_claims() {
        let mut state = required_state();
        let first_proof = proof(&state);
        let response = run_bind(&mut state, &first_proof, Some(FixtureMode::Accept))
            .await
            .expect("bind con authority che accetta");
        assert_eq!(response.binding.binding_id, "binding");
        // v1.1: the binding carries the claims CONFIRMED by the authority,
        // which for an intact proof coincide with the proof's signed ones;
        // unexpected fields (threadId/cwd/liveHost) stay None.
        assert_eq!(
            response.binding.claims.owner_instance_id, first_proof.claims.owner_instance_id,
            "il binding porta i claim confermati dall'authority"
        );
        assert_eq!(response.binding.claims.thread_id, None);
        assert_eq!(response.binding.claims.cwd, None);
        assert_eq!(response.binding.claims.live_host, None);
        assert!(state.ready());
        // one-shot: a second bind is a replay even when the authority accepts
        let replay_proof = proof(&state);
        assert_eq!(
            run_bind(&mut state, &replay_proof, Some(FixtureMode::Accept)).await,
            Err(IdentityErrorCode::Replay)
        );
    }

    #[tokio::test]
    async fn tampered_outer_claims_are_rejected_not_normalized() {
        // v1.1: rewritten outer claims = tampered proof → refusal, not
        // silent normalization (negative test for "unsigned outer claims
        // must not enter binding").
        let mut state = required_state();
        let mut tampered = proof(&state);
        tampered.claims.tmux_session = "cloud-audit-forged".to_string();
        tampered.claims.binding_id = "audit-forged-binding".to_string();
        tampered.claims.issuer_owner = "audit-forged-issuer".to_string();
        tampered.claims.scopes = vec!["thread/start".to_string(), "thread/delete".to_string()];
        let original = proof(&state);
        let outcome = run_bind_with_signed(
            &mut state,
            &tampered,
            Some(FixtureMode::Accept),
            Some(original.claims.clone()),
        )
        .await;
        assert_eq!(outcome, Err(IdentityErrorCode::IdentityUnverified));
        assert!(!state.ready());
    }

    #[tokio::test]
    async fn claims_valid_but_proof_unauthenticated_fails_closed_d199() {
        // The key finding: valid claims + unauthenticated proof. Before the
        // fix this passed; with the fail-closed verifier it is IdentityUnverified.
        let mut state = required_state();
        let proof = proof(&state);
        let outcome = run_bind(&mut state, &proof, Some(FixtureMode::Reject)).await;
        assert_eq!(outcome, Err(IdentityErrorCode::IdentityUnverified));
        assert!(!state.ready());
    }

    #[tokio::test]
    async fn authority_unreachable_fails_closed() {
        let mut state = required_state();
        let proof = proof(&state);
        let outcome = run_bind(&mut state, &proof, Some(FixtureMode::Unreachable)).await;
        assert_eq!(
            outcome,
            Err(IdentityErrorCode::IdentityUnverified),
            "authority irraggiungibile = fail-closed, mai accetta-tutto"
        );
        assert!(!state.ready());
    }

    #[test]
    fn required_on_without_verifier_fails_closed() {
        // production without a wired verifier → ON = IdentityUnverified
        let mut state = required_state();
        let prepared = state.prepare(&proof(&state)).expect("prepare");
        assert_eq!(
            state.commit(prepared, /*verified*/ None),
            Err(IdentityErrorCode::IdentityUnverified)
        );
        assert!(!state.ready());
    }

    #[test]
    fn expired_proof_fails_closed_against_daemon_clock() {
        let mut state = required_state();
        let outcome = state.prepare(&expired_proof(&state));
        assert_eq!(
            outcome,
            Err(IdentityErrorCode::InvalidTime),
            "scaduto rispetto al clock del daemon: rifiuto prima del verify"
        );
    }

    #[test]
    fn replay_after_verified_bind_is_rejected_at_commit() {
        // replay across phases: prepared before the commit, commit run twice.
        let mut state = required_state();
        let prepared = state.prepare(&proof(&state)).expect("prepare");
        let verifier = FixtureVerifier {
            signed: None,
            mode: FixtureMode::Accept,
        };
        let verified =
            futures::executor::block_on(verifier.verify(prepared.proof(), prepared.challenge()))
                .expect("authority accetta");
        state.commit(prepared, Some(verified)).expect("primo bind");
        // one-shot: after the commit even PREPARE is closed (a binding exists
        // → Replay): no second path to the bind, not even with a proof
        // re-signed by the authority.
        let proof2 = proof(&state);
        assert_eq!(state.prepare(&proof2), Err(IdentityErrorCode::Replay));
    }

    #[tokio::test]
    async fn wrong_challenge_fails_closed() {
        let mut state = required_state();
        let mut invalid = proof(&state);
        invalid.challenge.connection_id = "other".to_string();
        let outcome = run_bind(&mut state, &invalid, Some(FixtureMode::Accept)).await;
        assert_eq!(outcome, Err(IdentityErrorCode::AudienceMismatch));
        assert!(!state.ready());
    }

    #[test]
    fn clock_tolerance_thirty_seconds_accepted_rejected_at_thirty_one() {
        // Tolerance: 30s — a not_before in the future within the tolerance
        // is accepted (clock skew); beyond that, refused.
        let mut state = required_state();
        let now = Utc::now().timestamp_millis();
        let mut within = proof(&state);
        within.claims.issued_at = now - 60_000;
        within.claims.not_before = now + 29_000;
        within.claims.expires_at = now + 300_000;
        assert!(
            state.prepare(&within).is_ok(),
            "29s entro tolleranza: accettato"
        );

        let mut beyond = proof(&state);
        beyond.claims.issued_at = now - 60_000;
        beyond.claims.not_before = now + 31_000;
        beyond.claims.expires_at = now + 300_000;
        assert_eq!(
            state.prepare(&beyond),
            Err(IdentityErrorCode::InvalidTime),
            "31s oltre tolleranza: rifiutato"
        );
    }

    #[test]
    fn map_verify_result_reason_mapping_and_challenge_binding() {
        use crate::authority::map_verify_result;
        let state = required_state();
        let challenge = state.challenge().expect("challenge");
        let proof = proof(&state);
        let mut claims = serde_json::to_value(&proof.claims).unwrap();
        // v1.1 superset: issuerOwner/notBefore/origin come from the
        // response mapping; the nonce comes from the authority record.
        claims["issuerOwner"] = claims["ownerInstanceId"].clone();
        claims["notBefore"] = claims["issuedAt"].clone();
        let result = json!({"ok": false, "v": 1, "reason": "bad-proof"});
        assert!(matches!(
            map_verify_result(&result, &challenge, 3),
            Err(AuthorityRefusal::Rejected("bad-proof"))
        ));
        let result = json!({"ok": false, "v": 1, "reason": "authority-unavailable"});
        assert!(matches!(
            map_verify_result(&result, &challenge, 3),
            Err(AuthorityRefusal::Unreachable)
        ));
        let mut normalized = claims.clone();
        normalized["ownerInstanceId"] = json!("authority-owner");
        let result = json!({"ok": true, "v": 1, "claims": normalized});
        let verified = map_verify_result(&result, &challenge, 3).expect("claims v1.1");
        assert_eq!(verified.owner_instance_id, "authority-owner");
        // Cross-challenge: tuple diverging from the expected challenge → refusal.
        let mut other = challenge.clone();
        other.nonce = "0".repeat(64);
        let result = json!({"ok": true, "v": 1, "claims": claims});
        assert!(matches!(
            map_verify_result(&result, &other, 3),
            Err(AuthorityRefusal::Rejected("challenge_mismatch"))
        ));
    }

    #[test]
    fn standalone_off_never_issues_a_challenge() {
        // An explicit standalone declaration suppresses the handshake at the
        // source: capability support alone must not produce a challenge, so a
        // not-required connection carries no identity messages and consumes no
        // channel, and it is ready without any bind.
        let mut state = ConnectionIdentityState::default();
        state.advertise(
            ConnectionId(7),
            /*required*/ false,
            /*supported*/ true,
        );
        assert!(
            state.challenge().is_none(),
            "a not-required connection must stay challenge-free"
        );
        assert!(
            state.ready(),
            "standalone connections are ready without a bind"
        );
    }

    #[test]
    fn challenge_mismatch_refusal_maps_to_audience_mismatch() {
        // Only `challenge_mismatch` changes the error code; every other
        // refusal stays fail-closed on IdentityUnverified.
        assert_eq!(
            identity_error_for_refusal(&AuthorityRefusal::Rejected("challenge_mismatch")),
            IdentityErrorCode::AudienceMismatch
        );
        for refusal in [
            AuthorityRefusal::Rejected("bad-proof"),
            AuthorityRefusal::Rejected("expired"),
            AuthorityRefusal::Rejected("replay"),
            AuthorityRefusal::Rejected("revoked"),
            AuthorityRefusal::Rejected("rejected"),
            AuthorityRefusal::Malformed,
            AuthorityRefusal::Unreachable,
        ] {
            assert_eq!(
                identity_error_for_refusal(&refusal),
                IdentityErrorCode::IdentityUnverified,
                "{refusal:?}"
            );
        }
    }
}
