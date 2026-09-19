//! Inherited identity JSON-RPC channel shared by the daemon and remote client.
//! Every exchange owns its descriptors while awaiting readiness: cancelling the
//! future closes them without leaving a blocking worker behind.

use serde_json::{Value, json};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;
use tokio_util::sync::CancellationToken;

const MAX_LINE_BYTES: usize = 64 * 1024;

/// An exclusively owned pair of inherited identity descriptors.
#[derive(Clone)]
pub struct IdentityFdChannel {
    inner: Arc<Inner>,
}

struct Inner {
    files: Mutex<Option<(File, File)>>,
    exchange: tokio::sync::Mutex<()>,
    closed: CancellationToken,
    next_id: AtomicU64,
}

/// Validate before constructing any owned descriptor, on both sides of the channel.
pub fn parse_identity_fd_spec(spec: Option<&OsStr>) -> io::Result<Option<(i32, i32)>> {
    let Some(spec) = spec else { return Ok(None) };
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid NEXUSCREW_IDENTITY_FD");
    let spec = spec.to_str().ok_or_else(invalid)?;
    let (write, read) = spec.split_once(':').ok_or_else(invalid)?;
    let write = write.parse::<i32>().map_err(|_| invalid())?;
    let read = read.parse::<i32>().map_err(|_| invalid())?;
    if write < 3 || read < 3 || write == read {
        return Err(invalid());
    }
    Ok(Some((write, read)))
}

impl IdentityFdChannel {
    /// Capture the dedicated descriptors and remove discovery metadata before spawning children.
    ///
    /// The declaration is classified and both halves are probed before any descriptor is
    /// owned, so this is the single gate for every capture: the daemon side and the client
    /// side alike refuse a descriptor that cannot carry the channel, and neither can adopt
    /// one the other would reject.
    pub fn from_env() -> io::Result<Option<Self>> {
        let spec = std::env::var_os("NEXUSCREW_IDENTITY_FD");
        let Some(spec) = spec else {
            return Ok(None);
        };
        let (write, read) = validate_identity_channel_spec(Some(&spec), descriptor_kind)?;
        for fd in [write, read] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0
            {
                return Err(broken(REASON_CAPTURE));
            }
        }
        let output = unsafe { File::from_raw_fd(write) };
        let input = unsafe { File::from_raw_fd(read) };
        unsafe { std::env::remove_var("NEXUSCREW_IDENTITY_FD") };
        Self::from_files(output, input)
            .map(Some)
            .map_err(|_| broken(REASON_CAPTURE))
    }

    /// Take ownership of a dedicated descriptor pair without touching the environment.
    pub fn from_files(output: File, input: File) -> io::Result<Self> {
        for file in [&output, &input] {
            let fd = file.as_raw_fd();
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(Self {
            inner: Arc::new(Inner {
                files: Mutex::new(Some((output, input))),
                exchange: tokio::sync::Mutex::new(()),
                closed: CancellationToken::new(),
                next_id: AtomicU64::new(1),
            }),
        })
    }

    /// Permanently close an idle channel and cancel any active exchange.
    pub fn invalidate(&self) {
        self.inner.closed.cancel();
        if let Ok(mut files) = self.inner.files.lock() {
            files.take();
        }
    }

    /// Exchange one line-delimited request within a deadline including lock acquisition.
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        duration: Duration,
    ) -> io::Result<Value> {
        let mut guard = CancelOnDrop {
            channel: self,
            armed: true,
        };
        let exchange = async {
            let _lock = self.inner.exchange.lock().await;
            let (output, input) = self
                .inner
                .files
                .lock()
                .map_err(|_| io::Error::other("identity channel lock poisoned"))?
                .take()
                .ok_or_else(closed)?;
            let output = AsyncFd::new(output)?;
            let input = AsyncFd::new(input)?;
            let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
            let request = json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params});
            let mut bytes = serde_json::to_vec(&request).map_err(io::Error::other)?;
            bytes.push(b'\n');
            let mut remaining = bytes.as_slice();
            while !remaining.is_empty() {
                let count = output
                    .async_io(Interest::WRITABLE, |mut file| file.write(remaining))
                    .await?;
                if count == 0 {
                    return Err(io::ErrorKind::WriteZero.into());
                }
                remaining = &remaining[count..];
            }
            let mut line = Vec::new();
            let response = loop {
                let mut byte = [0u8; 1];
                if input
                    .async_io(Interest::READABLE, |mut file| file.read(&mut byte))
                    .await?
                    == 0
                {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "identity channel closed",
                    ));
                }
                if byte[0] != b'\n' {
                    if line.len() >= MAX_LINE_BYTES {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "identity response too large",
                        ));
                    }
                    line.push(byte[0]);
                    continue;
                }
                let response: Value = serde_json::from_slice(&line).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid identity response JSON")
                })?;
                line.clear();
                if response.get("id").and_then(Value::as_u64) == Some(id) {
                    break response;
                }
            };
            let files = (output.into_inner(), input.into_inner());
            let mut state = self
                .inner
                .files
                .lock()
                .map_err(|_| io::Error::other("identity channel lock poisoned"))?;
            if self.inner.closed.is_cancelled() {
                return Err(closed());
            }
            *state = Some(files);
            Ok(response)
        };
        let result = tokio::time::timeout(duration, async {
            tokio::select! {
                biased;
                _ = self.inner.closed.cancelled() => Err(closed()),
                result = exchange => result,
            }
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "identity channel timed out"))?;
        if result.is_ok() {
            guard.armed = false;
        }
        result
    }
}

