use super::*;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

fn pair() -> (IdentityFdChannel, UnixStream) {
    let (channel, peer) = UnixStream::pair().unwrap();
    let output: OwnedFd = channel.try_clone().unwrap().into();
    let input: OwnedFd = channel.into();
    (
        IdentityFdChannel::from_files(output.into(), input.into()).unwrap(),
        peer,
    )
}

fn drain_eof(peer: &mut UnixStream) -> bool {
    peer.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bytes = [0u8; 4096];
    while Instant::now() < deadline {
        match peer.read(&mut bytes) {
            Ok(0) => return true,
            Ok(_) => continue,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::Interrupted =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return false,
        }
    }
    false
}

#[tokio::test]
#[allow(clippy::print_stderr)]
async fn timeout_covers_backpressured_write_and_closes_descriptors() {
    // The `/proc/self/task` count is a process-GLOBAL counter, not a
    // property of this test, and the parallel harness turns it into false
    // reds. The local invariants are:
    // lo scambio termina entro il budget (la scrittura in backpressure non
    // resta appesa), il peer vede EOF (fd chiusi) e la runtime del test non
    // lascia task vivi in piu'.
    let tasks = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();
    let (channel, mut peer) = pair();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        channel.request(
            "test",
            json!({"large":"x".repeat(1024*1024)}),
            Duration::from_millis(50),
        ),
    )
    .await
    .expect("lo scambio deve terminare entro il budget");
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    assert!(drain_eof(&mut peer));
    let after_tasks = tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks();
    eprintln!("shared write timeout: tasks {tasks}->{after_tasks}, peer EOF=true");
    assert!(after_tasks <= tasks, "tasks leaked: {tasks}->{after_tasks}");
}

#[tokio::test]
async fn waiting_for_lock_times_out_and_cancels_active_exchange() {
    let (channel, mut peer) = pair();
    let (first, second) = tokio::join!(
        channel.request("first", json!({}), Duration::from_secs(1)),
        channel.request("second", json!({}), Duration::from_millis(50)),
    );
    assert_eq!(first.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(second.unwrap_err().kind(), io::ErrorKind::TimedOut);
    assert!(drain_eof(&mut peer));
}

#[tokio::test]
async fn dropping_request_future_invalidates_channel() {
    let (channel, mut peer) = pair();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            channel.request("test", json!({}), Duration::from_secs(1))
        )
        .await
        .is_err()
    );
    assert!(drain_eof(&mut peer));
    assert_eq!(
        channel
            .request("again", json!({}), Duration::from_secs(1))
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[tokio::test]
async fn excessive_response_is_rejected_and_closed() {
    let (channel, mut peer) = pair();
    peer.write_all(&vec![b'x'; MAX_LINE_BYTES + 1]).unwrap();
    assert_eq!(
        channel
            .request("test", json!({}), Duration::from_secs(1))
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert!(drain_eof(&mut peer));
}
