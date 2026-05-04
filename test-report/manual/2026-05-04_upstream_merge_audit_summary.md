# Codex-VL Upstream Merge Audit - 2026-05-04

## Scope

Audit of the staged merge that brings the fork forward to current upstream `main`
while preserving the Codex-VL fork core:

- Vivling UI/runtime modules
- `/vl` terminal visual layer modules
- `/loop` loop controller and loop job UI modules
- Termux packaging and Android V8 portability patch set
- MCP dynamic tools compatibility for app-server thread start/resume

No release build is included in this report.

## Merge Shape

- Base used for the clean public merge: last known good public fork commit.
- Upstream merged: current upstream `main`.
- The TUI subtree was kept from the fork and adapted only for upstream API drift.
- Upstream app-server and core changes were kept, with fork compatibility patches
  reapplied where needed.

## Preserved Fork Assets

The staged index contains the required fork core paths:

- `codex-rs/tui/src/vivling.rs`
- `codex-rs/tui/src/vl/mod.rs`
- `codex-rs/tui/src/app/loop_controller.rs`
- `codex-rs/tui/src/app/vl_handler.rs`
- `codex-rs/tui/src/chatwidget/loop_jobs.rs`
- `codex-rs/tui/src/bottom_pane/vivling_view.rs`
- `codex-rs/tui/src/bottom_pane/vl_ext.rs`
- `codex-rs/tui/src/chatwidget/vl_ext.rs`
- `.github/workflows/package-linux-termux.yml`
- `scripts/fetch_rusty_v8_android.py`
- `third_party/v8/android-artifacts.toml`

No staged deletion targets these fork core paths.

## Local/Private Sanitization

The public test report set was reduced to readable runtime summaries and
templates. Large generated schema dumps were intentionally excluded.

Sanitization applied to report content:

- user home paths replaced with `<home>`
- node global prefix paths replaced with `<node-global-prefix>`
- user binary paths replaced with `<user-bin>`
- Termux prefix paths replaced with `<termux-prefix>`
- Termux home paths replaced with `<termux-home>`
- loopback Ollama endpoint normalized to `localhost:11434`

The staged report set was checked for private user paths, private cloud host
names, private infrastructure paths, API credential variable names, GitHub
credential prefixes, and common authorization-header patterns.

## GitHub Main Hygiene

The staged public merge excludes private infrastructure-only paths such as:

- `.docs/`
- private CI metadata directories
- `scripts/forge/`
- local release helper scripts under `scripts/release/`
- private Android verification helper paths

The added `.codex/environments/environment.toml` is an upstream public file and
contains only a generic cargo run action.

## Checks Performed

- staged core-presence check for Vivling, VL, loop, and Termux assets
- staged deletion guard for Vivling, VL, and loop paths
- staged private-path and credential-pattern scan for `test-report`
- staged private-infrastructure path scan for GitHub main
- staged whitespace check with `git diff --cached --check`

## Notes

Credential-pattern scans also match upstream test fixtures that intentionally
use dummy authorization strings. These are not runtime credentials.
