# Runtime Suite

This directory is the portable runtime validation zone for `codex-vl`.

Goal:
- clone the repo on any target machine
- point the suite at the installed `codex-vl`
- run the same no-crash/runtime checks on macOS, Linux, or Termux
- generate a report you can keep with the release candidate

This suite is intentionally focused on the installed CLI, not only on source-tree
build validation.

## What It Covers

Base runtime checks:
- version and binary path resolution
- top-level help and subcommand help routing
- login status
- MCP and features listing
- completion generation
- app-server schema generation
- wrapper routing through `node .../bin/codex.js`
- installed binary linkage inspection when supported

Optional live checks:
- non-interactive `exec`
- workspace write/read smoke
- network smoke

## Usage

From the repo root:

```bash
qa/runtime-suite/run_cli_runtime_suite.sh
```

With live checks enabled:

```bash
qa/runtime-suite/run_cli_runtime_suite.sh --live
```

Write reports somewhere else:

```bash
REPORT_ROOT="$HOME/runtime-reports" qa/runtime-suite/run_cli_runtime_suite.sh --live
```

Use a non-default installed command:

```bash
CODEX_CMD=/opt/homebrew/bin/codex-vl qa/runtime-suite/run_cli_runtime_suite.sh
```

Use an explicit `codex-exec` binary:

```bash
CODEX_EXEC_CMD=/custom/path/codex-exec qa/runtime-suite/run_cli_runtime_suite.sh --live
```

## Output

Reports are written under:

```text
qa/runtime-suite/reports/latest/<platform>/<timestamp>/
```

Each run generates:
- one markdown report
- one TSV result table
- one `logs/` directory with per-check stdout/stderr
- one generated schema output directory when that check runs

## Platform Notes

### macOS
- linkage check uses `otool -L`

### Linux
- linkage check uses `readelf -d`

### Termux / Android
- linkage check uses `readelf -d` when available
- live checks are especially useful here because they validate the installed package surface

## Release Workflow

Recommended use before a private release:

1. Run local source tests where appropriate.
2. Install or update the candidate package on the target machine.
3. Run this runtime suite.
4. Keep the generated markdown report with the release notes or test evidence.

## Planned Expansion

This zone is the base for broader parity with the older `codex-termux` validation flow:
- richer release reports
- extended platform-specific smoke checks
- alignment with the Rust integration `tests/suite` layer
