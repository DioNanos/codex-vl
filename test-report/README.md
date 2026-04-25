# Test Report

This directory is the dedicated validation area for `codex-vl` release candidates.

Use it to keep install notes, runtime reports, screenshots, and manual test
evidence before promoting a `develop` build into a clean GitHub `main` release.

Recommended contents:

- install logs for side-by-side `~/.local` installs
- platform-specific smoke notes for Linux, macOS arm64, and Android/Termux
- runtime suite results from `qa/runtime-suite/run_cli_runtime_suite.sh`
- manual checks for `/loop`, `/vivling`, `/vl`, and normal Codex flows
- regressions found while validating the release candidate

Suggested naming:

- `YYYY-MM-DD_<target>_install.md`
- `YYYY-MM-DD_<target>_runtime.md`
- `YYYY-MM-DD_release_candidate_summary.md`

This folder is intentionally versioned so release candidates can carry their own
test evidence.
