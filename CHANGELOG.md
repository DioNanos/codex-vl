# Changelog

All notable Codex VL changes are tracked here.

Codex VL tracks OpenAI Codex upstream, but this changelog only covers fork-specific work.

## 0.153.2-vl.2 - Upstream rust-v0.153.2

### Codex VL fixes

- Fleet TUIs now require a verified cell identity before attaching to a shared
  app-server; unverified sessions use an embedded fallback with a visible diagnostic,
  while explicit shared endpoints are rejected.
- Catalog-supplied `persistent_instructions` and Guardian `node_repl_policy` have
  an 8 KiB byte cap at every ingress and consumer: oversized values are rejected,
  never truncated, and Guardian fails closed with a clear diagnostic.
- Release infrastructure keeps V8 guarded, runs SDK checks on hosted runners,
  refreshes zlib snapshot URLs, and preserves the README ASCII policy.
- A dedicated CI gate covers the identity and model-message cap paths.

## 0.153.2-vl.1 - Upstream rust-v0.153.2

### Upstream

- Rebases Codex VL on OpenAI Codex `rust-v0.153.2`, including the GPT-6-Astra
  model catalog and the latest public model-tier updates.
- Hooks activity is visible in the TUI, with Vim undo/redo and app-server
  reconnect improvements for longer-running sessions.
- App-server adds asynchronous user-input requests and richer thread metadata,
  while `[tui].disable_paste_burst` and experimental context management expand configuration control.

### Codex VL fixes

- Loop and Vivling slash commands keep the TUI alive when a user precondition
  fails, including owner or delegation requests with the brain switched off.
- `manage_loops` retains access to its jobs after resume, and loop ticks accept
  fenced or prefixed JSON replies from any provider.
- The expression planner no longer floods the log database, while terminal
  turn reconciliation removes phantom “Working” state and stale queued input.
- Esc interrupts a running task even when a long tool hides the status row, and
  `/loop` usage now lists delegate, strategy, delegation, and owner commands.
- Termux: the npm launcher now fixes its own shebang on install, so `codex-vl` starts without `termux-exec` tricks.

## 0.150.2 - Restore embedded TUI loop routing

### Fixed

- Restored the pre-merge routing for the `manage_loops` dynamic tool in the
  embedded TUI app server: `manage_loops` calls reach the loop controller again
  instead of being rejected with "TUI dynamic tools require an active external
  task". The upstream gate still applies to every other dynamic tool in
  embedded sessions.
- Added two regression tests guarding the embedded TUI routing boundary.

## 0.150.1 - Upstream rust-v0.150.1

Update onto the upstream OpenAI Codex `rust-v0.150.1` stable release, published
on the `latest` channel. The complete Codex VL workflow layer remains present
across goal state, loop jobs, Vivling, the VL interface, remote control,
app-server integration, and the fork-owned package and update channels.

### Fixed

- The `manage_loops` built-in dynamic tools are now injected only for the TUI
  client. Every `thread/start` used to receive them, including app-server
  clients with no loop controller on their side of the connection.
- Update banners render the fork's own channels again. The history cell
  snapshots had pinned the upstream package name and installer, so the redirect
  the updater already performed was never reflected in the copy.
- Two fork test sites imported `ImageDetail` from its pre-0.150 module path,
  and the fork retry harness in the installed-apps suite lost its attempt
  counter to an upstream-shaped merge; both compile and run again.

## 0.147.0 - Upstream rust-v0.147.0

Update onto the upstream OpenAI Codex `rust-v0.147.0` stable release, published
on the `latest` channel. Upstream tagged it a day after `0.147.0-alpha.13`; the
two are siblings off the same parent and differ only in the workspace version.
The full Codex VL workflow layer remains present across goal state, loop jobs,
Vivling, the VL interface, remote control, app-server integration, and the
fork-owned package and update channels.

### Fixed

