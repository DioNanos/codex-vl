# Test Report

This directory is the dedicated validation area for `codex-vl` release candidates.

Use it to keep install notes, automated runtime validation, screenshots, and
manual acceptance evidence before promoting a `develop` build into a clean
GitHub `main` release.

## Validation policy

`codex-vl` should test everything that is realistically automatable first.
Manual/UI checks come after that and should only cover what cannot be trusted to
an unattended runtime suite.

This means:

- automatic checks should cover the widest possible command and runtime surface
- `/loop` behavior should be validated automatically where the agent can create,
  trigger, update, list, disable, and remove loop jobs on its own
- non-interactive CLI, packaging, install, and first-start checks should stay in
  the automated section
- manual testing should be reserved for TUI behavior, visual flows, and product
  judgment calls

## Folder structure

- `automated/`
  - unattended runtime reports
  - install smoke results
  - platform-specific CLI/runtime validation
  - loop self-management checks
- `manual/`
  - UI/TUI walkthroughs
  - screenshots
  - subjective UX notes
  - Vivling interaction checks
  - final release acceptance summaries

## Automatic suite expectations

The automatic suite should validate as much as possible from the installed
package, not just from the source tree.

Expected automatic coverage:

- package/version checks
- top-level help and subcommand help routing
- login status and safe auth entry points
- MCP listing and feature listing
- completion generation
- app-server schema generation
- non-interactive `exec` flows
- workspace write/read checks
- network smoke where allowed
- binary linkage/package sanity
- `/loop` lifecycle checks that are feasible without a human in the TUI
- no-crash first-start validation on supported targets

Good automatic reports should look similar in spirit to the practical runtime
reports already used in `codex-termux`: clear PASS/FAIL status, explicit command
surface tested, and a short outcome summary.

## Manual/UI suite expectations

Manual validation should be tracked separately from the automatic suite.

Expected manual coverage:

- first interactive TUI launch
- thread start/resume/fork in the UI
- `/loop` interaction from the TUI surface
- `/vivling` and `/vl` interaction quality
- visual regressions
- release-candidate acceptance decision

## Suggested files

- `automated/YYYY-MM-DD_<target>_runtime.md`
- `automated/YYYY-MM-DD_<target>_install.md`
- `manual/YYYY-MM-DD_<target>_ui.md`
- `manual/YYYY-MM-DD_release_candidate_summary.md`

Templates:

- `AUTOMATED_RUNTIME_TEMPLATE.md`
- `MANUAL_UI_TEMPLATE.md`

This folder is intentionally versioned so each release candidate can carry its
own validation evidence.
