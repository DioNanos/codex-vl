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
- side-by-side npm packaging under `@mmmbuto/codex-vl`

## Install

```bash
npm install -g @mmmbuto/codex-vl
codex-vl --version
codex-vl login
```

When a pre-release lane is active, install it explicitly:

```bash
npm install -g @mmmbuto/codex-vl@next
codex-vl --version
```

Linux x64 and Termux Android arm64 installs use packaged native binaries. On
macOS, npm install builds the local native binaries with Cargo; install Rust if
Cargo is not already available.

Codex VL uses the normal Codex configuration and runtime state in `~/.codex/`.
Installing it does not replace the official `codex` binary.

For a local npm prefix:

```bash
npm config set prefix ~/.local
npm install -g @mmmbuto/codex-vl
~/.local/bin/codex-vl --version
```

## Release Channels

The npm `latest` tag tracks the stable `0.131.0` line. It merges upstream
Codex `rust-v0.131.0` while preserving the Codex VL workflow layer:

- loop management as a conservative, user-controlled local feature
- Vivling identity, persistence, lifecycle state, and model profile routing
- an early terminal CRT strip that can show compact Vivling state and speech
- upstream plugin sharing, remote-control, thread pagination, Bedrock auth, and
  environment-aware image handling improvements from the 0.131.0 release
- SQLite contention hardening for multi-session local use
- MCP startup retry hardening for stdio servers that are slow to expose tools
- maintainable integration points so upstream merges stay practical
- optimized Linux x64 and Termux Android arm64 npm packages under the `latest`
  release lane
- a macOS arm64 source-build npm package that builds and installs locally with
  Cargo during npm postinstall

For this line, macOS is shipped as a source-build payload instead of a prebuilt
native binary. The local install path still requires Rust/Cargo on the Mac.

The npm `next` tag is reserved for the next upstream alpha lane after a stable
release. Use it only when a specific pre-release has been announced.

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
`0.131.0` npm `latest` publish includes Linux x64 and Termux Android arm64 native
packages plus the macOS arm64 source-build package.

## Status

Codex VL is active development software. Use the official OpenAI Codex release
when you want the upstream baseline without Codex VL additions.

## License

Apache 2.0. Upstream Codex remains under Apache 2.0, and the Codex VL additions
are distributed under the same license.