- An `AGENTS.md` created during a session — which is what `/init` does — is now
  discovered. Discovery cached its result under the environment selection, which
  does not change when a file appears, so the session kept reporting that none
  existed.
- The model catalog parses again. Upstream added its own legacy
  `base_instructions` key whose serializer also flattens `ModelInfo`, so with the
  fork field serialized too the catalog carried the key twice.
- Models whose catalog entry ships no instruction template get the fork's
  fallback instructions again. The merge had replaced that branch with upstream's,
  which returns an empty string; a behavioural test now fails if it is emptied
  again.
- musl packaging keeps working: the Linux workflows extract exactly two V8
  checksums per target from the versioned manifest, and upstream ships that file
  for 150.4.0 with Windows entries only.

### Changed

- The state runtime keeps the fork's VL migration reconciliation around
  upstream's refactored DB open path.
- `manage_loops` still resolves as a dynamic tool call, after upstream's new
  rejection path for requests belonging to abandoned side threads.
- MCP startup keeps its bounded tools-list retry, adapted to upstream's new
  catalog item limit.

### Removed

- The Termux TLS root patch carried here for parity: upstream removed the client
  it wrapped and `reqwest` left the crate's dependencies, so it no longer
  compiled.
- The fork's by-name session lookup test, superseded by upstream's
  `named_session_lookup` and its own tests.

## 0.146.1 - Upstream rust-v0.146.1

Update on the upstream OpenAI Codex `rust-v0.146.1` patch release (backported
safer cyber-model auto-review defaults). The merge touched no fork-owned path.
Published on the `next` channel; `latest` later moved to `0.147.0`. (Amended:
this entry originally described a gated candidate.)

The full Codex VL workflow layer remains present across goal state, loop jobs,
Vivling, the VL interface, remote control, app-server integration, and the
fork-owned package and update channels.

### Fixed

- The Windows installer resolves releases from the fork only. It previously
  preferred upstream's distribution channel with an upstream GitHub fallback,
  so running it installed upstream Codex under the fork's name.
- The message-history batch reader falls back when advisory file locking is
  unsupported, matching every other lock call site. Without it, batch history
  reads failed outright on Android/Termux storage.

## 0.146.0 - Upstream rust-v0.146.0 candidate

Candidate update on the final OpenAI Codex `rust-v0.146.0` tag. The full
Codex VL workflow layer remains present across goal state, loop jobs, Vivling,
the VL interface, remote control, app-server integration, and the fork-owned
package and update channels.

The merge retains the no-upstream-installer contract, MCP startup retry and
provider tool-namespace support, while adapting the state runtime to upstream's
resolved SQLite configuration. Legacy Vivling loop migrations remain reconciled
before upstream state migrations run. Publication remains subject to the
feature-register audit and sanitized artifact verification.

## 0.145.0 - Upstream rust-v0.145.0 stable

Stable release based on the final OpenAI Codex `rust-v0.145.0` tag. The full
Codex VL workflow layer remains available across goals, loop jobs, Vivling,
the VL interface, remote control, app-server integration, fork-owned update
surfaces, and the Linux, macOS, and Android package lanes.

This release also carries forward the explicit MCP environment-name
allowlists introduced in 0.144.8. Linux x64, Linux arm64, and Android arm64
ship native packages; macOS arm64 remains a source-build package with its
fail-closed prerequisite checks.

## 0.144.8 - Explicit MCP environment allowlists

Stable patch on the OpenAI Codex `rust-v0.144.6` base. The `mcp add`
command now accepts repeatable `--env-var NAME` options for local stdio
servers whose identity or routing data already exists in the launching
process environment.

Only variable names are validated and persisted. Values are resolved from
the live process environment when the MCP server starts, remain behind the
existing cleared-environment boundary, and are never copied into
`config.toml`. Duplicate names collapse to their first occurrence.

## 0.144.7 - Darwin npm 12 install diagnostics

