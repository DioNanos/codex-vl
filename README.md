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
Android arm64 installs use packaged native binaries. npm 11 and later require
explicit permission for the package postinstall script.

Install the latest release on Linux x64, Linux arm64, or Termux arm64.

```bash
npm install -g @mmmbuto/codex-vl@latest --allow-scripts=@mmmbuto/codex-vl
```

On Termux, also install ripgrep (the Android package resolves `rg` from PATH
and does not bundle it):

```bash
pkg install ripgrep
```

Check the installed version.

```bash
codex-vl --version
```

Sign in to Codex VL.

```bash
codex-vl login
```

The macOS arm64 package builds the native binary locally with Cargo. On npm 11
and later, explicitly allow the postinstall script and show its build output.

Check that Xcode Command Line Tools are installed.

```bash
xcode-select -p
```

Check that Cargo is available.

```bash
cargo --version
```

Install the latest release on macOS arm64 and display build output.

```bash
npm install -g @mmmbuto/codex-vl@latest --allow-scripts=@mmmbuto/codex-vl --foreground-scripts
```

Check the installed macOS version.

```bash
codex-vl --version
```

The first macOS build can take 10-30 minutes. If a previous install left the
platform package or binary incomplete, uninstall `@mmmbuto/codex-vl` first,
then repeat the complete command above. Do not run npm's target-less
`npm install -g --allow-scripts=...` hint by itself: it does not name an
install target.

Codex VL uses the normal Codex configuration and runtime state in `~/.codex/`.
Installing it does not replace the official `codex` binary.

For a local npm prefix:

Set the npm installation prefix.

```bash
npm config set prefix ~/.local
```

Install the latest release under that prefix; foreground output also shows the macOS source build.

```bash
npm install -g @mmmbuto/codex-vl@latest --allow-scripts=@mmmbuto/codex-vl --foreground-scripts
```

Check the version under the local prefix.

```bash
~/.local/bin/codex-vl --version
```

## Release Channels

The `latest` and `next` channels both point to `0.162.0-vl.1`, based on
upstream Codex `rust-v0.162.0`. The conservative `stable` tag currently points
to `0.153.2-vl.2`.

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
codex-vl mcp add example-server --env-var EXAMPLE_MCP_SESSION --env-var TMUX --env-var TMUX_PANE -- example-server mcp
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

## Voice

`/voice` starts a realtime voice conversation over WebRTC. The CLI spawns a
packaged helper (`codex-voice-host`) that owns the microphone and the speakers
and decodes audio with a private GStreamer runtime carried inside the package
itself (`codex-resources/voice`). Codex VL ships that runtime on macOS arm64 and
on glibc Linux (x64 and arm64), so `/voice` works wherever there is a
microphone. Voice audio goes to the realtime voice backend, exactly as in
upstream Codex.

Prerequisites by platform:

- **macOS arm64** - a microphone, and Microphone permission for the terminal or
  editor running the CLI (System Settings -> Privacy & Security ->
  Microphone). The runtime travels with the package; there is nothing else to
  install.
- **Linux x64 / arm64 (glibc)** - a working audio stack (ALSA or PulseAudio)
  with both an input and an output device. No GStreamer package is needed on the
  host: the bundle loads its own plugin directory. The Linux CLI is linked
  against musl while the voice helper is a glibc binary, so `/voice` needs a
  glibc host (Debian, Ubuntu, Fedora, ...); it is not available on musl-only
  systems such as Alpine.
- **Android / Termux** - not available. The platform gate rejects it, and the
  private GStreamer runtime does not exist there.
- **Servers and headless hosts** - `/voice` stays hidden when the runtime is
  missing, and reports a clear error when the runtime is present but no
  microphone can be opened.

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

Enter the Rust workspace.

```bash
cd codex-rs
```

Build the CLI in release mode.

```bash
cargo build --release -p codex-cli --bin codex
```

For a local macOS install, build from source with Cargo, then point your local
wrapper or npm prefix at the produced `codex` binary (the `codex-vl-exec`
command dispatches it via the `exec` subcommand). The
npm package includes Linux x64, Linux arm64 and Termux Android arm64
native packages plus the macOS arm64 source-build package.

## Roadmap

- **Termux-native audio** (parked): `/voice` is shipped on macOS and glibc
  Linux; Termux needs its own audio backend (PulseAudio or `termux-api`)
  because the Android audio path cannot initialize in a plain CLI process.

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
