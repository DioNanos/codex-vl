//! Connection-scoped identity messages for the NexusCrew handshake.
//!
//! The wire representation follows the NexusCrew `identity.v1` R7 schema.
//! Verification and issuance remain at the authority/daemon boundary.

use crate::JsonSchema;
use crate::TS;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const IDENTITY_EXTENSION: &str = "nexuscrew.identity.v1";
pub const IDENTITY_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub enum IdentityKind {
    #[serde(rename = "connection-v1")]
    #[ts(rename = "connection-v1")]
    ConnectionV1,
    #[serde(rename = "thread-v1")]
    #[ts(rename = "thread-v1")]
    ThreadV1,
    #[serde(rename = "mcp-v1")]
    #[ts(rename = "mcp-v1")]
    McpV1,
}

impl IdentityKind {
    pub const fn requires_thread(self) -> bool {
        matches!(self, Self::ThreadV1 | Self::McpV1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum IdentityOrigin {
    LocalTui,
    RemoteLive,
    Daemon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "lowercase")]
pub enum IdentityMode {
    Shared,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct IdentityFrom {
    pub instance_id: String,
    pub cell: String,
    pub tmux_session: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct IdentityLiveHost {
    pub owner_instance_id: String,
    pub cell_id: String,
    pub tmux_session: String,
    pub lease_id: String,
    pub generation: u64,
    pub epoch: u64,
    pub designation: String,
    pub pairing_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct IdentityContext {
    pub version: String,
    pub kind: IdentityKind,
    pub verified: bool,
    pub mode: IdentityMode,
    pub origin: IdentityOrigin,
    pub binding_id: String,
    pub owner_instance_id: String,
    pub cell_id: String,
    #[serde(alias = "session")]
    pub tmux_session: String,
    pub from: IdentityFrom,
    pub audience: String,
    pub connection_id: String,
    pub scopes: Vec<String>,
    pub issued_at: String,
    pub not_before: String,
    pub expires_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_host: Option<IdentityLiveHost>,
}

impl IdentityContext {
    pub fn validate(&self) -> Result<(), IdentityErrorCode> {
        if self.version != IDENTITY_SCHEMA_VERSION {
            return Err(IdentityErrorCode::InvalidVersion);
        }
        if !self.verified {
            return Err(IdentityErrorCode::ContextUnverified);
        }
        if self.mode != IdentityMode::Shared {
            return Err(IdentityErrorCode::InvalidMode);
        }
        for value in [
            &self.binding_id,
            &self.owner_instance_id,
            &self.cell_id,
            &self.tmux_session,
            &self.audience,
            &self.connection_id,
        ] {
            if value.trim().is_empty() {
                return Err(IdentityErrorCode::ContextMissing);
            }
        }
        if self.from.instance_id != self.owner_instance_id
            || self.from.cell != self.cell_id
            || self.from.tmux_session != self.tmux_session
        {
            return Err(IdentityErrorCode::ContextFromMismatch);
        }
        if self.scopes.is_empty() || self.scopes.len() > 32 {
            return Err(IdentityErrorCode::InvalidScopes);
        }
        let mut seen = HashSet::with_capacity(self.scopes.len());
        if self
            .scopes
            .iter()
            .any(|scope| scope.trim().is_empty() || !seen.insert(scope))
        {
            return Err(IdentityErrorCode::InvalidScopes);
        }
        if !valid_timestamp(&self.issued_at)
            || !valid_timestamp(&self.not_before)
            || !valid_timestamp(&self.expires_at)
            || self.not_before > self.expires_at
            || self.expires_at <= self.issued_at
        {
            return Err(IdentityErrorCode::InvalidTime);
        }
        if self.kind.requires_thread() && self.thread_id.as_deref().is_none_or(str::is_empty) {
            return Err(IdentityErrorCode::ThreadForbidden);
        }
        if self.origin == IdentityOrigin::RemoteLive && self.live_host.is_none() {
            return Err(IdentityErrorCode::ContextMissing);
        }
        Ok(())
    }
}

fn valid_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes.last() == Some(&b'Z')
        && [0..4, 5..7, 8..10, 11..13, 14..16, 17..19]
            .into_iter()
            .flatten()
            .all(|index| bytes.get(index).is_some_and(u8::is_ascii_digit))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct IdentityChallenge {
    pub version: String,
    pub connection_id: String,
    pub daemon_boot_id: String,
    pub audience: String,
    pub nonce: String,
    pub issued_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct IdentityClaims {
    pub issuer_owner: String,
    pub audience: String,
    pub owner_instance_id: String,
    pub cell_id: String,
    #[serde(alias = "session")]
    pub tmux_session: String,
    pub incarnation_id: String,
    pub launch_epoch: String,
    pub daemon_boot_id: String,
    pub connection_id: String,
    pub binding_id: String,
    pub origin: IdentityOrigin,
    pub scopes: Vec<String>,
    pub issued_at: String,
    pub not_before: String,
    pub expires_at: String,
    pub nonce: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_host: Option<IdentityLiveHost>,
}

impl IdentityClaims {
    pub fn validate(&self) -> Result<(), IdentityErrorCode> {
        for value in [
            &self.issuer_owner,
            &self.audience,
            &self.owner_instance_id,
            &self.cell_id,
            &self.tmux_session,
            &self.incarnation_id,
            &self.launch_epoch,
            &self.daemon_boot_id,
            &self.connection_id,
            &self.binding_id,
            &self.nonce,
        ] {
            if value.trim().is_empty() {
                return Err(IdentityErrorCode::ContextMissing);
            }
        }
        if self.scopes.is_empty() || self.scopes.len() > 32 {
            return Err(IdentityErrorCode::InvalidScopes);
        }
        if !valid_timestamp(&self.issued_at)
            || !valid_timestamp(&self.not_before)
            || !valid_timestamp(&self.expires_at)
            || self.not_before > self.expires_at
            || self.expires_at <= self.issued_at
        {
            return Err(IdentityErrorCode::InvalidTime);
        }
        if self.origin == IdentityOrigin::RemoteLive && self.live_host.is_none() {
            return Err(IdentityErrorCode::ContextMissing);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct IdentityProof {
    pub version: String,
    pub kind: IdentityKind,
    pub challenge: IdentityChallenge,
    pub claims: IdentityClaims,
    /// Opaque authority proof. It is never interpreted by a client.
    pub proof: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct IdentityBindParams {
    pub proof: IdentityProof,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct IdentityBinding {
    pub binding_id: String,
    pub claims: IdentityClaims,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct IdentityBindResponse {
    pub binding: IdentityBinding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub enum IdentityErrorCode {
    #[serde(rename = "IDENTITY_UNVERIFIED")]
    #[ts(rename = "IDENTITY_UNVERIFIED")]
    IdentityUnverified,
    #[serde(rename = "EXPIRED")]
    #[ts(rename = "EXPIRED")]
    Expired,
    #[serde(rename = "REVOKED")]
    #[ts(rename = "REVOKED")]
    Revoked,
    #[serde(rename = "REPLAY")]
    #[ts(rename = "REPLAY")]
    Replay,
    #[serde(rename = "AUDIENCE_MISMATCH")]
    #[ts(rename = "AUDIENCE_MISMATCH")]
    AudienceMismatch,
    #[serde(rename = "THREAD_FORBIDDEN")]
    #[ts(rename = "THREAD_FORBIDDEN")]
    ThreadForbidden,
    #[serde(rename = "AUTHORITY_UNAVAILABLE")]
    #[ts(rename = "AUTHORITY_UNAVAILABLE")]
    AuthorityUnavailable,
    #[serde(rename = "NEXUSCREW_MCP_IDENTITY_CONTEXT_MISSING")]
    #[ts(rename = "NEXUSCREW_MCP_IDENTITY_CONTEXT_MISSING")]
    ContextMissing,
    #[serde(rename = "NEXUSCREW_MCP_IDENTITY_CONTEXT_UNVERIFIED")]
    #[ts(rename = "NEXUSCREW_MCP_IDENTITY_CONTEXT_UNVERIFIED")]
    ContextUnverified,
    #[serde(rename = "NEXUSCREW_MCP_IDENTITY_CONTEXT_FROM_MISMATCH")]
    #[ts(rename = "NEXUSCREW_MCP_IDENTITY_CONTEXT_FROM_MISMATCH")]
    ContextFromMismatch,
    #[serde(rename = "IDENTITY_INVALID_VERSION")]
    #[ts(rename = "IDENTITY_INVALID_VERSION")]
    InvalidVersion,
    #[serde(rename = "IDENTITY_INVALID_MODE")]
    #[ts(rename = "IDENTITY_INVALID_MODE")]
    InvalidMode,
    #[serde(rename = "IDENTITY_INVALID_SCOPES")]
    #[ts(rename = "IDENTITY_INVALID_SCOPES")]
    InvalidScopes,
    #[serde(rename = "IDENTITY_INVALID_TIME")]
    #[ts(rename = "IDENTITY_INVALID_TIME")]
    InvalidTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct IdentityError {
    pub code: IdentityErrorCode,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> IdentityProof {
        IdentityProof {
            version: "1".to_string(),
            kind: IdentityKind::ConnectionV1,
            challenge: IdentityChallenge {
                version: "1".to_string(),
                connection_id: "connection".to_string(),
                daemon_boot_id: "boot".to_string(),
                audience: "daemon/connection".to_string(),
                nonce: "nonce".to_string(),
                issued_at: "2026-09-05T00:00:00Z".to_string(),
                expires_at: "2026-09-05T00:00:15Z".to_string(),
            },
            claims: IdentityClaims {
                issuer_owner: "owner".to_string(),
                audience: "daemon/connection".to_string(),
                owner_instance_id: "owner-instance".to_string(),
                cell_id: "cell".to_string(),
                tmux_session: "session".to_string(),
                incarnation_id: "incarnation".to_string(),
                launch_epoch: "epoch".to_string(),
                daemon_boot_id: "boot".to_string(),
                connection_id: "connection".to_string(),
                binding_id: "binding".to_string(),
                origin: IdentityOrigin::LocalTui,
                scopes: vec!["thread/start".to_string()],
                issued_at: "2026-09-05T00:00:00Z".to_string(),
                not_before: "2026-09-05T00:00:00Z".to_string(),
                expires_at: "2026-09-05T00:01:00Z".to_string(),
                nonce: "nonce".to_string(),
                thread_id: None,
                cwd: None,
                live_host: None,
            },
            proof: "opaque-fixture-proof".to_string(),
        }
    }

    fn context() -> IdentityContext {
        IdentityContext {
            version: "1".to_string(),
            kind: IdentityKind::McpV1,
            verified: true,
            mode: IdentityMode::Shared,
            origin: IdentityOrigin::Daemon,
            binding_id: "binding".to_string(),
            owner_instance_id: "owner".to_string(),
            cell_id: "cell".to_string(),
            tmux_session: "session".to_string(),
            from: IdentityFrom {
                instance_id: "owner".to_string(),
                cell: "cell".to_string(),
                tmux_session: "session".to_string(),
            },
            audience: "mcp".to_string(),
            connection_id: "connection".to_string(),
            scopes: vec!["thread/start".to_string()],
            issued_at: "2026-09-05T00:00:00Z".to_string(),
            not_before: "2026-09-05T00:00:00Z".to_string(),
            expires_at: "2026-09-05T00:01:00Z".to_string(),
            thread_id: Some("thread".to_string()),
            cwd: Some("/tmp".to_string()),
            live_host: None,
        }
    }

    #[test]
    fn identity_schema_round_trip_preserves_binding_claims() {
        let proof = fixture();
        let wire = serde_json::to_value(&proof).expect("serialize identity proof");
        assert_eq!(wire["challenge"]["connectionId"], "connection");
        assert_eq!(wire["kind"], "connection-v1");
        assert_eq!(wire["claims"]["origin"], "local_tui");
        let decoded = serde_json::from_value::<IdentityProof>(wire).expect("decode identity proof");
        assert_eq!(decoded, proof);
    }

    #[test]
    fn identity_context_validates_r7_fields() {
        assert!(context().validate().is_ok());
    }

    #[test]
    fn identity_context_rejects_from_mismatch_and_missing_thread() {
        let mut invalid = context();
        invalid.from.cell = "other".to_string();
        assert_eq!(
            invalid.validate(),
            Err(IdentityErrorCode::ContextFromMismatch)
        );

        let mut invalid = context();
        invalid.thread_id = None;
        assert_eq!(invalid.validate(), Err(IdentityErrorCode::ThreadForbidden));
    }

    #[test]
    fn identity_error_codes_are_stable_on_the_wire() {
        let error = IdentityError {
            code: IdentityErrorCode::ContextFromMismatch,
            message: "identity source does not match context".to_string(),
        };
        let wire = serde_json::to_value(error).expect("serialize identity error");
        assert_eq!(wire["code"], "NEXUSCREW_MCP_IDENTITY_CONTEXT_FROM_MISMATCH");
    }

    #[test]
    fn identity_kind_outside_closed_set_is_rejected() {
        let mut wire = serde_json::to_value(fixture()).expect("serialize identity proof");
        wire["kind"] = serde_json::Value::String("unknown-v1".to_string());
        assert!(serde_json::from_value::<IdentityProof>(wire).is_err());
    }

    #[test]
    fn identity_context_rejects_expired_and_not_before_future_windows() {
        let mut expired = context();
        expired.expires_at = "2026-09-04T23:59:59Z".to_string();
        assert_eq!(expired.validate(), Err(IdentityErrorCode::InvalidTime));

        let mut future = context();
        future.not_before = "2026-09-06T00:00:00Z".to_string();
        assert_eq!(future.validate(), Err(IdentityErrorCode::InvalidTime));
    }
}
