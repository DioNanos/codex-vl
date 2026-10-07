#!/usr/bin/env bash
# Apply the fork CI overlay to a tree that is about to become GitHub main.
# Idempotent: already applied -> no-op; neither applicable nor applied -> hard failure.
# Dependencies are checked before anything is written.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
patch="$here/fork-ci-overlay.patch"
py="${PYTHON:-python3}"

if ! command -v git >/dev/null 2>&1; then
  echo "overlay: git is required; nothing was changed" >&2; exit 1
fi
if ! "$py" -c 'import yaml' >/dev/null 2>&1; then
  cat >&2 <<EOM
overlay: PyYAML is not available for '$py'; nothing was changed.
Provide it, then run the script again:
  python3 -m venv ~/.venvs/fork-overlay && ~/.venvs/fork-overlay/bin/pip install pyyaml==6.0.2 && PYTHON=~/.venvs/fork-overlay/bin/python $0
or, with uv:
  uv run --no-project --with pyyaml==6.0.2 $0
EOM
  exit 1
fi

if git apply --check "$patch" 2>/dev/null; then
  git apply "$patch"; echo "overlay: applied"
elif git apply -R --check "$patch" 2>/dev/null; then
  echo "overlay: already applied"
else
  echo "overlay: patch neither applies nor is applied; regenerate it (see MAINTAINER.md)" >&2; exit 1
fi
"$py" "$here/check_fork_ci_overlay.py" .
