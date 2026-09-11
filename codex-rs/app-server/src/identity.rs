use chrono::{SecondsFormat, Utc};
use codex_app_server_protocol::{
    IDENTITY_SCHEMA_VERSION, IdentityBindResponse, IdentityBinding, IdentityChallenge,
    IdentityErrorCode, IdentityProof,
};
use std::ffi::OsStr;
use uuid::Uuid;

#[cfg(feature = "d174-test-fixture")]
use std::io::Write;

use crate::outgoing_message::ConnectionId;

/// Environment variable through which the launcher hands the daemon the
/// inherited descriptor pair used to have identity proofs checked against the
/// authority that started it.
const IDENTITY_VERIFIER_FD_ENV: &str = "NEXUSCREW_IDENTITY_FD";

/// Whether this daemon can reach the authority that verifies identity proofs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerifierChannel {
    /// A descriptor pair was declared and both descriptors are open.
    Available,
    /// The launcher declared no descriptor pair.
    Absent,
    /// A descriptor pair was declared but cannot be used; the payload explains why.
    Unusable(&'static str),
}

fn parse_verifier_fd_spec(spec: &OsStr) -> Result<(i32, i32), &'static str> {
    let spec = spec.to_str().ok_or("descriptor spec is not valid UTF-8")?;
    let (write, read) = spec
        .split_once(':')
        .ok_or("descriptor spec is not \"<write>:<read>\"")?;
    let write = write
        .parse::<i32>()
        .map_err(|_| "write descriptor is not a number")?;
    let read = read
        .parse::<i32>()
        .map_err(|_| "read descriptor is not a number")?;
    if write < 3 || read < 3 || write == read {
        return Err("descriptor spec does not name two distinct inherited descriptors");
    }
    Ok((write, read))
}

/// Classify a declared descriptor pair. `is_open` is injected so the decision can
/// be exercised without touching the process descriptor table.
pub(crate) fn verifier_channel(
    spec: Option<&OsStr>,
    is_open: impl Fn(i32) -> bool,
) -> VerifierChannel {
    let Some(spec) = spec else {
        return VerifierChannel::Absent;
    };
    match parse_verifier_fd_spec(spec) {
        Err(reason) => VerifierChannel::Unusable(reason),
        Ok((write, read)) if is_open(write) && is_open(read) => VerifierChannel::Available,
        Ok(_) => VerifierChannel::Unusable("inherited descriptors are not open"),
    }
}

/// Look for the verifier channel the launcher left behind.
///
/// This probes the environment because the daemon does not own a verifier handle
/// yet. Once the daemon captures the channel itself, this probe must be replaced
/// by a check on that captured handle: capturing clears the variable, and an
/// environment probe would then report a false absence.
pub(crate) fn verifier_channel_from_env() -> VerifierChannel {
    verifier_channel(
        std::env::var_os(IDENTITY_VERIFIER_FD_ENV).as_deref(),
        descriptor_is_open,
    )
}

#[cfg(unix)]
fn descriptor_is_open(fd: i32) -> bool {
    // Probe only: ownership of the descriptors stays with whoever opens the channel.
    unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
}

#[cfg(not(unix))]
fn descriptor_is_open(_fd: i32) -> bool {
    false
}