Stable patch on the OpenAI Codex `rust-v0.144.6` base. On Apple Silicon,
both launchers now distinguish a missing platform package, an installed
platform package whose native binary is absent, and a native binary that
cannot be executed. Each state reports an actionable, path-aware error.

The macOS recovery guidance now uses the complete npm 12 command with an
explicit install target, a script allowlist limited to `@mmmbuto/codex-vl`,
and foreground lifecycle output. README, postinstall diagnostics, fork
identity pins, and CI fixtures enforce the same fail-closed contract.

This release also carries upstream's refreshed bundled model metadata and
GPT-5.6 prompt/context update from `rust-v0.144.6`.

## 0.144.6 - Verified ripgrep packaging hotfix

Stable fork hotfix on the final OpenAI Codex `rust-v0.144.5` base. Linux x64
and Linux arm64 packages now install the target-specific ripgrep 15.1.0
artifact from the canonical DotSlash manifest instead of copying the host
binary or generating a GNU grep compatibility stub.

Every artifact is verified against its pinned size and SHA-256 digest before
extraction. Missing targets, malformed manifests, download failures, and
integrity mismatches fail the package build closed.

## 0.144.5 - Upstream rust-v0.144.5 next candidate

Candidate release on npm `next`, based on the final OpenAI Codex
`rust-v0.144.5` patch release. Upstream expands dangerous-command detection to
cover additional forced recursive deletion forms before execution.

Loop-job refreshes now reuse the process-owned state database handle instead
of reopening and migrating SQLite from the event consumer path. A passive
refresh failure is logged and leaves the TUI running rather than terminating
the app-server session.

## 0.144.4 - Upstream rust-v0.144.4 next candidate

Candidate release on npm `next`, based on the final OpenAI Codex
`rust-v0.144.4` patch release. Upstream reports no user-facing changes in this
patch; the complete Codex VL feature layer and Android/Termux carry-forward
remain intact.

Fork compatibility hardening allows model catalog entries to omit
`base_instructions`. Explicit configuration overrides remain authoritative,
while missing or blank catalog instructions safely fall back to the embedded
base instructions instead of producing an instructionless session.

## 0.144.3 - Upstream rust-v0.144.3 next candidate

Candidate release on npm `next`, based on OpenAI Codex `rust-v0.144.3`.
The complete upstream delta is combined with the complete Codex VL feature
layer, including goals, loop jobs, Vivling runtime and delegation, VL UI,
remote control, app-server integration, fork identity, and Android/Termux
carry-forward. npm `latest` remains on `0.144.1` and `stable` on `0.143.0`.

Upstream adds the advanced reasoning picker and persisted per-thread reasoning
effort, and rolls Guardian review prompting back to the validated layout.

## 0.140.0 - Upstream rust-v0.140.0 final

Stable release on the npm `latest` tag, based on the OpenAI Codex
`rust-v0.140.0` release line. Upstream-tracking release: no fork-specific
feature changes beyond the standing Codex VL workflow layer and the
Termux/Android carry-forward. Validated on device (Linux, macOS, Termux —
AI-guided surface reports, PASS). The `stable` dist-tag stays on `0.135.0`.

## 0.133.0 - Upstream rust-v0.133.0 final

Stable release on the npm `latest` tag.

Based on the OpenAI Codex `rust-v0.133.0` release line.

### Vivling improvements

- Vivlings now actually speak. The CRT footer phrase and every `/vl`
  reply flow through the same LLM channel the Vivling chooses for chat,
  gated by an atomic daily budget (stage defaults: Baby 50, Juvenile
  100, Adult 200).
- The greeting at session start now comes out in the configured
  language instead of a generic English placeholder.
- CRT phrases and proactive replies no longer cut mid-word.
- `Ctrl+J` opens a bordered, scrollable chat panel that wraps long
  replies cleanly and adapts to narrow terminals (Termux portrait to
  wide desktop).
