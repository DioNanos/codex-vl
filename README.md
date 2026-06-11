# Codex VL

> A side-by-side Codex CLI variant with local loop orchestration and an early
> Vivling companion layer for terminal workflows.

[![npm package](https://img.shields.io/npm/v/@mmmbuto/codex-vl?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/codex-vl)
[![license](https://img.shields.io/badge/license-Apache%202.0-4b5563?style=flat-square)](./LICENSE)

Codex VL is a fork of [OpenAI Codex](https://github.com/openai/codex) that
installs as `codex-vl`, so it can live next to the official `codex` command.

The fork keeps the upstream Codex runtime model and adds a small set of
experimental workflow features:

- `/loop` for session-scoped recurring checks and follow-up tasks
- `/vivling` for a persistent local companion and orchestration foundation
- `/vl` for direct Vivling chat when a brain profile is configured
- `/remote-control` for daemon lifecycle checks from inside the TUI
- side-by-side npm packaging under `@mmmbuto/codex-vl`

## Install

```bash
npm install -g @mmmbuto/codex-vl
codex-vl --version
codex-vl login
```

Linux x64, Linux arm64 (Raspberry Pi 4 / 5 and other arm64 boards) and Termux
Android arm64 installs use packaged native binaries. On macOS, npm install
builds the local native binaries with Cargo; install Rust if Cargo is not
already available.

Codex VL uses the normal Codex configuration and runtime state in `~/.codex/`.
Installing it does not replace the official `codex` binary.

For a local npm prefix:

```bash
npm config set prefix ~/.local
npm install -g @mmmbuto/codex-vl
~/.local/bin/codex-vl --version
```

## Release Channels

The npm `next` tag tracks the `0.139.0` line, which merges upstream Codex
`rust-v0.139.0` while preserving the Codex VL workflow layer (and carries the
Termux TLS fix plus the native Android V8 149.2.0 build). The `latest` tag
now tracks the same `0.139.0` line, and the `stable` dist-tag points to the
`0.135.0` line for conservative installs. All ship Linux x64, Linux arm64 (musl) and Android arm64 native
packages plus a macOS arm64 source-build package, each platform under its own
`<platform>` dist-tag (`linux-x64`, `linux-arm64`, `android-arm64`,
`darwin-arm64`).

```bash
npm install -g @mmmbuto/codex-vl@next     # 0.139.0
npm install -g @mmmbuto/codex-vl@latest   # 0.139.0
npm install -g @mmmbuto/codex-vl@stable   # 0.135.0
```

For macOS, the package is a source-build payload instead of a prebuilt native
binary; the local install path requires Rust/Cargo on the Mac. The postinstall
script performs a non-blocking preflight check (Xcode Command Line Tools, Cargo,
optional rustup target) and prints actionable hints when something is missing.

**Restored on the native Android arm64 package (0.136.x):**

- **code-mode** (`exec` / `wait`): the in-process V8 runtime is now enabled on
  the native Android target, so code-mode is no longer a no-op stub there. This
  is the meaningful capability gain on Android. The Android package bundles
  `libc++_shared.so` next to the binaries (`RUNPATH=$ORIGIN`), since Termux has
  no system copy.

**Known limitation — realtime voice/audio on Android (Termux):**

The realtime audio modules build for Android, but they are **not usable in a
plain Termux CLI**. The audio backend (cpal → oboe → `ndk-context`) requires an
Android `JavaVM`/`Activity` to initialize, which a command-line process in
Termux does not have. As a result the experimental `/realtime` and `/settings`
commands cannot open an audio device under Termux. The realtime conversation
feature is off by default (under-development); do not enable it on Termux.

Making voice work on Termux would require a different audio backend
(PulseAudio or `termux-api`) rather than cpal's Android AAudio path. That is
**not** in scope here and is tracked on the Codex VL roadmap, not implemented as
a runtime change in this release.

Vivling behavior is still experimental. It is intended to become a workflow
assistant over time, but the current public surface is deliberately small.

## Commands

### `/loop`

Creates and manages recurring local jobs attached to the current TUI session.
Loops are useful for periodic status checks, long-running work supervision, and
agent-managed follow-up tasks.

### `/vivling`

Manages the active Vivling. Current features include local state, growth,
lifecycle status, species data, and optional brain profile configuration.
The public development journal is at
[dev.mmmbuto.com/vivling](https://dev.mmmbuto.com/vivling/).

### `/vl`

Sends a direct message to the active Vivling. If the Vivling brain is ready, the
message routes through its configured Codex profile. Otherwise Codex VL uses the
local fallback reply path.

### `/remote-control`

Checks and controls the Codex remote-control daemon without leaving the TUI.
Supported subcommands are `status`, `start`, `stop`, and `restart`. Enrollment
toggles are intentionally not implemented in this command yet.

## Configuration

Vivling brain models use standard Codex profiles and providers. No shell wrapper
is required.

Start with:

- [Vivling brain model configuration](docs/vivling_model_catalog.md)
- [Codex configuration reference](docs/config.md)

Minimal flow:

```text
/vivling model <profile>
/vivling brain on
/vl hello
```

## Build From Source

```bash
cd codex-rs
cargo build --release -p codex-cli --bin codex -p codex-exec --bin codex-exec
```

For a local macOS install, build from source with Cargo, then point your local
wrapper or npm prefix at the produced `codex` and `codex-exec` binaries. The
npm `latest` publish includes Linux x64, Linux arm64 and Termux Android arm64
native packages plus the macOS arm64 source-build package.

## Roadmap

- **Realtime audio on Termux** (high priority, in progress alongside the MSA
  Vivling work): a Termux-native audio backend (PulseAudio / `termux-api`)
  so realtime voice works on Android/Termux, where cpal's AAudio path cannot
  initialize (see the Android limitation under Release Channels).

## Status

Codex VL is active development software. Use the official OpenAI Codex release
when you want the upstream baseline without Codex VL additions.

## License

Apache 2.0. Upstream Codex remains under Apache 2.0, and the Codex VL additions
are distributed under the same license.
