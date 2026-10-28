//! Launcher-private environment policy for external child processes.
//!
//! The npm launchers (`codex-cli/bin/codex.js` and `codex-exec.js`) export a
//! handful of `CODEX_*` metadata variables that describe the running
//! installation: which package manager installed it and where the packaged
//! binary lives. The fork's managed-install context, doctor, and daemon read
//! them, so they must survive inside the launcher's own process tree and its
//! internal re-execs. They say nothing about the user's machine, though, and
//! must not leak to arbitrary external child processes such as shell tools,
//! MCP servers, hooks, or auth helpers — neither by inheritance nor through
//! a later explicit override.
//!
//! The boundary is Android-only: it covers the packaged bionic payload whose
//! ELF resolves its shared libraries through DT_RUNPATH, the same payload for
//! which the launchers stopped rewriting `LD_LIBRARY_PATH`. On every other
//! platform these names keep flowing unchanged. Neither the loader
//! (`LD_PRELOAD`, `LD_LIBRARY_PATH`), Termux process context, `PREFIX`, nor
//! the identity channel variables are in scope here.
//!
//! Everything in this module is pure policy over `std` types so that any
//! crate can rely on it without dependency cycles: [`is_private_launcher_env`]
//! decides for environment maps and serialized payloads, while
//! [`finalize_external_child`] mutates a [`Command`] after its last
//! environment mutation and removes both inherited values and explicit
//! overrides, so a trailing `envs()` call cannot reintroduce the names. It is
//! deliberately not wired into `background_command`: background commands
//! also serve the fork's own re-execs, which must keep their context.

use std::collections::HashMap;
use std::process::Command;

/// `target_os` value that activates the boundary.
pub const ANDROID_TARGET_OS: &str = "android";

/// Absolute path of the packaged binary, as launched by the npm launcher.
pub const CODEX_SELF_EXE_ENV_VAR: &str = "CODEX_SELF_EXE";

/// Package root of the running npm-launched installation.
pub const CODEX_MANAGED_PACKAGE_ROOT_ENV_VAR: &str = "CODEX_MANAGED_PACKAGE_ROOT";

/// Install-method flags recorded by the npm launcher.
pub const CODEX_MANAGED_BY_NPM_ENV_VAR: &str = "CODEX_MANAGED_BY_NPM";
pub const CODEX_MANAGED_BY_BUN_ENV_VAR: &str = "CODEX_MANAGED_BY_BUN";
pub const CODEX_MANAGED_BY_PNPM_ENV_VAR: &str = "CODEX_MANAGED_BY_PNPM";
pub const CODEX_MANAGED_BY_VITE_PLUS_ENV_VAR: &str = "CODEX_MANAGED_BY_VITE_PLUS";

/// Launcher metadata that external child processes must not receive on the
/// target platform. The original caller values are never restored: the names
/// are dropped, not swapped.
pub const PRIVATE_LAUNCHER_ENV_VARS: &[&str] = &[
    CODEX_SELF_EXE_ENV_VAR,
    CODEX_MANAGED_PACKAGE_ROOT_ENV_VAR,
    CODEX_MANAGED_BY_NPM_ENV_VAR,
    CODEX_MANAGED_BY_BUN_ENV_VAR,
    CODEX_MANAGED_BY_PNPM_ENV_VAR,
    CODEX_MANAGED_BY_VITE_PLUS_ENV_VAR,
];

/// Whether `name` is launcher-private metadata on `target_os`.
///
/// The comparison is ASCII case-insensitive: environment lookup on Android is
/// case sensitive, but a tool or launcher writing the name in a different
/// case must not be able to bypass the boundary. Names that merely resemble
/// the reserved ones stay untouched.
pub fn is_private_launcher_env(name: impl AsRef<str>, target_os: &str) -> bool {
    if target_os != ANDROID_TARGET_OS {
        return false;
    }
    let name = name.as_ref();
    PRIVATE_LAUNCHER_ENV_VARS
        .iter()
        .any(|restricted| restricted.eq_ignore_ascii_case(name))
}

