# Codex-VL Test Reports

This directory is intentionally small. Its job is to give the next validating AI
one clear path through release-candidate testing without old reports, stale
templates, or local machine noise.

## Start Here

Use `AI_GUIDED_TEST_SUITE.md` as the canonical checklist.

The suite is AI-guided by design: the AI runs commands, inspects outputs,
classifies PASS/FAIL, and writes a sanitized report. Do not replace this with a
blind runner unless a maintainer explicitly asks for that.

The manual AI validation itself is part of the release test. Reports in this
directory must not take their verdict from shell runner scripts, bulk `sh`
pipelines, TSV summaries, or aggregate exit codes. The validating AI must run
checks one by one, inspect the output, and explain each PASS/FAIL call.

Do not run Rust builds or `cargo test` from the post-install `test-report/`
flow. Compile-time checks belong to the build/merge phase; this directory
validates the installed CLI and the AI-operated runtime surface.

## Current Files

- `AI_GUIDED_TEST_SUITE.md`: reusable guide for future validating AIs.
- `manual/2026-05-09_0.130.0_ai_guided_surface.md`: 0.130.0 Termux Android
  arm64 post-install command surface, MCP listing, feature listing, runtime
  exec, read/write, network, TUI/Vivling startup, and goal/loop lifecycle
  evidence.

## Report Rules

Keep only the latest useful release evidence here. Remove stale reports once a
newer suite supersedes them.

Reports must be sanitized before they are committed:

- no absolute local paths
- no private hosts or IPs
- no tokens, usernames, or account identifiers
- no raw environment dumps
- no unrelated process lists
- no unredacted MCP secrets

Use placeholders such as `<repo>`, `<tmp-workspace>`, and `<platform-device>`
when concrete local values are not safe for public GitHub mirroring.
