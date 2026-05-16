//! codex-vl fork identity hardening (iter F).
//!
//! Source-pin tests that guard against a silent revert of the fork
//! identity (npm package scope, GitHub release-feed URL, install-script
//! fork default repo) during an upstream merge. The pins are intentionally
//! written as `include_str!` source-grep assertions so they:
//!
//! - work regardless of `#[cfg(not(debug_assertions))]` gating on the
//!   real constants (the file we import from is always present on disk),
//! - catch both literal reintroductions of `openai/codex` and accidental
//!   gutting of the helper constants.
//!
//! Upstream-true surfaces (SDK packages, installer scripts in
//! `scripts/install/`, `MODULE.bazel`, `responses-api-proxy`, feedback /
//! announcement tooltips) are intentionally NOT scanned here because
//! they legitimately reference the parent `openai/codex` repo.

/// Pin: TUI updates module keeps the GitHub releases feed URL pointing at
/// the fork. `tui/src/updates.rs` is `#[cfg(not(debug_assertions))]` at
/// the module level, so we use `include_str!` rather than a direct
/// reference to `LATEST_RELEASE_URL`.
#[test]
fn fork_identity_pin_tui_updates_latest_release_url() {
    const SOURCE: &str = include_str!("../src/updates.rs");
    assert!(
        SOURCE.contains("api.github.com/repos/DioNanos/codex-vl/releases/latest"),
        "tui/src/updates.rs::LATEST_RELEASE_URL must point at the fork \
         release feed (DioNanos/codex-vl).",
    );
    assert!(
        !SOURCE.contains("api.github.com/repos/openai/codex/releases/latest"),
        "tui/src/updates.rs must not point the updater at the upstream \
         openai/codex release feed.",
    );
}

/// Pin: the `install_native_deps.py` build script keeps a fork-safe
/// default repo so that, even when the workflow URL cannot be parsed,
/// the fallback resolves to `DioNanos/codex-vl` instead of silently
/// pointing back at the upstream parent.
#[test]
fn fork_identity_install_native_deps_fork_default_repo() {
    const SOURCE: &str = include_str!("../../../codex-cli/scripts/install_native_deps.py");
    assert!(
        SOURCE.contains("DEFAULT_GITHUB_REPO = \"DioNanos/codex-vl\""),
        "install_native_deps.py must declare DEFAULT_GITHUB_REPO = \
         \"DioNanos/codex-vl\" so the fork-safe fallback is explicit.",
    );
    assert!(
        SOURCE.contains("repo = DEFAULT_GITHUB_REPO"),
        "install_native_deps.py must use DEFAULT_GITHUB_REPO as the \
         fallback repo when the workflow URL cannot be parsed.",
    );
    assert!(
        SOURCE.contains("repo: str = DEFAULT_GITHUB_REPO"),
        "install_native_deps.py::_download_artifacts default `repo` \
         parameter must resolve to DEFAULT_GITHUB_REPO.",
    );
    // Explicitly forbid the literal upstream fallback from re-entering
    // either the fallback assignment or the function default.
    assert!(
        !SOURCE.contains("repo = \"openai/codex\""),
        "install_native_deps.py must not use \"openai/codex\" as the \
         fallback repo literal.",
    );
    assert!(
        !SOURCE.contains("repo: str = \"openai/codex\""),
        "install_native_deps.py::_download_artifacts must not default \
         the `repo` parameter to \"openai/codex\".",
    );
}

/// Catch-all: a curated set of fork-owned files must not contain the
/// upstream `openai/codex` substring. Each entry in the table lists the
/// optional substrings whose presence is allowed (e.g. comments that
/// explicitly call out the upstream historical default). Any unlisted
/// occurrence is treated as a silent-revert candidate.
#[test]
fn fork_identity_no_openai_codex_in_fork_owned_sources() {
    struct Case<'a> {
        path: &'a str,
        source: &'a str,
        // Lines containing any of these substrings are allowed to mention
        // `openai/codex` (e.g. comments that document upstream historical
        // defaults that are intentionally preserved).
        allowed_substrings: &'a [&'a str],
    }

    // The substring `fork_identity` marks lines that belong to our own
    // pin tests — those lines intentionally mention `openai/codex` to
    // assert that the surrounding source does NOT. Whitelisting the
    // marker avoids a self-reference failure.
    let pin_marker = &["fork_identity"];

    let cases = [
        Case {
            path: "codex-rs/cli/src/doctor/updates.rs",
            source: include_str!("../../cli/src/doctor/updates.rs"),
            allowed_substrings: pin_marker,
        },
        Case {
            path: "codex-rs/tui/src/updates.rs",
            source: include_str!("../src/updates.rs"),
            allowed_substrings: &[],
        },
        Case {
            path: "codex-rs/tui/src/npm_registry.rs",
            source: include_str!("../src/npm_registry.rs"),
            allowed_substrings: pin_marker,
        },
        Case {
            path: "codex-rs/tui/src/history_cell/notices.rs",
            source: include_str!("../src/history_cell/notices.rs"),
            allowed_substrings: &[],
        },
        Case {
            path: "codex-rs/tui/src/update_prompt.rs",
            source: include_str!("../src/update_prompt.rs"),
            allowed_substrings: &[],
        },
        Case {
            path: "scripts/stage_npm_packages.py",
            source: include_str!("../../../scripts/stage_npm_packages.py"),
            allowed_substrings: &[],
        },
        Case {
            // The install script intentionally keeps the historical
            // upstream default workflow URL as a documented placeholder;
            // the fork-safe default repo is `DEFAULT_GITHUB_REPO` and
            // the parsing of `--workflow-url` is what drives the real
            // value at runtime.
            path: "codex-cli/scripts/install_native_deps.py",
            source: include_str!("../../../codex-cli/scripts/install_native_deps.py"),
            allowed_substrings: &[
                "DEFAULT_WORKFLOW_URL",
                "original `openai/codex` parent workflow",
                "without falling back to openai/codex",
                "silently pointing back at openai/codex",
            ],
        },
    ];

    for case in cases {
        let mut offending_lines = Vec::new();
        for (idx, line) in case.source.lines().enumerate() {
            if !line.contains("openai/codex") {
                continue;
            }
            let allowed = case
                .allowed_substrings
                .iter()
                .any(|needle| line.contains(needle));
            if !allowed {
                offending_lines.push(format!("  {}:{}: {}", case.path, idx + 1, line.trim()));
            }
        }
        assert!(
            offending_lines.is_empty(),
            "Fork-owned source `{}` contains unwhitelisted `openai/codex` \
             references — silent-revert candidate. Offending lines:\n{}",
            case.path,
            offending_lines.join("\n"),
        );
    }
}
