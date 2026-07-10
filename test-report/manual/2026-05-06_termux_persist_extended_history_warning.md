# Termux Persist Extended History Warning Report

Date: 2026-05-06
Platform: Termux Android arm64
Branch under test: `develop`
Source commit before fix: `e4a562b43`
Source commit after fix: `bda505386`
Suite type: AI-guided Termux runtime and source-fix verification
Report hygiene: sanitized for public publication

## Summary

Decision: `PASS WITH BUILDER-SIDE FOLLOW-UP REQUIRED`

The source tree was updated to `origin/develop` at `e4a562b43`. The installed
runtime reports `codex-cli 0.128.4`. The startup warning was traced to client
code, not user configuration: app-server now deprecates `persistExtendedHistory`,
while the TUI still sent `persist_extended_history: true` for thread start,
resume, and fork requests.

The fix removes those three explicit TUI request fields so default request
serialization no longer sends the deprecated parameter.

The installed package smoke tests pass on Termux. The patched source was not
rebuilt on this device; Rust build/test gates are builder-only for this pass.

## Source Update

Initial `git pull origin develop` stopped because `origin/develop` had been
force-updated and the local `develop` branch diverged.

Resolution:

- created a local backup branch for the previous local `develop` head
- reset `develop` to `origin/develop`
- confirmed `HEAD` and `origin/develop` both pointed at `e4a562b43`

No uncommitted local work was present before applying the warning fix.

## Root Cause

Server-side behavior in the updated source:

- `persistExtendedHistory` is deprecated and ignored
- app-server always uses limited history persistence
- app-server intentionally emits a `deprecationNotice` when clients still send
  the field as `true`

Client-side cause:

- `codex-rs/tui/src/app_server_session.rs` set
  `persist_extended_history: true` in:
  - `thread_start_params_from_config`
  - `thread_resume_params_from_config`
  - `thread_fork_params_from_config`

## Patch

Changed files:

- `codex-rs/tui/src/app_server_session.rs`

Change:

- removed the explicit `persist_extended_history: true` fields from TUI
  start/resume/fork request construction

Expected effect:

- the TUI no longer sends the deprecated `persistExtendedHistory` parameter
- app-server should no longer emit the startup deprecation warning for normal
  TUI start/resume/fork flows

## Verification

Repo and release state:

- `PASS` branch `develop` tracks `origin/develop`
- `PASS` source head after fix: `bda505386`
- `PASS` npm dist-tags:
  - `next`: `0.128.4`
  - `latest`: `0.128.2`
  - `android-arm64`: `0.128.4-android-arm64`
  - `linux-x64`: `0.128.4-linux-x64`
  - `darwin-arm64`: `0.128.4-darwin-arm64`

Installed package surface:

- `PASS` `codex-vl --version`
  - observed `codex-cli 0.128.4`
- `PASS` `codex-vl-exec --version`
  - observed `codex-exec 0.128.4`
- `PASS` help routing:
  - `codex-vl --help`
  - `codex-vl exec --help`
  - `codex-vl review --help`
  - `codex-vl login --help`
  - `codex-vl logout --help`
  - `codex-vl resume --help`
  - `codex-vl fork --help`
  - `codex-vl mcp --help`
  - `codex-vl sandbox --help`
  - `codex-vl app-server generate-json-schema --help`
  - `codex-vl debug prompt-input --help`
  - `codex-vl completion bash`
- `PASS` `codex-vl login status`
  - reported logged in via ChatGPT
- `PASS` `codex-vl mcp list`
  - configured MCP servers listed with secrets redacted by CLI
- `PASS` `codex-vl features list`
  - fork-relevant features visible, including `goals`, `plugins`,
    `shell_tool`, `tool_search`, and `unified_exec`

Runtime smoke in a temporary workspace:

- `PASS` `codex-vl exec --skip-git-repo-check --ephemeral`
  - exact response: `OK`
- `PASS` `codex-vl-exec --sandbox workspace-write --skip-git-repo-check --json`
  directory/listing check
  - current directory and `seed.txt` listed
- `PASS` workspace read/write check
  - first generated shell command used a non-portable `printf` form and failed
  - agent corrected itself in the same turn
  - final readback returned:
    - `seed`
    - `hello-codex-vl`
- `PASS` network smoke
  - first HTTP status line: `HTTP/2 200`

Source and patch checks:

- `PASS` grep check in TUI source
  - no remaining `persist_extended_history: true`
  - no remaining `persistExtendedHistory`
- `PASS` `git diff --check`
- `PASS` patch surface
  - report updated through patch tool
  - changes are reviewable in Git
- `PASS` `cargo fmt --manifest-path codex-rs/Cargo.toml --all`
  - completed with rustfmt stable warnings about unstable
    `imports_granularity`; no formatting failure

## AI Tool Surface

The validating AI had these usable tool categories in-session:

- shell command execution
- stdin/session continuation
- patch application
- loop management
- live MCP memory read/write
- deferred tool discovery

No private MCP state files were read directly.

## Builder-Only Gates

`cargo check --manifest-path codex-rs/Cargo.toml -p codex-tui` was attempted
locally and stopped before reaching the patched code because the configured
Android linker was not present in this Termux environment:

```text
error: linker `aarch64-linux-android29-clang` not found
```

Observed local toolchain:

- `rustc` host: `aarch64-linux-android`
- available linker: `aarch64-linux-android-clang`
- missing linker expected by repo config: `aarch64-linux-android29-clang`

This is recorded as an environment limitation. Full build verification should be
performed on the configured Android/Termux builder.

The focused Rust tests from `test-report/AI_GUIDED_TEST_SUITE.md` were not
accepted as Termux Pixel validation in this pass. They remain required on a
configured builder:

```sh
cargo test --manifest-path codex-rs/Cargo.toml -p codex-tools dynamic_tools -- --nocapture
cargo test --manifest-path codex-rs/Cargo.toml -p codex-tools goal_tools -- --nocapture
cargo test --manifest-path codex-rs/Cargo.toml -p codex-app-server dynamic_tools -- --nocapture
cargo test --manifest-path codex-rs/Cargo.toml -p codex-state goals -- --nocapture
```

## Manual TUI Limitation

Manual TUI startup verification for the fixed warning requires a package rebuilt
from `bda505386` or newer. The currently installed package reports `0.128.4`,
but it predates this source fix. Runtime command smoke is therefore marked
PASS, while final startup-warning verification is a builder/reinstall follow-up.

## Follow-Up

Recommended builder-side check:

```sh
cargo check --manifest-path codex-rs/Cargo.toml -p codex-tui
```

Recommended runtime check after reinstalling a package built from this patch:

```sh
codex-vl
```

Expected result: no startup warning about `persistExtendedHistory`.