/// Identity enforcement is only meaningful when proofs can actually be checked.
///
/// With no reachable verifier the daemon could never accept a proof, so every
/// client would be rejected for good; it then serves connections the way it does
/// when the launcher asks for nothing, and says so once at startup.
pub(crate) fn effective_identity_required(
    required_by_launcher: bool,
    channel: VerifierChannel,
) -> bool {
    if !required_by_launcher {
        return false;
    }
    match channel {
        VerifierChannel::Available => true,
        VerifierChannel::Absent => {
            tracing::warn!(
                "identity was requested but no verifier channel was inherited \
                 ({IDENTITY_VERIFIER_FD_ENV} is unset); serving connections without identity \
                 enforcement"
            );
            false
        }
        VerifierChannel::Unusable(reason) => {
            tracing::warn!(
                "identity was requested but the inherited verifier channel is unusable \
                 ({reason}); serving connections without identity enforcement"
            );
            false
        }
    }
}

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
    use std::ffi::OsString;
    use std::sync::Arc;
    use std::sync::Mutex;
    use tracing_subscriber::layer::SubscriberExt;

    /// Records the warning messages emitted while the subscriber is installed.
    #[derive(Clone, Default)]
    struct WarningLog(Arc<Mutex<Vec<String>>>);

    impl WarningLog {
        fn messages(&self) -> Vec<String> {
            self.0.lock().expect("warning log").clone()
        }
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for WarningLog {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if *event.metadata().level() != tracing::Level::WARN {
                return;
            }
            let mut message = MessageVisitor(String::new());
            event.record(&mut message);
            self.0.lock().expect("warning log").push(message.0);
        }
    }

    struct MessageVisitor(String);

    impl tracing::field::Visit for MessageVisitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.0 = format!("{value:?}");
            }
        }
    }

    fn required_with(required: bool, channel: VerifierChannel) -> (bool, Vec<String>) {
        let log = WarningLog::default();
        let subscriber = tracing_subscriber::registry().with(log.clone());
        let effective = tracing::subscriber::with_default(subscriber, || {
            effective_identity_required(required, channel)
        });
        (effective, log.messages())
    }

    #[test]
    fn requested_identity_without_verifier_serves_and_warns_once() {
        let (effective, warnings) = required_with(true, VerifierChannel::Absent);
        assert!(
            !effective,
            "a daemon that cannot check proofs must not demand them"
        );
        assert_eq!(warnings.len(), 1, "warnings: {warnings:?}");
        assert!(
            warnings[0].contains("NEXUSCREW_IDENTITY_FD"),
            "the warning must name the missing channel: {warnings:?}"
        );
    }

    #[test]
    fn requested_identity_with_unusable_verifier_serves_and_warns_once() {
        let (effective, warnings) =
            required_with(true, VerifierChannel::Unusable("descriptors are on fire"));
        assert!(!effective);
        assert_eq!(warnings.len(), 1, "warnings: {warnings:?}");
        assert!(
            warnings[0].contains("descriptors are on fire"),
            "the warning must carry the reason: {warnings:?}"
        );
    }

    #[test]
    fn requested_identity_with_verifier_stays_enforced() {
        let (effective, warnings) = required_with(true, VerifierChannel::Available);
        assert!(
            effective,
            "a reachable verifier keeps the daemon fail-closed"
        );
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn identity_not_requested_is_never_enforced() {
        for channel in [
            VerifierChannel::Available,
            VerifierChannel::Absent,
            VerifierChannel::Unusable("whatever"),
        ] {
            let (effective, warnings) = required_with(false, channel);
            assert!(!effective, "channel: {channel:?}");
            assert!(warnings.is_empty(), "channel: {channel:?} {warnings:?}");
        }
    }

    #[test]
    fn undeclared_channel_is_absent() {
        assert_eq!(verifier_channel(None, |_| true), VerifierChannel::Absent);
    }

    #[test]
    fn malformed_specs_are_unusable() {
        for spec in ["", "3", "3:", ":4", "a:b", "3:3", "2:4", "3:2", "-1:4"] {
            let spec = OsString::from(spec);
            assert!(
                matches!(
                    verifier_channel(Some(spec.as_os_str()), |_| true),
                    VerifierChannel::Unusable(_)
                ),
                "spec {spec:?} must not pass for a verifier channel"
            );
        }
    }

    #[test]
    fn declared_but_closed_descriptors_are_unusable() {
        let spec = OsString::from("7:8");
        assert!(matches!(
            verifier_channel(Some(spec.as_os_str()), |_| false),
            VerifierChannel::Unusable(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn open_descriptors_are_available_against_the_real_descriptor_table() {
        let mut fds = [0i32; 2];
        assert_eq!(
            unsafe { libc::pipe(fds.as_mut_ptr()) },
            0,
            "the test needs a real descriptor pair"
        );
        let spec = OsString::from(format!("{}:{}", fds[1], fds[0]));
        let channel = verifier_channel(Some(spec.as_os_str()), descriptor_is_open);
        // A descriptor number far above any the process can have allocated.
        let closed = OsString::from("1073741823:1073741822");
        let closed_channel = verifier_channel(Some(closed.as_os_str()), descriptor_is_open);
        for fd in fds {
            unsafe { libc::close(fd) };
        }
        assert_eq!(channel, VerifierChannel::Available);
        assert!(matches!(closed_channel, VerifierChannel::Unusable(_)));
    }

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
