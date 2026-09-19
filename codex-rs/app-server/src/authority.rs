//! Real authority-side verifier of the identity proof over the fd3/fd4
//! channel inherited from the cell supervisor (`NEXUSCREW_IDENTITY_FD=3:4`)
//! using the `nexuscrew/identity/verify` v1 method (spec:
//! `docs/identity/verify-channel-v1.md`).
//!
//! Fail-closed: authority absent, timeout (4000 ms), `verify-unsupported`
//! (server without a verifier) or any `ok:false` → `AuthorityRefusal`; the bind
//! refuses (IdentityUnverified). The HMAC key never leaves the authority.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use codex_app_server_protocol::{IdentityChallenge, IdentityProof};
use serde_json::{Value, json};

use crate::identity::{AuthorityRefusal, AuthorityVerifier, VerifiedIdentityClaims};
use crate::identity_channel::IdentityFdChannel;
#[cfg(test)]
use crate::identity_channel::parse_identity_fd_spec as parse_fd_spec;

const VERIFY_TIMEOUT_MS: u64 = 4000;
const VERIFY_METHOD: &str = "nexuscrew/identity/verify";

/// Client of the verify v1 channel. Cloneable (shared fds, one instance per
/// process: dropping the last copy closes the inherited fds).
#[derive(Clone)]
pub struct FdAuthorityVerifier {
    inner: IdentityFdChannel,
}

/// Startup decision for identity enforcement.
///
/// Reads the launcher flag once, captures the inherited channel once, and
/// returns the enforcement latch together with the verifier. Under an identity
/// request an absent or unusable declaration fails closed here, before any
/// listener is opened.
pub(crate) fn authority_verifier_for_startup()
-> std::io::Result<(bool, Option<Arc<dyn AuthorityVerifier>>)> {
    authority_verifier_for_startup_with(/*shared_channel*/ None)
}

/// Startup decision when the caller already owns a channel captured in this process.
///
/// An embedded app-server runs in the process that captured the inherited
/// descriptors for the client side, and that capture removes the declaration from
/// the environment: re-reading it here would refuse a channel this process already
/// owns. A shared channel is therefore authoritative and the environment is not
/// consulted; without a shared channel the fail-closed capture below is unchanged.
pub(crate) fn authority_verifier_for_startup_with(
    shared_channel: Option<IdentityFdChannel>,
) -> std::io::Result<(bool, Option<Arc<dyn AuthorityVerifier>>)> {
    let required = crate::identity::identity_required_from_env();
    if let Some(channel) = shared_channel {
        if !required {
            return Ok((false, None));
        }
        return Ok((
            true,
            Some(Arc::new(FdAuthorityVerifier::from_channel(channel)) as Arc<dyn AuthorityVerifier>),
        ));
    }
    match crate::identity_channel::capture_identity_channel(required)? {
        crate::identity_channel::CapturedIdentityChannel::Standalone => Ok((false, None)),
        crate::identity_channel::CapturedIdentityChannel::Ready(channel) => Ok((
            true,
            Some(Arc::new(FdAuthorityVerifier::from_channel(channel)) as Arc<dyn AuthorityVerifier>),
        )),
    }
}

impl FdAuthorityVerifier {
    /// Opens the fds declared by `NEXUSCREW_IDENTITY_FD` ("<w>:<r>", e.g. "3:4").
    /// `None` when the variable is absent (standalone: no verifier).
    pub fn open_from_env() -> std::io::Result<Option<Self>> {
        IdentityFdChannel::from_env().map(|channel| channel.map(|inner| Self { inner }))
    }

    /// Build a verifier from a channel the startup already captured and owns.
    pub(crate) fn from_channel(channel: IdentityFdChannel) -> Self {
        Self { inner: channel }
    }

    #[cfg(test)]
    fn open_from_spec(spec: Option<&str>) -> std::io::Result<Option<Self>> {
        let Some((w_fd, r_fd)) = parse_fd_spec(spec.map(std::ffi::OsStr::new))? else {
            return Ok(None);
        };
        Self::open_from_fds(w_fd, r_fd).map(Some)
    }

    #[cfg(test)]
    fn open_from_fds(w_fd: i32, r_fd: i32) -> std::io::Result<Self> {
        use std::os::fd::FromRawFd;
        // Dup of the inherited fds (ManuallyDrop avoids closing the original).
        let out_file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(w_fd) });
        let in_file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(r_fd) });
        Ok(Self {
            inner: IdentityFdChannel::from_files(out_file.try_clone()?, in_file.try_clone()?)?,
        })
    }
}