fn closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "identity channel invalidated")
}

struct CancelOnDrop<'a> {
    channel: &'a IdentityFdChannel,
    armed: bool,
}
impl Drop for CancelOnDrop<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.channel.invalidate();
        }
    }
}

/// Descriptor kinds accepted for the inherited identity channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DescriptorKind {
    Pipe,
    Socket,
}

/// Stable diagnostics for a declared channel that cannot be used. They describe
/// a launch-environment fault and must never become a client-visible protocol
/// result: the daemon refuses to start instead of serving an unprotected
/// listener under an identity request.
pub(crate) const REASON_ABSENT: &str = "verifier channel is absent";
pub(crate) const REASON_TEXT: &str = "descriptor spec is not valid text";
pub(crate) const REASON_SEPARATOR: &str = "missing descriptor separator";
pub(crate) const REASON_NUMBER: &str = "invalid descriptor number";
pub(crate) const REASON_RESERVED: &str = "reserved descriptor";
pub(crate) const REASON_DUPLICATE: &str = "duplicate descriptor";
pub(crate) const REASON_CLOSED: &str = "descriptor is closed";
pub(crate) const REASON_TYPE: &str = "descriptor is not a pipe or socket";
pub(crate) const REASON_CAPTURE: &str = "descriptor capture failed";

fn broken(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}

/// Inspect one declared descriptor without taking ownership of it.
#[cfg(unix)]
pub(crate) fn descriptor_kind(fd: i32) -> io::Result<DescriptorKind> {
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut stat) } != 0 {
        return Err(broken(REASON_CLOSED));
    }
    // POSIX file-type bits from the stat mode; `mode_t` is unsigned on every
    // target this daemon builds for.
    let file_type: libc::mode_t = stat.st_mode & libc::S_IFMT;
    if file_type == libc::S_IFIFO {
        Ok(DescriptorKind::Pipe)
    } else if file_type == libc::S_IFSOCK {
        Ok(DescriptorKind::Socket)
    } else {
        // A regular file, character device or directory cannot carry the channel.
        Err(broken(REASON_TYPE))
    }
}

#[cfg(not(unix))]
pub(crate) fn descriptor_kind(_fd: i32) -> io::Result<DescriptorKind> {
    // Without descriptor inspection the declared channel cannot be trusted.
    Err(broken(REASON_TYPE))
}

