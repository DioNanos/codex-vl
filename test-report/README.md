# Codex-VL Test Reports

This directory is intentionally small. Its job is to give the next validating AI
one clear path through release-candidate testing without old reports, stale
templates, or local machine noise.

## Start Here

Use `AI_GUIDED_TEST_SUITE.md` as the canonical checklist.

The suite is AI-guided by design: the AI runs commands, inspects outputs,
classifies PASS/FAIL, and writes a sanitized report. Do not replace this with a
blind runner unless a maintainer explicitly asks for that.

## Current Files

- `AI_GUIDED_TEST_SUITE.md`: reusable guide for future validating AIs.
- `automated/2026-05-06_0.128.4_next_build_publish.md`: 0.128.4 build,
  package, publish, and Forge push evidence.
- `automated/2026-05-07_0.128.5_merge_build_publish.md`: 0.128.5 upstream
  merge, Linux optimized build evidence, and remaining release gates.
- `automated/2026-05-07_0.129.0_linux_post_install_suite.md`: 0.129.0 Linux
  post-install command surface, focused Rust tests, npm dist-tag, read/write,
  and sanitized report evidence.
- `manual/2026-05-06_0.128.4_ai_guided_surface.md`: 0.128.4 AI-guided
  command, tool, TUI, read/write, patch, and runtime smoke report.
- `manual/2026-05-06_termux_persist_extended_history_warning.md`: Termux
  source update and startup deprecation-warning fix evidence.

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
