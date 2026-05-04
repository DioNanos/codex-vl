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

## Current Release Focus

The `0.128.3` line focuses on keeping the fork close to upstream while
stabilizing the first useful Codex VL layers:

- loop management as a conservative, user-controlled local feature
- Vivling identity, persistence, lifecycle state, and model profile routing
- an early terminal CRT strip that can show compact Vivling state and speech
- maintainable integration points so upstream merges stay practical

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

## Status

Codex VL is active development software. Use the official OpenAI Codex release
when you want the upstream baseline without Codex VL additions.

## License

Apache 2.0. Upstream Codex remains under Apache 2.0, and the Codex VL additions
are distributed under the same license.