/// [`is_private_launcher_env`] for the compiling target.
pub fn is_private_launcher_env_here(name: impl AsRef<str>) -> bool {
    is_private_launcher_env(name, std::env::consts::OS)
}

/// Finalize a `Command` that is about to run an EXTERNAL child process on
/// the compiling target: drop the launcher-private names from both the
/// inherited environment and the command's own overrides.
///
/// Call this after the last `env`/`envs` mutation (including the caller's
/// policy overlays) so no later write can reintroduce the names. Commands
/// that re-execute the fork's own binaries must NOT be finalized; they keep
/// the launcher context by design. Stdio configuration, process groups, and
/// platform creation flags are left untouched.
pub fn finalize_external_child(command: &mut Command) {
    finalize_external_child_for(command, std::env::consts::OS);
}

/// [`finalize_external_child`] with an explicit `target_os`, for tests and
/// for boundaries that serialize an environment for another host.
pub fn finalize_external_child_for(command: &mut Command, target_os: &str) {
    if target_os != ANDROID_TARGET_OS {
        return;
    }

    // Mirrors the non-inheritable-auth scrub: canonical names first, then any
    // case variants found either in the running process (inheritance source)
    // or among the command's explicit overrides.
    let configured_names = command
        .get_envs()
        .map(|(name, _)| name.to_os_string())
        .collect::<Vec<_>>();

    for name in PRIVATE_LAUNCHER_ENV_VARS {
        command.env_remove(name);
    }
    for name in std::env::vars_os()
        .map(|(name, _)| name)
        .chain(configured_names)
    {
        if name
            .to_str()
            .is_some_and(|name| is_private_launcher_env(name, target_os))
        {
            command.env_remove(name);
        }
    }
}

/// Drop the launcher-private names from an environment map built for an
/// external child (tool environments, exec payloads, remote MCP overlays).
///
/// The map is scrubbed in place; unrelated keys — including values that are
/// absent, empty, or non-UTF-8 — pass through untouched.
pub fn scrub_private_launcher_env(env: &mut HashMap<String, String>, target_os: &str) {
    if target_os != ANDROID_TARGET_OS {
        return;
    }
    env.retain(|name, _| !is_private_launcher_env(name, target_os));
}

/// [`scrub_private_launcher_env`] for the compiling target.
pub fn scrub_private_launcher_env_here(env: &mut HashMap<String, String>) {
    scrub_private_launcher_env(env, std::env::consts::OS);
}