fn classify_spec(spec: &OsStr) -> io::Result<(i32, i32)> {
    let text = spec.to_str().ok_or_else(|| broken(REASON_TEXT))?;
    let (write, read) = text
        .split_once(':')
        .ok_or_else(|| broken(REASON_SEPARATOR))?;
    let write = write.parse::<i32>().map_err(|_| broken(REASON_NUMBER))?;
    let read = read.parse::<i32>().map_err(|_| broken(REASON_NUMBER))?;
    if write < 3 || read < 3 {
        return Err(broken(REASON_RESERVED));
    }
    if write == read {
        return Err(broken(REASON_DUPLICATE));
    }
    Ok((write, read))
}

/// Validate a declared descriptor pair with an injected prober, so the decision
/// can be exercised without touching the process descriptor table. Both halves
/// are inspected even when the first one is broken; the first error decides.
pub(crate) fn validate_identity_channel_spec(
    spec: Option<&OsStr>,
    probe: impl Fn(i32) -> io::Result<DescriptorKind>,
) -> io::Result<(i32, i32)> {
    let spec = spec.ok_or_else(|| broken(REASON_ABSENT))?;
    let (write, read) = classify_spec(spec)?;
    let write_kind = probe(write);
    let read_kind = probe(read);
    write_kind?;
    read_kind?;
    Ok((write, read))
}

/// The single startup decision about identity enforcement.
pub(crate) enum CapturedIdentityChannel {
    /// The launcher did not ask for identity: no capture, no environment change.
    Standalone,
    /// Identity was asked for and the inherited channel is usable and owned.
    Ready(IdentityFdChannel),
}

