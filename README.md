# Codex VL

> A side-by-side Codex CLI variant with local loop orchestration and an early
> Vivling companion layer for terminal workflows.

[![npm package](https://img.shields.io/npm/v/@mmmbuto/codex-vl?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/codex-vl)
[![license](https://img.shields.io/badge/license-Apache%202.0-4b5563?style=flat-square)](./LICENSE)

Codex VL is a fork of [OpenAI Codex](https://github.com/openai/codex) that
installs as `codex-vl`, so it can live next to the official `codex` command.

The fork keeps the upstream Codex runtime model and adds a small set of
experimental workflow features:

- `/loop` for session-scoped recurring checks and follow-up tasks; `/loop apply` / `/loop dismiss` to action or discard Vivling loop suggestions
- `/vivling` for a persistent local companion and orchestration foundation
- `/vl` for direct Vivling chat when a brain profile is configured
- `/remote-control` for daemon lifecycle checks and app pairing from inside the TUI
- `/mcp reload` to apply MCP server configuration changes without restarting
- side-by-side npm packaging under `@mmmbuto/codex-vl`

## Install

Linux x64, Linux arm64 (Raspberry Pi 4 / 5 and other arm64 boards) and Termux
Android arm64 installs use packaged native binaries:

```bash
npm install -g @mmmbuto/codex-vl
codex-vl --version
codex-vl login
```

The macOS arm64 package builds the native binary locally with Cargo. npm 12
blocks dependency lifecycle scripts unless the package is explicitly allowed,
so verify the build prerequisites and use this complete install command:

```bash
xcode-select -p
cargo --version
npm install -g @mmmbuto/codex-vl@next \
  --allow-scripts=@mmmbuto/codex-vl \
  --foreground-scripts
codex-vl --version
```

The command above names `@next`, the channel this version ships on. Use
`@latest` instead when you want the stable line rather than the alpha — the
channel in the command has to match the one you actually want, and the two
resolve to different versions.

The first macOS build can take 10-30 minutes. If a previous install left the
platform package or binary incomplete, uninstall `@mmmbuto/codex-vl` first,
then repeat the complete command above. Do not run npm's target-less
`npm install -g --allow-scripts=...` hint by itself: it does not name an
install target.

Codex VL uses the normal Codex configuration and runtime state in `~/.codex/`.
Installing it does not replace the official `codex` binary.

For a local npm prefix:

```bash
npm config set prefix ~/.local
npm install -g @mmmbuto/codex-vl # add the macOS flags shown above on npm 12
~/.local/bin/codex-vl --version
```

## Release Channels

The `0.153.2` line is based on the upstream Codex `rust-v0.153.2` stable
release and preserves the complete Codex VL workflow layer, alongside the
native Android V8 build.

The upstream base of that line is a **stable** release, but the Codex VL
build on it is an alpha (`-vl.1`) and ships on the `next` channel. The
conservative `stable` tag remains on `0.144.5` until a later explicitly
authorized promotion.

Packages cover Linux x64, Linux arm64 (musl), Android arm64, and macOS arm64
source builds. The main package is a thin wrapper: the binaries live in the
per-platform packages, which is why they are published **first** and why the
main one is useless without them.

The `0.147.0` line remains as described above: the Termux TLS root fix carried
there for parity was retired at that version, because upstream removed the
client it wrapped and MCP OAuth discovery now runs on the injected
`codex-http-client`.

Local stdio MCP servers start with a restricted environment. Forward a
process variable explicitly by name when the server needs it; repeat the flag
for multiple names:

```bash
codex-vl mcp add example-server \
  --env-var EXAMPLE_MCP_SESSION \
  --env-var TMUX \
  --env-var TMUX_PANE \
  -- example-server mcp
```

The command stores only the variable names. Their values are read from the
launching process when the MCP server starts and are never written to
`config.toml`.

For macOS, the package is a source-build payload instead of a prebuilt native
binary; the local install path requires Rust/Cargo on the Mac. The postinstall
script performs a fail-closed preflight check (Xcode Command Line Tools, Cargo,
optional rustup target) and prints actionable hints when something is missing.

**Restored on the native Android arm64 package (0.136.x):**

- **code-mode** (`exec` / `wait`): the in-process V8 runtime is now enabled on
  the native Android target, so code-mode is no longer a no-op stub there. This
  is the meaningful capability gain on Android. The Android package bundles
  `libc++_shared.so` next to the binaries (`RUNPATH=$ORIGIN`), since Termux has
  no system copy.

**Realtime voice/audio (removed upstream):**

Upstream OpenAI Codex removed the experimental TUI realtime voice/audio feature
in `rust-v0.140.0` (openai/codex#27801). Codex VL no longer ships it on any
platform. The Android cpal/oboe enablement this fork used to carry — never
usable from a plain Termux CLI anyway, since the audio backend (cpal → oboe →
`ndk-context`) needs an Android `JavaVM`/`Activity` a command-line process does
not have — was dropped along with it. A Termux-native audio backend remains a
possible future direction, not a current capability.

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
Supported subcommands are `status`, `start`, `stop`, `restart`, and `pair`.

`pair` prints the code the ChatGPT mobile app asks for. It attaches to a daemon
that is already listening and starts one only when none is, so pairing does not
need a separate `start` first. `status` is the only read-only subcommand: it
probes the daemon without starting it. Client enrollment toggles are
intentionally not implemented in this command.

### `/mcp`

Lists the configured MCP servers; `/mcp verbose` also lists their tools.

`/mcp reload` re-reads the MCP server configuration and applies it to every
active session, so a server added, removed, or re-pointed in `config.toml`
takes effect without restarting Codex VL. Each session picks the new
configuration up when it resolves its MCP runtime for the next model step,
which means a turn already running can adopt it before it ends.

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
cargo build --release -p codex-cli --bin codex
```

For a local macOS install, build from source with Cargo, then point your local
wrapper or npm prefix at the produced `codex` binary (the `codex-vl-exec`
command dispatches it via the `exec` subcommand). The
npm package includes Linux x64, Linux arm64 and Termux Android arm64
native packages plus the macOS arm64 source-build package.

## Roadmap

- **Termux-native audio** (parked): upstream removed the realtime voice feature
  in `rust-v0.140.0` (openai/codex#27801), so this is on hold unless realtime
  voice returns upstream. If it does, a Termux-native backend (PulseAudio /
  `termux-api`) would be needed, since cpal's Android AAudio path cannot
  initialize in a plain CLI.

## Status

Codex VL is active development software. Use the official OpenAI Codex release
when you want the upstream baseline without Codex VL additions.

## Security

Codex VL is a community fork of OpenAI Codex. Security-relevant properties of this build:

- **Network**: agents bind to loopback by default; nothing is exposed externally unless you opt in.
  `/remote-control` and the app-server require explicit opt-in.
- **Supply chain**: builds and releases come only from fork-owned CI and the `@mmmbuto/codex-vl`
  npm scope. This package does not silently fetch or run the upstream installer; updates flow
  through the fork's own channel.
- **Termux**: TLS trust uses bundled webpki roots (no Android platform-verifier dependency), and
  advisory file locks degrade safely where unsupported.

For sensitive work, prefer the official Codex CLI on Linux/macOS over SSH.

To report a vulnerability, see [SECURITY.md](./SECURITY.md).

## License

Apache 2.0. Upstream Codex remains under Apache 2.0, and the Codex VL additions
are distributed under the same license.