/// The reserved names for `target_os`; empty where the boundary does not
/// apply. Useful for boundaries that validate a serialized environment
/// before sending it to a remote exec or MCP host.
pub fn private_launcher_env_names(target_os: &str) -> &'static [&'static str] {
    if target_os == ANDROID_TARGET_OS {
        PRIVATE_LAUNCHER_ENV_VARS
    } else {
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::ffi::OsString;
    use std::hash::Hasher;

    fn command_env_names(command: &Command) -> Vec<OsString> {
        command
            .get_envs()
            .map(|(name, _)| name.to_os_string())
            .collect()
    }

    /// A private name must not keep a value for the child.
    ///
    /// `Command::env_remove` neutralizes an inherited value or an explicit
    /// override by recording an unset, and the name can still show up in the
    /// override list with `None`. The child receives nothing in that case, so
    /// the assertion is on the value: a `Some` is a leak, a `None` is a drop.
    fn assert_no_private_launcher_values(command: &Command) {
        for (name, value) in command.get_envs() {
            let is_private = name
                .to_str()
                .is_some_and(|name| is_private_launcher_env(name, ANDROID_TARGET_OS));
            assert!(
                !is_private || value.is_none(),
                "{name:?} leaked a value: {value:?}"
            );
        }
    }

    #[test]
    fn policy_matches_exactly_the_six_names_on_android() {
        for name in PRIVATE_LAUNCHER_ENV_VARS {
            assert!(is_private_launcher_env(*name, ANDROID_TARGET_OS));
            assert!(is_private_launcher_env(
                name.to_lowercase(),
                ANDROID_TARGET_OS
            ));
        }

        let lookalikes = [
            "CODEX_SELF_EXE_EXTRA",
            "CODEX_MANAGED_PACKAGE_ROOT_2",
            "CODEX_MANAGED_BY_NPM_LEGACY",
            "CODEX_HOME",
            "CODEX_SESSION_ID",
            "PREFIX",
            "TERMUX__SE_PROCESS_CONTEXT",
            "TERMUX_VERSION",
            "LD_LIBRARY_PATH",
            "LD_PRELOAD",
            "NEXUSCREW_IDENTITY_FD",
            "",
        ];
        for name in lookalikes {
            assert!(!is_private_launcher_env(name, ANDROID_TARGET_OS), "{name}");
        }

        for target in ["linux", "macos", "windows", "freebsd"] {
            for name in PRIVATE_LAUNCHER_ENV_VARS {
                assert!(!is_private_launcher_env(*name, target));
            }
        }
    }

    #[test]
    fn finalizer_removes_explicit_overrides_and_keeps_foreign_entries() {
        let mut command = Command::new("definitely-not-a-real-program");
        for name in PRIVATE_LAUNCHER_ENV_VARS {
            command.env(name, "launcher-value");
        }
        command.env("TOOL_KEEP_ME", "kept");
        command.env("LD_LIBRARY_PATH", "/caller/lib");
        finalize_external_child_for(&mut command, ANDROID_TARGET_OS);

        assert_no_private_launcher_values(&command);
        let names = command_env_names(&command);
        assert!(names.contains(&OsString::from("TOOL_KEEP_ME")));
        assert!(names.contains(&OsString::from("LD_LIBRARY_PATH")));
    }

    #[cfg(unix)]
    #[test]
    fn finalizer_preserves_non_utf8_foreign_values() {
        use std::os::unix::ffi::OsStringExt;

        let mut command = Command::new("definitely-not-a-real-program");
        let raw = OsString::from_vec(vec![0xff, 0xfe, b'-']);
        command.env("TOOL_RAW", &raw);
        command.env(CODEX_SELF_EXE_ENV_VAR, "launcher-value");
        finalize_external_child_for(&mut command, ANDROID_TARGET_OS);

        let raw_entry = command
            .get_envs()
            .find(|(name, _)| *name == OsStr::new("TOOL_RAW"))
            .and_then(|(_, value)| value)
            .map(OsStr::to_os_string);
        assert_eq!(raw_entry, Some(raw));

        assert_no_private_launcher_values(&command);
    }

    #[test]
    fn finalizer_is_a_noop_outside_android() {
        let mut command = Command::new("definitely-not-a-real-program");
        for name in PRIVATE_LAUNCHER_ENV_VARS {
            command.env(name, "launcher-value");
        }
        finalize_external_child_for(&mut command, "linux");

        let names = command_env_names(&command);
        for name in PRIVATE_LAUNCHER_ENV_VARS {
            assert!(
                names.contains(&OsString::from(name)),
                "{name} must not be removed on non-Android targets"
            );
        }
    }

    #[test]
    fn map_scrub_drops_only_the_six_names_on_android() {
        let mut env = HashMap::new();
        for name in PRIVATE_LAUNCHER_ENV_VARS {
            env.insert(name.to_string(), "launcher-value".to_string());
        }
        env.insert("CODEX_SELF_EXE_EXTRA".to_string(), "keep".to_string());
        env.insert(
            "PREFIX".to_string(),
            "/data/data/com.termux/files/usr".to_string(),
        );
        env.insert("LD_LIBRARY_PATH".to_string(), String::new());

        let mut android_env = env.clone();
        scrub_private_launcher_env(&mut android_env, ANDROID_TARGET_OS);
        assert_eq!(android_env.len(), 3);
        assert!(android_env.contains_key("LD_LIBRARY_PATH"));
        assert_eq!(android_env["LD_LIBRARY_PATH"], "");

        scrub_private_launcher_env(&mut env, "macos");
        assert_eq!(env.len(), PRIVATE_LAUNCHER_ENV_VARS.len() + 3);
    }

    /// Probe plumbing for the isolated inheritance tests below.
    ///
    /// The helper processes are this test binary itself, relaunched with
    /// `--exact`: the child applies the production finalizer to a controlled
    /// grandchild, and the reporter writes an allowlisted view of its own
    /// environment plus its executable identity to a file the parent owns.
    /// Three properties keep the plumbing safe to run next to a real runner
    /// environment: the reporter never writes a name outside the allowlist
    /// (the six reserved launcher names in any case variant plus the
    /// synthetic test sentinels), every probe command starts from
    /// `env_clear()` so no real runner value can even reach a helper, and
    /// each test owns a fresh private `0700` directory with an O_EXCL-created
    /// `0600` report file removed by an RAII guard even on panic. Every
    /// mutation happens on `Command` values of child processes, so the test
    /// runner's global environment is never touched, and the helper is the
    /// same binary the suite runs in — a fixture compatible with whatever
    /// target executes the tests, identified by hash end to end.
    const PROBE_MODE_ENV: &str = "LAUNCHER_ENV_PROBE_MODE";
    const PROBE_OUT_ENV: &str = "LAUNCHER_ENV_PROBE_OUT";
    const PROBE_MODE_CHILD: &str = "child";
    const PROBE_MODE_REPORTER: &str = "reporter";
    const PROBE_IDENTITY_LINE: &str = "__EXE_IDENTITY__";

    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;

    /// Synthetic, test-only names the probes may observe in addition to the
    /// reserved launcher names. Real environment names never appear here.
    const PROBE_SYNTHETIC_NAMES: &[&str] = &["BOUNDARY_KEEP_ME", "LAUNCHER_POLICY_PROBE"];

    static PROBE_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Whether `name` may be written to a probe report: one of the six
    /// reserved launcher names in any ASCII case variant, or one exact
    /// synthetic sentinel. Everything else — every real runner variable
    /// included — is skipped, so no runner credential can reach the file.
    fn is_probe_reportable_name(name: &OsStr) -> bool {
        name.to_str().is_some_and(|name| {
            PRIVATE_LAUNCHER_ENV_VARS
                .iter()
                .any(|reserved| reserved.eq_ignore_ascii_case(name))
                || PROBE_SYNTHETIC_NAMES.contains(&name)
        })
    }

    /// Fresh directory name for one probe: runner pid, a per-process atomic
    /// counter, and a nonce hashed from the wall clock, so parallel parent
    /// tests never share a directory and the name cannot be pre-created.
    fn probe_dir_path() -> std::path::PathBuf {
        let counter = PROBE_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .expect("system clock after the unix epoch");
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash_slice(
            &[now.as_secs(), now.subsec_nanos() as u64, counter],
            &mut hasher,
        );
        std::env::temp_dir().join(format!(
            "codex-launcher-env-probe-{}-{}-{:016x}",
            std::process::id(),
            counter,
            hasher.finish()
        ))
    }

    /// A probe report owned by one parent test: a private `0700` directory
    /// under the temp dir plus a report file created with `create_new`
    /// (O_EXCL) and `0600` permissions, so a pre-planted file or symlink at
    /// that name is neither followed nor clobbered. The `Drop` guard removes
    /// both even when the test panics.
    struct ProbeReport {
        dir: std::path::PathBuf,
        file: std::path::PathBuf,
    }

    impl ProbeReport {
        fn create() -> Self {
            let dir = probe_dir_path();
            let mut dir_builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                dir_builder.mode(0o700);
            }
            dir_builder.create(&dir).expect("create private probe dir");

            let file = dir.join("report");
            let mut file_options = std::fs::OpenOptions::new();
            file_options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                file_options.mode(0o600);
            }
            file_options
                .open(&file)
                .expect("create probe report with create_new");
            Self { dir, file }
        }

        fn path(&self) -> &std::path::Path {
            &self.file
        }

        fn read(&self) -> String {
            std::fs::read_to_string(&self.file).expect("read probe report")
        }
    }

    impl Drop for ProbeReport {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.file);
            let _ = std::fs::remove_dir(&self.dir);
        }
    }

    /// Deterministic identity of the helper fixture executable. This is a
    /// stable equality check between the parent and the observed grandchild,
    /// not a cryptographic digest.
    fn exe_identity_hash() -> u64 {
        use std::hash::Hasher;
        let bytes = std::fs::read(std::env::current_exe().expect("current exe")).expect("read exe");
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&bytes, &mut hasher);
        hasher.finish()
    }

    fn probe_command(exact_test: &str, mode: &str, report_path: &std::path::Path) -> Command {
        let mut command = Command::new(std::env::current_exe().expect("current exe"));
        // Start from an empty environment and restore only the minimum the
        // helper needs: no real runner value can reach a helper process, and
        // the reporter's allowlist keeps whatever remains out of the report.
        command.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        if let Some(home) = std::env::var_os("HOME") {
            command.env("HOME", home);
        }
        command
            .args(["--exact", exact_test])
            .env(PROBE_MODE_ENV, mode)
            .env(PROBE_OUT_ENV, report_path);
        command
    }

    fn run_probe(command: &mut Command, label: &str) {
        let output = command.output().expect("{label} helper must run");
        assert!(output.status.success(), "{label} helper failed: {output:?}");
    }

    /// Overwrite the parent-owned, pre-created report file. Truncating an
    /// already-open descriptor inside the parent's private directory cannot
    /// follow a planted symlink.
    fn write_probe_report(path: &std::path::Path, report: &str) {
        use std::io::Write as _;

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .expect("open pre-created probe report");
        file.write_all(report.as_bytes())
            .and_then(|()| file.flush())
            .expect("write probe report");
    }

    /// Fail if any report line carries a reserved launcher name in any case
    /// variant, not just the canonical spelling the tests set.
    fn assert_no_private_name_lines(report: &str) {
        for line in report.lines() {
            let Some((name, _)) = line.split_once('\t') else {
                continue;
            };
            assert!(
                !is_private_launcher_env(name, ANDROID_TARGET_OS),
                "{name} leaked to the report"
            );
        }
    }

    fn probe_report_value<'a>(report: &'a str, name: &str) -> Option<&'a str> {
        report.lines().find_map(|line| {
            let (reported_name, value) = line.split_once('\t')?;
            (reported_name == name).then_some(value)
        })
    }

    /// Test-binary entry used as the isolated child helper. In a normal suite
    /// run the selector is absent and the test returns immediately; when the
    /// parent relaunches the binary with the selector set, it finalizes a
    /// controlled grandchild reporter with the production finalizer.
    #[test]
    fn launcher_env_probe_child() {
        if std::env::var(PROBE_MODE_ENV).as_deref() != Ok(PROBE_MODE_CHILD) {
            return;
        }
        // The report file is owned by the parent: write where it reads.
        let report_path =
            std::path::PathBuf::from(std::env::var_os(PROBE_OUT_ENV).expect("probe report path"));
        let mut command = probe_command(
            "launcher_env::tests::launcher_env_probe_reporter",
            PROBE_MODE_REPORTER,
            &report_path,
        );
        // Probe commands start from a cleared environment, so mirror the
        // allowlisted names this helper really inherited from its parent back
        // onto the reporter command: the finalizer must still see and drop
        // the launcher-private ones, and pass the synthetic sentinel through.
        for (name, value) in std::env::vars_os() {
            if is_probe_reportable_name(&name) {
                command.env(name, value);
            }
        }
        finalize_external_child_for(&mut command, ANDROID_TARGET_OS);
        run_probe(&mut command, "child");
    }

    /// Test-binary entry that writes an allowlisted view of its environment
    /// and its executable identity. Only writes the report file the parent
    /// pre-created; no assertions run here beyond the plumbing itself.
    #[test]
    fn launcher_env_probe_reporter() {
        if std::env::var(PROBE_MODE_ENV).as_deref() != Ok(PROBE_MODE_REPORTER) {
            return;
        }
        let out = std::env::var_os(PROBE_OUT_ENV).expect("probe report path");
        let mut report = String::new();
        for (name, value) in std::env::vars_os() {
            // Security allowlist: a name that is neither a reserved launcher
            // name nor a synthetic sentinel is never written to the report.
            if !is_probe_reportable_name(&name) {
                continue;
            }
            report.push_str(&name.to_string_lossy());
            report.push('\t');
            report.push_str(&value.to_string_lossy());
            report.push('\n');
        }
        report.push_str(PROBE_IDENTITY_LINE);
        report.push('\t');
        report.push_str(&exe_identity_hash().to_string());
        report.push('\n');
        write_probe_report(std::path::Path::new(&out), &report);
    }

    /// Sentinel (isolated inheritance): a real grandchild process never
    /// receives the six names. The parent relaunches this test binary as a
    /// child helper carrying the sentinels on its `Command` only; the child
    /// applies the production finalizer to the reporter grandchild, and the
    /// report confirms both the drop and the fixture identity. The runner's
    /// global environment is never mutated.
    #[test]
    fn sentinel_real_grandchild_does_not_inherit_the_six_names() {
        let report = ProbeReport::create();

        let mut command = probe_command(
            "launcher_env::tests::launcher_env_probe_child",
            PROBE_MODE_CHILD,
            report.path(),
        );
        for name in PRIVATE_LAUNCHER_ENV_VARS {
            command.env(name, format!("inherited-{name}"));
        }
        command.env("LAUNCHER_POLICY_PROBE", "keep-me");
        run_probe(&mut command, "child");

        let body = report.read();

        assert_eq!(
            probe_report_value(&body, "LAUNCHER_POLICY_PROBE"),
            Some("keep-me")
        );
        for name in PRIVATE_LAUNCHER_ENV_VARS {
            assert!(
                probe_report_value(&body, name).is_none(),
                "{name} leaked to the grandchild"
            );
        }
        assert_no_private_name_lines(&body);
        assert_eq!(
            probe_report_value(&body, PROBE_IDENTITY_LINE),
            Some(exe_identity_hash().to_string().as_str()),
            "the observed grandchild must be the same fixture executable"
        );
    }

    /// Sentinel (isolated override): explicit overrides written on the
    /// command are dropped by the finalizer even in a real child, and a
    /// foreign entry survives intact.
    #[test]
    fn sentinel_real_child_drops_an_explicit_override() {
        let report = ProbeReport::create();

        let mut command = probe_command(
            "launcher_env::tests::launcher_env_probe_reporter",
            PROBE_MODE_REPORTER,
            report.path(),
        );
        for (index, name) in PRIVATE_LAUNCHER_ENV_VARS.iter().enumerate() {
            command.env(*name, format!("override-sentinel-{index}"));
        }
        command.env("BOUNDARY_KEEP_ME", "kept");
        finalize_external_child_for(&mut command, ANDROID_TARGET_OS);
        run_probe(&mut command, "reporter");

        let body = report.read();

        assert_eq!(probe_report_value(&body, "BOUNDARY_KEEP_ME"), Some("kept"));
        for name in PRIVATE_LAUNCHER_ENV_VARS {
            assert!(
                probe_report_value(&body, name).is_none(),
                "{name} leaked to the child"
            );
        }
        assert_no_private_name_lines(&body);
    }

    /// Generic positive control, NOT a real SelfReexec proof: an unfinalized
    /// command still carries whatever it was given, which is exactly why the
    /// boundary needs the finalizer. The fork's real internal re-exec paths
    /// keep their context by design and stay qualified on the device gate.
    #[test]
    fn positive_control_unfinalized_child_keeps_the_names() {
        let report = ProbeReport::create();

        let mut command = probe_command(
            "launcher_env::tests::launcher_env_probe_reporter",
            PROBE_MODE_REPORTER,
            report.path(),
        );
        command.env(CODEX_SELF_EXE_ENV_VAR, "self-reexec-marker");
        run_probe(&mut command, "reporter");

        let body = report.read();

        assert_eq!(
            probe_report_value(&body, CODEX_SELF_EXE_ENV_VAR),
            Some("self-reexec-marker")
        );
    }

    /// The report is allowlisted: a name that is neither a reserved launcher
    /// name nor a synthetic probe sentinel is never written, even when it is
    /// present in the environment the father hands to the helper.
    #[test]
    fn reporter_never_writes_names_outside_the_allowlist() {
        let report = ProbeReport::create();

        let mut command = probe_command(
            "launcher_env::tests::launcher_env_probe_reporter",
            PROBE_MODE_REPORTER,
            report.path(),
        );
        command.env("SECRET_LIKE_NOT_ALLOWED", "probe-only-secret-value");
        run_probe(&mut command, "reporter");

        let body = report.read();

        assert_eq!(
            probe_report_value(&body, "SECRET_LIKE_NOT_ALLOWED"),
            None,
            "a non-allowlisted name must never be reported"
        );
        assert!(
            !body.contains("probe-only-secret-value"),
            "a non-allowlisted value must never be reported"
        );
        assert!(
            probe_report_value(&body, PROBE_IDENTITY_LINE).is_some(),
            "the reporter must have run and written its report"
        );
    }

    /// Sentinel: overrides written before the finalizer still end up removed,
    /// which pins the ordering the contract requires.
    #[test]
    fn sentinel_late_overrides_are_removed() {
        let mut command = Command::new("definitely-not-a-real-program");
        let late: Vec<(String, String)> = PRIVATE_LAUNCHER_ENV_VARS
            .iter()
            .map(|name| ((*name).to_string(), "late-override".to_string()))
            .collect();
        command.envs(late);
        command.env("BOUNDARY_KEEP_ME", "kept");

        finalize_external_child_for(&mut command, ANDROID_TARGET_OS);

        assert_no_private_launcher_values(&command);
        let names = command_env_names(&command);
        assert!(names.contains(&OsString::from("BOUNDARY_KEEP_ME")));
    }

    /// Sentinel: empty input, a command with no overrides, and look-alike names
    /// are not corrupted, and the finalizer never introduces a value.
    #[test]
    fn sentinel_empty_and_lookalike_inputs_are_not_corrupted() {
        let mut empty = HashMap::new();
        scrub_private_launcher_env(&mut empty, ANDROID_TARGET_OS);
        assert!(empty.is_empty());

        let mut command = Command::new("definitely-not-a-real-program");
        finalize_external_child_for(&mut command, ANDROID_TARGET_OS);
        assert_no_private_launcher_values(&command);
        assert!(
            command.get_envs().all(|(_, value)| value.is_none()),
            "the finalizer must not introduce a value"
        );

        let mut lookalikes = HashMap::new();
        lookalikes.insert("CODEX_SELF_EXE_EXTRA".to_string(), "keep".to_string());
        lookalikes.insert(
            "CODEX_MANAGED_BY_NPM_LEGACY".to_string(),
            "keep".to_string(),
        );
        scrub_private_launcher_env(&mut lookalikes, ANDROID_TARGET_OS);
        assert_eq!(lookalikes.len(), 2);
    }
}
