# Termux Persist Extended History Warning Report

Date: 2026-05-06
Platform: Termux Android arm64
Branch under test: `develop`
Source commit before fix: `e4a562b43`
Suite type: focused source and runtime-surface verification
Report hygiene: sanitized for public publication

## Summary

Decision: `PASS WITH BUILD ENVIRONMENT LIMITATION`

The source tree was updated to `origin/develop` at `e4a562b43`. The installed
runtime reports `codex-cli 0.128.4`. The startup warning was traced to client
code, not user configuration: app-server now deprecates `persistExtendedHistory`,
while the TUI still sent `persist_extended_history: true` for thread start,
resume, and fork requests.

The fix removes those three explicit TUI request fields so default request
serialization no longer sends the deprecated parameter.

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

Commands/results:

- `PASS` `codex-vl --version`
  - observed `codex-cli 0.128.4`
- `PASS` `codex-vl exec --help`
  - help text rendered normally
- `PASS` grep check in TUI source
  - no remaining `persist_extended_history: true`
  - no remaining `persistExtendedHistory`
- `PASS` `git diff --check`
- `PASS` `cargo fmt --manifest-path codex-rs/Cargo.toml --all`
  - completed with rustfmt stable warnings about unstable
    `imports_granularity`; no formatting failure

## Build Limitation

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
