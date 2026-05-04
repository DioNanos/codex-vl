# Release Candidate Manual Report - Termux (0.125.1)

**Goal**: interactive validation of `codex-vl` behavior that cannot be fully trusted to unattended runtime checks

## Target

- platform: termux
- package version: `0.125.1`
- test date: 2026-04-26

## Manual checks

- `NOT RUN` first interactive launch
- `NOT RUN` thread start/resume/fork in TUI
- `NOT RUN` `/loop` interaction in TUI
- `NOT RUN` `/vivling` interaction
- `NOT RUN` `/vl` interaction
- `NOT RUN` visual/UX sanity

## Notes

- Non-interactive checks completed and documented in `test-report/automated/2026-04-26_termux_0.125.1_runtime.md`.
- `codex exec` dynamic tool call execution is still unavailable by design in current `exec` mode; loop coverage in this report was validated through `manage_loops` add/list/remove tool lifecycle.
- The `workspace write-read` prompt in `qa/runtime-suite/run_cli_runtime_suite.sh` was fixed to avoid malformed command quoting.

## Release recommendation

- `GO` for runtime/npm validation
- rationale: non-interactive runtime checks passed after the prompt fix; interactive TUI checks remain outside this Termux pass.