impl AuthorityVerifier for FdAuthorityVerifier {
    fn verify<'a>(
        &'a self,
        proof: &'a IdentityProof,
        challenge: &'a IdentityChallenge,
    ) -> Pin<Box<dyn Future<Output = Result<VerifiedIdentityClaims, AuthorityRefusal>> + Send + 'a>>
    {
        let envelope = match serde_json::from_str::<Value>(&proof.proof) {
            Ok(value) if value.is_object() => value,
            _ => {
                tracing::warn!("identity proof opaque field is not a JSON object");
                return Box::pin(async { Err(AuthorityRefusal::Malformed) });
            }
        };
        let Some(generation) = envelope.get("generation").and_then(Value::as_u64) else {
            tracing::warn!("identity proof opaque envelope has no numeric lease generation");
            return Box::pin(async { Err(AuthorityRefusal::Malformed) });
        };
        let Some(raw_proof) = envelope
            .get("proof")
            .filter(|value| value.is_object())
            .cloned()
        else {
            tracing::warn!("identity proof opaque envelope has no raw NC proof");
            return Box::pin(async { Err(AuthorityRefusal::Malformed) });
        };
        // The `expected` tuple is always derived from the challenge this
        // daemon received in initialize, never from client input. The
        // authority answers `challenge_mismatch` when the proof does not
        // match it.
        let expected = json!({
            "nonce": challenge.nonce,
            "connectionId": challenge.connection_id,
            "daemonBootId": challenge.daemon_boot_id,
            "audience": challenge.audience,
        });
        Box::pin(async move {
            // A refusal is a RESPONSE, not a transport fault. The channel is
            // invalidated only when the exchange itself did not happen
            // (timeout, EOF, invalid JSON, WriteZero, oversized response):
            // in those cases the descriptors are in an unknown state and can
            // no longer be trusted. On `ok:false` or a JSON-RPC error the
            // channel stays ALIVE: tearing it down used to disable identity
            // verification for the whole process, so a single forged proof
            // (or one that expired during a slow startup) made the daemon
            // unusable until restart — and that teardown is irreversible
            // because `NEXUSCREW_IDENTITY_FD` was removed from the
            // environment at startup.
            let response = match self
                .inner
                .request(
                    VERIFY_METHOD,
                    json!({"v":1, "generation":generation, "proof":raw_proof, "expected":expected}),
                    Duration::from_millis(VERIFY_TIMEOUT_MS),
                )
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    self.inner.invalidate();
                    return Err(match error.kind() {
                        std::io::ErrorKind::InvalidData => AuthorityRefusal::Malformed,
                        _ => AuthorityRefusal::Unreachable,
                    });
                }
            };
            if let Some(error) = response.get("error") {
                Err(AuthorityRefusal::Rejected(canonical_reason(
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("malformed"),
                )))
            } else if let Some(result) = response.get("result") {
                map_verify_result(result, challenge, generation)
            } else {
                Err(AuthorityRefusal::Malformed)
            }
        })
    }
}

fn canonical_reason(reason: &str) -> &'static str {
    match reason {
        "expired" => "expired",
        "bad-proof" => "bad-proof",
        "replay" => "replay",
        "revoked" => "revoked",
        "malformed" => "malformed",
        // `challenge_mismatch` is a closed-enum reason and must be
        // preserved: the bind maps it to AudienceMismatch instead of the
        // generic IdentityUnverified.
        "challenge_mismatch" => "challenge_mismatch",
        _ => "rejected",
    }
}

