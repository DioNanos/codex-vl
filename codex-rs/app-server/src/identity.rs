use chrono::{SecondsFormat, Utc};
use codex_app_server_protocol::{
    IDENTITY_SCHEMA_VERSION, IdentityBindResponse, IdentityBinding, IdentityChallenge,
    IdentityErrorCode, IdentityProof,
};
use uuid::Uuid;

#[cfg(feature = "d174-test-fixture")]
use std::io::Write;

use crate::outgoing_message::ConnectionId;

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
        supported: bool,
    ) {
        self.required |= required;
        if (self.required || supported) && self.challenge.is_none() {
            let issued_at = Utc::now();
            let challenge = IdentityChallenge {
                version: IDENTITY_SCHEMA_VERSION.to_string(),
                connection_id: connection_id.0.to_string(),
                daemon_boot_id: Uuid::now_v7().to_string(),
                audience: format!("daemon/{}", connection_id.0),
                nonce: Uuid::now_v7().to_string(),
                issued_at: issued_at.to_rfc3339_opts(SecondsFormat::Secs, true),
                expires_at: (issued_at + chrono::Duration::seconds(15))
                    .to_rfc3339_opts(SecondsFormat::Secs, true),
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

    pub(crate) fn bind(
        &mut self,
        proof: IdentityProof,
    ) -> Result<IdentityBindResponse, IdentityErrorCode> {
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
        let binding = IdentityBinding {
            binding_id: proof.claims.binding_id.clone(),
            claims: proof.claims,
        };
        self.binding = Some(binding.clone());
        Ok(IdentityBindResponse { binding })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_server_protocol::{IdentityClaims, IdentityKind, IdentityOrigin};

    fn proof(state: &ConnectionIdentityState) -> IdentityProof {
        let challenge = state.challenge().expect("challenge");
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
                daemon_boot_id: challenge.daemon_boot_id,
                connection_id: challenge.connection_id,
                binding_id: "binding".to_string(),
                origin: IdentityOrigin::LocalTui,
                scopes: vec!["thread/start".to_string()],
                issued_at: "2026-09-05T00:00:00Z".to_string(),
                not_before: "2026-09-05T00:00:00Z".to_string(),
                expires_at: "2026-09-06T00:00:00Z".to_string(),
                nonce: challenge.nonce,
                thread_id: None,
                cwd: None,
                live_host: None,
            },
            proof: "authority-proof".to_string(),
        }
    }

    #[test]
    fn required_connection_is_blocked_until_bind() {
        let mut state = ConnectionIdentityState::default();
        state.advertise(
            ConnectionId(7),
            /*required*/ true,
            /*supported*/ true,
        );
        assert!(!state.ready());
    }

    #[test]
    fn matching_proof_binds_once() {
        let mut state = ConnectionIdentityState::default();
        state.advertise(
            ConnectionId(7),
            /*required*/ true,
            /*supported*/ true,
        );
        let response = state.bind(proof(&state)).expect("bind");
        assert_eq!(response.binding.binding_id, "binding");
        assert!(state.ready());
        assert_eq!(state.bind(proof(&state)), Err(IdentityErrorCode::Replay));
    }

    #[test]
    fn wrong_challenge_fails_closed() {
        let mut state = ConnectionIdentityState::default();
        state.advertise(
            ConnectionId(7),
            /*required*/ true,
            /*supported*/ true,
        );
        let mut invalid = proof(&state);
        invalid.challenge.connection_id = "other".to_string();
        assert_eq!(
            state.bind(invalid),
            Err(IdentityErrorCode::AudienceMismatch)
        );
        assert!(!state.ready());
    }
}
