# Changelog

All notable Codex VL changes are tracked here.

Codex VL tracks OpenAI Codex upstream, but this changelog only covers fork-specific work.

## 0.132.0 - Stable upstream sync and Codex VL workflow layer

Stable release on the npm `latest` tag.

Based on the OpenAI Codex `rust-v0.132.0` release line.

### Added

- `/remote-control` command inside the TUI for daemon `status`, `start`,
  `stop`, and `restart`, delegating lifecycle work to the canonical
  `codex-vl remote-control` binary path instead of duplicating daemon logic.
- Explicit guarded replies for `/remote-control on`, `off`, `enable`, and
  `disable`; enrollment toggles wait for a reusable upstream client surface
  rather than adding fork-only pairing code.
- Generalized `/loop` payload foundation for future structured loop actions,
  while preserving raw prompt storage compatibility for existing loop jobs.
- Fork-owned standalone install script and source-build documentation hygiene,
  so public install surfaces point at `DioNanos/codex-vl` and
  `@mmmbuto/codex-vl`.

### Changed

- Vivling species registry and runtime command handlers were split behind the
  same internal facades, keeping `/vivling` behavior stable while reducing
  future upstream merge conflicts.
- Fork identity shim tests now pin npm wrapper package aliases, reinstall
  guidance, repository identity, public install scripts, and source-install
  docs against accidental upstream drift.
- npm package metadata moved from the pre-release lane to `0.132.0`.

### Preserved fork features

- `/loop`, `/goal`, `/vivling`, `/vl`, Vivling lifecycle/CRT state, and fork
  package identity are preserved across the upstream stable merge.
- `/goal` still clears completed goals while blocked goals remain inspectable.

### Upstream merge

- Python SDK authentication and turn API updates.
- `codex exec resume --output-schema`.
- Batched TUI startup probes.
- Remote executor authentication updates.
- App-server image fidelity improvements.

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
