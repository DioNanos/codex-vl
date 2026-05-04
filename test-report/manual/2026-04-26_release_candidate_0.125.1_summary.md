# Manual UI Report - Release Candidate 0.125.1

**Goal**: interactive validation of `codex-vl` behavior that cannot be trusted to unattended runtime checks

## Target

- platform: Linux x64 local workstation, plus release artifacts for macOS ARM64 and Android/Termux ARM64
- package version: `0.125.1`
- test date: 2026-04-26

## Manual checks

- `PASS` release build order and artifact handoff: macOS, then Linux, then Android
- `PASS` release acceptance for npm publish after all platform payloads were verified
- `PASS` GitHub release creation with public tag `v0.125.1`
- `PASS` Vivling footer animation regression covered by unit test and included in release commit
- `PASS` local npm global install cleaned and repointed to `@mmmbuto/codex-vl@0.125.1`
- `PASS` Ollama restored as enabled and active system service
- `PASS` custom/Ollama non-interactive `codex-vl exec` smoke with `deepseek-v4-flash:cloud`
- `PASS` DeepSeek via Ollama sees `manage_loops` and attempts to call it
- `PENDING` first interactive TUI launch on the user's local install
- `PENDING` thread start/resume/fork in TUI
- `PENDING` `/loop` interaction in TUI with a responsive custom provider; `exec` mode exposes the tool but does not execute dynamic tool calls
- `PENDING` `/vivling` and `/vl` hands-on interaction quality
- `PENDING` final visual/UX sanity screenshots

## Notes

- User requested Mac-first packaging so the Mac could be powered off after a good macOS artifact. The macOS artifact was verified before Linux and Android were dispatched.
- Linux and Android builds succeeded; all artifacts were copied into the local release staging area and validated before npm assembly.
- npm `0.125.1` was published only after Linux, macOS, and Android payloads included required binaries.
- Public release URL: `https://github.com/DioNanos/codex-vl/releases/tag/v0.125.1`
- The old manual `codex-vl` and `codex-vl-exec` symlinks to `0.125.0` were removed. Both aliases now resolve to the npm install.
- Ollama boot service now starts successfully from systemd and serves `localhost:11434`.
- DeepSeek was asked directly whether it sees loop tooling; it answered that it sees `manage_loops`, then attempted the `manage_loops action=list` call twice.
- The attempted call fails only at the `codex exec` runtime boundary because dynamic tool calls are not supported in exec mode.

## Screenshots

- not captured

## Release recommendation

- `GO`
- rationale: packaging, npm publish, public release, clean local npm install, version smoke, exec entrypoint, Ollama service/API, custom-provider exec, and binary payload validation passed. Remaining pending checks are interactive UI acceptance items, not blockers for the already-published npm release.