/// Pure mapping of a verify result to verified claims. The claims in the
/// response are the exclusive source of the binding: nothing is merged from
/// the external claims of the signed proof. Pure, so it is testable without
/// I/O.
pub(crate) fn map_verify_result(
    result: &Value,
    challenge: &IdentityChallenge,
    generation: u64,
) -> Result<VerifiedIdentityClaims, AuthorityRefusal> {
    let ok = result.get("ok").and_then(Value::as_bool).unwrap_or(false);
    if !ok {
        let reason = result
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("malformed");
        return match reason {
            "timeout" | "authority-unavailable" | "verify-unsupported" => {
                Err(AuthorityRefusal::Unreachable)
            }
            other => Err(AuthorityRefusal::Rejected(canonical_reason(other))),
        };
    }
    let Some(normalized) = result.get("claims").filter(|claims| claims.is_object()) else {
        return Err(AuthorityRefusal::Malformed);
    };
    // The claims in the response are the exclusive source of the binding;
    // nothing is merged from the external claims of the proof. Before
    // returning, the confirmed claims are checked against the challenge this
    // daemon expected, and any divergence fails closed.
    let mut normalized = normalized.clone();
    if let Some(object) = normalized.as_object_mut() {
        object.insert("generation".to_string(), Value::from(generation));
    }
    let verified: crate::identity::VerifiedIdentityClaims = serde_json::from_value(normalized)
        .map_err(|error| {
            tracing::warn!(
                ?error,
                "identity verify result claims did not match VL mapping"
            );
            AuthorityRefusal::Malformed
        })?;
    if verified.nonce != challenge.nonce
        || verified.connection_id != challenge.connection_id
        || verified.daemon_boot_id != challenge.daemon_boot_id
        || verified.audience != challenge.audience
    {
        tracing::warn!("identity verify claims diverge from the expected challenge tuple");
        return Err(AuthorityRefusal::Rejected("challenge_mismatch"));
    }
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_reason_keeps_challenge_mismatch_distinct() {
        // Without this arm `challenge_mismatch` fell back to "rejected" and
        // the bind could not distinguish it from a generic refusal.
        assert_eq!(canonical_reason("challenge_mismatch"), "challenge_mismatch");
        assert_eq!(canonical_reason("bad-proof"), "bad-proof");
        assert_eq!(canonical_reason("qualcosa-di-nuovo"), "rejected");
    }

    #[test]
    fn verifier_fd_parser_rejects_non_utf8() {
        use std::os::unix::ffi::OsStrExt;
        assert!(parse_fd_spec(Some(std::ffi::OsStr::from_bytes(b"3:\xff"))).is_err());
    }

    #[test]
    fn verifier_fd_parser_rejects_reserved() {
        for spec in ["0:3", "3:0", "1:3", "3:1", "2:3", "3:2"] {
            assert!(
                parse_fd_spec(Some(std::ffi::OsStr::new(spec))).is_err(),
                "{spec}"
            );
        }
    }

    #[test]
    fn verifier_fd_parser_rejects_duplicates() {
        assert!(parse_fd_spec(Some(std::ffi::OsStr::new("3:3"))).is_err());
    }

    #[test]
    fn verifier_fd_parser_rejects_negative() {
        for spec in ["-1:4", "3:-1", "-2:4", "3:-2"] {
            assert!(
                parse_fd_spec(Some(std::ffi::OsStr::new(spec))).is_err(),
                "{spec}"
            );
        }
    }

    #[test]
    fn verifier_fd_parser_rejects_non_numeric() {
        for spec in ["x:4", "3:x", "3:", ":4", "3:4:5", "2147483648:4"] {
            assert!(
                parse_fd_spec(Some(std::ffi::OsStr::new(spec))).is_err(),
                "{spec}"
            );
        }
    }

    #[test]
    fn verifier_fd_spec_absent_is_none() {
        assert!(
            FdAuthorityVerifier::open_from_spec(None)
                .expect("absent fd spec")
                .is_none()
        );
    }

    #[test]
    fn verifier_fd_spec_present_opens_both_descriptors() {
        use std::os::fd::{AsRawFd, FromRawFd};

        // The declared channel must be a pipe or socket pair: a regular file is
        // refused by the startup validation, so the fixture has to be honest.
        let mut fds = [0i32; 2];
        assert_eq!(
            unsafe { libc::pipe(fds.as_mut_ptr()) },
            0,
            "the test needs a real descriptor pair"
        );
        let output = unsafe { std::fs::File::from_raw_fd(fds[1]) };
        let input = unsafe { std::fs::File::from_raw_fd(fds[0]) };
        let spec = format!("{}:{}", output.as_raw_fd(), input.as_raw_fd());
        assert!(
            FdAuthorityVerifier::open_from_spec(Some(&spec))
                .expect("valid fd spec")
                .is_some()
        );
    }

    #[test]
    fn verifier_fd_spec_malformed_fails_closed() {
        match FdAuthorityVerifier::open_from_spec(Some("not-a-pair")) {
            Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput),
            Ok(_) => panic!("malformed fd spec must fail closed"),
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "authority_channel_tests.rs"]
mod channel_tests;
