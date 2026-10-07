#!/usr/bin/env bash
# Apply the fork CI overlay to a tree that is about to become GitHub main.
# Idempotent: already applied -> no-op; neither applicable nor applied -> hard failure.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
patch="$here/fork-ci-overlay.patch"
if git apply --check "$patch" 2>/dev/null; then
  git apply "$patch"; echo "overlay: applied"
elif git apply -R --check "$patch" 2>/dev/null; then
  echo "overlay: already applied"
else
  echo "overlay: patch neither applies nor is applied; regenerate it (see runbook)" >&2; exit 1
fi
python3 "$here/check_fork_ci_overlay.py" .
