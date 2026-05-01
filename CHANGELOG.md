# Changelog

All notable Codex VL changes are tracked here.

Codex VL tracks OpenAI Codex upstream, but this changelog only covers fork-specific work.

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