- New `/vivling crt-brain` controls: `show` exposes mode, budget, calls
  today and remaining head-room; `budget unlimited` lifts the cap for
  unmetered wrappers; `reset-budget` zeroes daily counters without
  waiting for UTC rollover.
- Bond mechanics rate tuning makes companion progression more
  noticeable during real work sessions.

### Platform coverage

- New Linux arm64 prebuilt target: `@mmmbuto/codex-vl-linux-arm64`
  (`aarch64-unknown-linux-musl`) is now built in CI alongside Linux x64
  and Android arm64. The launcher (`bin/codex.js`,
  `bin/codex-exec.js`) auto-resolves the right native package on
  Raspberry Pi (64-bit OS), AWS Graviton, Apple Silicon Linux VMs, and
  other ARM64 Linux hosts.
- The macOS arm64 post-install build (the only target that compiles on
  the user's machine) now runs a preflight that checks for Xcode
  Command Line Tools, the Rust toolchain, `rustup`, and the
  `aarch64-apple-darwin` target before invoking `cargo build`. Each
  missing item is reported with the exact command to install it, so a
  fresh Mac no longer fails partway through compilation with cryptic
  linker or toolchain errors.

### Upstream merge

- Goals are now enabled by default, backed by a dedicated `goals.db`
  store, with `create_goal`, `update_goal`, and `get_goal` exposed as
  model tools.
- `codex remote-control` runs as a foreground command, waits for
  readiness, reports machine status, and keeps explicit daemon-style
  `start` / `stop` commands.
- Permission profiles gained list APIs, inheritance, managed
  `requirements.toml` support, runtime refresh behavior, and stronger
  Windows sandbox integration.
- Plugin discovery is easier to inspect, with marketplace-aware list
  output, installed versions, visible marketplace roots, and remote
  collection support.
- Extensions can observe more lifecycle events, including subagent
  start / stop, tool execution, turn metadata, and async approval /
  turn processing.

Full upstream release notes:
https://github.com/openai/codex/releases/tag/rust-v0.133.0

### Changed

- npm package metadata moved from the pre-release lane to `0.133.0`.
- Workspace and lockfile package versions aligned with the upstream
  `0.133.0` release.
- Public install, source-build, and native artifact staging surfaces
  remain fork-owned for `DioNanos/codex-vl` and `@mmmbuto/codex-vl`.

### Preserved fork features

- All Codex VL workflow commands and fork-owned modules are preserved
  across the upstream stable merge: `/vivling`, `/vl`, `/loop`,
  `/remote-control`, Vivling Memory V2 (V10 schema, bond mechanics,
  brain expression channel, daily LLM budget, language-aware boot,
  Ctrl+J chat panel, CRT footer).
- The fork-safe managed-update channel and the disabled standalone
  auto-updater are kept (the daemon never fetches the upstream
  installer script).
- The fork feedback channel points to `DioNanos/codex-vl/issues` and
  the announcement tip source is `DioNanos/codex-vl/main/announcement_tip.toml`.

Per aspera ad astra.

## 0.132.0 - Upstream rust-v0.132.0 final

Stable release on the npm `latest` tag.

Based on the OpenAI Codex `rust-v0.132.0` release line.

### Changed

- npm package metadata moved from the pre-release lane to `0.132.0`.
- Public install, source-build, and native artifact staging surfaces remain
  fork-owned for `DioNanos/codex-vl` and `@mmmbuto/codex-vl`.

### Preserved fork features

- Existing Codex VL workflow commands and fork package identity are preserved
  across the upstream stable merge.

### Upstream merge

- Python SDK authentication now supports API key login, ChatGPT browser and
  device-code flows, account inspection, and logout APIs.
- Python turn APIs are easier to use for text-only workflows and return richer
  turn results for handle-based runs.
- `codex exec resume` accepts `--output-schema`, so resumed automations can keep
  session context while still enforcing structured JSON output.
- TUI startup probes are batched before the first interactive frame.
- Remote executor registration can use standard Codex auth.
- App-server turns preserve requested image fidelity, including original local
  image detail, across user inputs and image-producing tools.

## 0.131.0 - Upstream rust-v0.131.0 final

Based on the OpenAI Codex `rust-v0.131.0` release line.

### Added

- Public development journal pointer in the `/vivling` README section, linking
  to the Codex VL dev journal at `dev.mmmbuto.com/vivling`.
- Upstream additions inherited from `rust-v0.131.0`, including data-driven
  service-tier controls, blended token usage display, permissions/approval
  mode surface, effective workspace roots, responsive Markdown tables, unified
  `@` mention picker across files, plugins and skills, plugin marketplace CLI
  flows, daemon-managed `codex remote-control` with runtime enable/disable and
  status reads, and the new `codex doctor` diagnostic command.

### Changed

- npm `latest` line moved from `0.130.0` to `0.131.0` with matching platform
  packages for Linux x64, Termux Android arm64, and a macOS arm64 source-build
  package that compiles locally with Cargo during npm postinstall.
- README release-channels section rewritten around `0.131.0 latest`. The npm
  `next` tag is now described as reserved for the next upstream alpha lane
  after a stable release rather than tracking the current pre-release line.

### Preserved fork features

- `/loop`, `/goal`, `/vivling`, `/vl` and the Vivling runtime, lifecycle, and
  CRT layer are preserved across the merge.
- Fork-owned update, doctor and install surfaces stay on `@mmmbuto/codex-vl`
  with the fork repository as the package source. No upstream installer URL
  is reintroduced into fork-owned scripts.

## 0.131.0-alpha.23 - Fork-safe remote-control bootstrap

Pre-release on the npm `next` tag. `latest` stayed on `0.130.0`.

### Changed

- npm-installed and bun-installed Codex VL now keep `autoUpdateEnabled=false`
  for the app-server daemon and short-circuit the managed updater path so the
  current fork binary stays in control of remote-control sessions.
- Standalone, Brew and other install contexts keep their previous managed
  path, so this is a targeted change for npm/bun installs only.

## 0.131.0-alpha.22 - Fork identity hardening (F-bis)

Pre-release on the npm `next` tag. `latest` stayed on `0.130.0`.

### Changed

- Doctor, updater, npm registry hints and release links all point to
  `@mmmbuto/codex-vl` and the fork repository, with regression tests pinned
  against silent upstream reintroduction.
- `install_native_deps` derives the owner/repository from the workflow URL
  passed by fork pipelines instead of a hardcoded upstream value, while the
  historical default URL is preserved as a documented placeholder.

## 0.131.0-alpha.20 - Upstream sync rust-v0.131.0-alpha.20

First Codex VL alpha aligned with the upstream `rust-v0.131.0` alpha line.

### Changed

- Merge target is the explicit upstream tag `rust-v0.131.0-alpha.20`, not the
  upstream branch head, to keep the conflict surface bounded.
- TUI session resume is adopted from upstream and the previous inline helpers
  are gated to test builds where appropriate.
- Release profile uses thin LTO for slightly smaller binaries.

### Internal

- Merge-safety refactors landed before the upstream sync:
  - chatwidget `slash_dispatch` boundary extraction so most VL slash logic
    lives outside the upstream-heavy dispatch surface.
  - `app/loop_controller` split from a single large module into a small set
    of focused submodules behind a stable internal facade.
  - `bottom_pane` VL boundary extraction so the upstream-heavy view module
    no longer carries VL-specific logic blocks.
  These changes are not directly user-visible. They exist to keep upstream
  merges practical without changing public behavior.

## 0.130.0 - Upstream rust-v0.130.0

Based on the OpenAI Codex `rust-v0.130.0` release line.

### Added

- npm `latest` Linux x64 and Termux Android arm64 prebuilt packages plus a
  macOS arm64 source-build package that compiles locally with Cargo during
  npm postinstall.

### Changed

- `/goal` lifecycle completion is now clear-on-complete to avoid stale
  completion state across sessions.
- MCP startup snapshot behavior and stdio retry hardening kept aligned with
  the upstream `0.130.0` runtime.
- SQLite state contention hardening retained for multi-session local use.

### Preserved fork features

- Vivling runtime, identity, persistence, lifecycle and brain profile routing.
- `/loop` session-scoped recurring jobs.
- `/goal` workflow alongside upstream `/goal` semantics.

## 0.128.3 - Local Linux rebuild

- Rebuilds and reinstalls the local Linux package from the aligned Forge/GitHub base.
- Keeps the 0.128.2 packaging corrections while refreshing the installed CLI payload.

## 0.128.2 - Corrected npm packaging

- Publishes Linux x64 and Termux Android arm64 npm packages with native prebuilts.
- Keeps macOS npm installs on local Cargo builds instead of shipping unsigned macOS binaries.
- Supersedes the deprecated `0.128.1` candidate packages.

## 0.128.1 - macOS npm packaging cleanup

- Changed the macOS npm package to build native binaries locally with Cargo.
- Removed unsigned macOS binary payloads from the candidate packaging flow.

## 0.128.0-vl.0 - Upstream Sync

Based on the OpenAI Codex `rust-v0.128.0` release line.

### Changed

- Preserves the upstream `0.128.0` feature set as the base, including `/goal`
  workflows and related app-server/TUI APIs.
- Keeps Codex VL additions as additive layers: Vivling, `/loop`, fork packaging,
  and platform build paths.

## 0.126.0-vl.0 - Upstream Sync and Vivling CRT Foundation

Based on the OpenAI Codex `0.126.0` release line.

### Added

- First modular Vivling CRT renderer foundation for the bottom terminal strip.
- Baby lifecycle CRT scripts for idle, play, eat, sleep, and work states.
- Focused 15+ZED Vivling roster foundation with Common, Rare, Legendary, and Mythic tiers.
- Vivling brain model guide for profile-based model resolution.

### Changed

- Vivling CRT output now prioritizes compact visual state and short speech over dense metrics.
- Lifecycle activity no longer floods the expanded Vivling chat log.
- Public README/docs were reduced to a smaller release-facing surface.
- Internal concept art, roadmap notes, and release lane notes moved under `.docs/`.

### Removed

- Old generated 90-species Vivipendium and EPUB prototype docs from the public docs tree.

## 0.124.0 - First Public Release

Based on OpenAI Codex `0.124.0`.

### Added

- Side-by-side `codex-vl` CLI packaging under `@mmmbuto/codex-vl`.
- `/loop` session-scoped loop supervision for recurring checks and long-running work.
- `/vivling` persistent companion system with local state, levels, species, cards, and work memory.
- `/vl <message>` direct Vivling chat shortcut.
- Adult Vivling brain dispatch through normal Codex profiles and model providers.
- Vivling loop-awareness and loop-owner experiments.
- Initial public README positioning for Codex VL.
- Initial README hero asset under `docs/assets/`.

### Changed

- `/vl` now routes to the Vivling brain when the active Vivling is adult, brain-enabled, and has a brain profile.
- `/vl` keeps a local fallback reply path when the brain is not ready.
- `/vivling` remains the controlled command surface rather than becoming free-form chat.

### Experimental

- Vivling learning from work summaries and loop events.
- Vivling brain profiles backed by custom model catalog entries.
- Linux and Termux/Android packaging flow.
- GitHub public release pipeline.

### Known Gaps

- Public release workflow still needs hardening.
- npm platform packaging needs cleanup before broad publish.
- Merge-safety refactor is still pending for slash commands, app events, migrations, and TUI integration hooks.
- Vivling genetics, bonding, spawn inheritance, and richer roster UX are still future work.

## Upstream Codex

For upstream OpenAI Codex changes, see the official OpenAI Codex release notes.