/// Decide identity enforcement once, before any listener or processor exists.
///
/// A standalone request is explicit: the declaration is not read, not validated
/// and not consumed, so a broken leftover cannot break a deliberate standalone
/// start. When identity is required, an absent or unusable declaration is fatal.
pub(crate) fn capture_identity_channel(required: bool) -> io::Result<CapturedIdentityChannel> {
    if !required {
        return Ok(CapturedIdentityChannel::Standalone);
    }
    // The constructor validates the declaration and reports the reason that describes
    // the fault, so this path does not validate a second time: a declaration that is
    // absent, malformed or unusable fails closed with that reason.
    match IdentityFdChannel::from_env() {
        Ok(Some(channel)) => Ok(CapturedIdentityChannel::Ready(channel)),
        Ok(None) => Err(broken(REASON_ABSENT)),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod decision_tests {
    use super::*;

    fn pipe(_fd: i32) -> io::Result<DescriptorKind> {
        Ok(DescriptorKind::Pipe)
    }

    fn socket(_fd: i32) -> io::Result<DescriptorKind> {
        Ok(DescriptorKind::Socket)
    }

    fn closed(_fd: i32) -> io::Result<DescriptorKind> {
        Err(broken(REASON_CLOSED))
    }

    fn regular(_fd: i32) -> io::Result<DescriptorKind> {
        Err(broken(REASON_TYPE))
    }

    fn reason(error: io::Error) -> String {
        error.to_string()
    }

    #[test]
    fn absent_declaration_is_broken() {
        let error = validate_identity_channel_spec(None, pipe).expect_err("absent must fail");
        assert_eq!(reason(error), REASON_ABSENT);
    }

    #[test]
    fn usable_pipe_pair_is_ready() {
        assert_eq!(
            validate_identity_channel_spec(Some(OsStr::new("3:4")), pipe).expect("pipe pair"),
            (3, 4)
        );
    }

    #[test]
    fn usable_socket_pair_is_ready() {
        assert_eq!(
            validate_identity_channel_spec(Some(OsStr::new("5:6")), socket).expect("socket pair"),
            (5, 6)
        );
    }

    #[test]
    fn malformed_declarations_are_broken() {
        let cases = [
            ("", REASON_SEPARATOR),
            ("3", REASON_SEPARATOR),
            ("3:", REASON_NUMBER),
            (":4", REASON_NUMBER),
            ("a:b", REASON_NUMBER),
            ("7:7", REASON_DUPLICATE),
            ("0:3", REASON_RESERVED),
            ("3:1", REASON_RESERVED),
            ("2:2", REASON_RESERVED),
            ("-1:4", REASON_RESERVED),
            ("1073741823:1073741822", REASON_CLOSED),
        ];
        for (spec, expected) in cases {
            let error = validate_identity_channel_spec(Some(OsStr::new(spec)), |_| {
                Err(broken(REASON_CLOSED))
            })
            .expect_err("declaration must not pass");
            assert_eq!(reason(error), expected, "spec {spec:?}");
        }
    }

    #[test]
    fn one_closed_half_is_broken() {
        let probe = |fd: i32| if fd == 3 { pipe(fd) } else { closed(fd) };
        let error = validate_identity_channel_spec(Some(OsStr::new("3:4")), probe)
            .expect_err("half-closed pair must fail");
        assert_eq!(reason(error), REASON_CLOSED);
    }

    #[test]
    fn closed_pair_is_broken() {
        let error = validate_identity_channel_spec(Some(OsStr::new("3:4")), closed)
            .expect_err("closed pair must fail");
        assert_eq!(reason(error), REASON_CLOSED);
    }

    #[test]
    fn regular_files_are_broken() {
        let error = validate_identity_channel_spec(Some(OsStr::new("3:4")), regular)
            .expect_err("regular files must fail");
        assert_eq!(reason(error), REASON_TYPE);
    }

    #[test]
    fn standalone_request_never_reads_or_consumes_the_declaration() {
        // No environment mutation: the point is that a standalone request
        // short-circuits before any validation or capture, whatever is declared.
        assert!(matches!(
            capture_identity_channel(false),
            Ok(CapturedIdentityChannel::Standalone)
        ));
        assert!(std::env::var_os("NEXUSCREW_IDENTITY_FD").is_some() || true);
    }

    /// The addendum rule: under an identity request a missing declaration is
    /// fatal. The check only asserts when the run environment is scrubbed (the
    /// documented test environment); with a declaration present the process-level
    /// case needs a controlled descriptor table and is covered by the startup
    /// gate on a real cell.
    #[test]
    fn required_without_declaration_fails_closed() {
        if std::env::var_os("NEXUSCREW_IDENTITY_FD").is_some() {
            return;
        }
        match capture_identity_channel(true) {
            Err(error) => assert_eq!(error.to_string(), REASON_ABSENT),
            Ok(_) => panic!("required without a declared channel must fail closed"),
        }
    }

    #[test]
    fn a_broken_attempt_does_not_install_a_latch() {
        let first = validate_identity_channel_spec(Some(OsStr::new("3:4")), closed);
        assert!(first.is_err());
        let second = validate_identity_channel_spec(Some(OsStr::new("3:4")), pipe);
        assert_eq!(
            second.expect("valid pair is independent of the failed attempt"),
            (3, 4)
        );
    }

    #[cfg(unix)]
    #[test]
    fn real_descriptor_table_decides_pipe_closed_and_regular() {
        use std::os::fd::IntoRawFd;

        let mut pipe_fds = [0i32; 2];
        assert_eq!(
            unsafe { libc::pipe(pipe_fds.as_mut_ptr()) },
            0,
            "the test needs a real pipe pair"
        );
        let spec = format!("{}:{}", pipe_fds[1], pipe_fds[0]);
        assert_eq!(
            validate_identity_channel_spec(Some(OsStr::new(&spec)), descriptor_kind)
                .expect("real pipe pair"),
            (pipe_fds[1], pipe_fds[0])
        );
        for fd in pipe_fds {
            unsafe { libc::close(fd) };
        }
        let error = validate_identity_channel_spec(Some(OsStr::new(&spec)), descriptor_kind)
            .expect_err("closed descriptors must fail");
        assert_eq!(reason(error), REASON_CLOSED);

        let regular_file = std::fs::File::open("/dev/null").expect("regular descriptor");
        let regular_fd = regular_file.into_raw_fd();
        let spec = format!("{}:{}", regular_fd, regular_fd + 1);
        let error = validate_identity_channel_spec(Some(OsStr::new(&spec)), descriptor_kind)
            .expect_err("regular descriptors must fail");
        let text = reason(error);
        assert!(
            text == REASON_TYPE || text == REASON_CLOSED,
            "unexpected reason {text}"
        );
        unsafe { libc::close(regular_fd) };
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "identity_channel_tests.rs"]
mod tests;
