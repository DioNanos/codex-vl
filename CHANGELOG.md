# Changelog

All notable Codex VL changes are tracked here.

Codex VL tracks OpenAI Codex upstream, but this changelog only covers fork-specific work.

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
