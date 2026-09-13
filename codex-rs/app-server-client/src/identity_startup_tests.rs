use super::*;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::Command;

#[test]
fn managed_startup_protects_channel_without_mcp() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let marker = directory.path().join("mcp-started");
    let fake = directory.path().join("nexuscrew");
    std::fs::write(
        &fake,
        "#!/bin/sh\nprintf invoked > \"$IDENTITY_TEST_MARKER\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    // The declared descriptors must be a real pipe or socket pair: the capture
    // refuses any other descriptor type, so a character device would not exercise
    // the startup protection this test is about.
    let (output, input) = UnixStream::pair().unwrap();
    let input_fd = input.as_raw_fd();
    let output_fd = output.as_raw_fd();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "remote::identity_startup_tests::managed_startup_child",
            "--nocapture",
        ])
        .env("IDENTITY_TEST_CHILD", "1")
        .env("IDENTITY_TEST_MARKER", &marker)
        .env("NEXUSCREW_MCP_SESSION", "managed-test")
        .env("NEXUSCREW_IDENTITY_FD", "90:91")
        .env("PATH", directory.path());
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(output_fd, 90) < 0 || libc::dup2(input_fd, 91) < 0 {
                return Err(IoError::last_os_error());
            }
            Ok(())
        });
    }
    let result = command.output().expect("child test process");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        !marker.exists(),
        "startup invoked nexuscrew mcp before binding"
    );
}

#[test]
fn managed_startup_child() {
    if std::env::var_os("IDENTITY_TEST_CHILD").is_none() {
        return;
    }
    assert!(prepare_nexuscrew_identity_channel().unwrap());
    assert!(std::env::var_os("NEXUSCREW_IDENTITY_FD").is_none());
    assert!(
        prepare_nexuscrew_identity_channel().unwrap(),
        "reconnect retains the channel"
    );
    for fd in [90, 91] {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0);
    }
    let result = Command::new("/bin/sh")
        .args([
            "-c",
            "test ! -e /proc/self/fd/90 && test ! -e /proc/self/fd/91",
        ])
        .status()
        .unwrap();
    assert!(result.success(), "identity descriptors leaked into a child");
}
