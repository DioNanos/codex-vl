#!/usr/bin/env bash

set -u

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPORT_ROOT="${REPORT_ROOT:-$ROOT_DIR/reports/latest}"
LIVE_CHECKS=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --live)
      LIVE_CHECKS=1
      shift
      ;;
    --help|-h)
      cat <<'EOF'
Usage: qa/runtime-suite/run_cli_runtime_suite.sh [--live]

Options:
  --live   Run real exec/workspace/network smokes in addition to safe runtime checks
EOF
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

command -v python3 >/dev/null 2>&1 || {
  echo "python3 is required" >&2
  exit 2
}

resolve_realpath() {
  python3 - "$1" <<'PY'
import os, sys
print(os.path.realpath(sys.argv[1]))
PY
}

detect_platform_label() {
  local uname_s uname_m
  uname_s="$(uname -s)"
  uname_m="$(uname -m)"

  if command -v termux-info >/dev/null 2>&1 || [[ "${PREFIX:-}" == *"/com.termux/"* ]]; then
    echo "termux"
    return
  fi

  case "${uname_s}:${uname_m}" in
    Darwin:arm64) echo "macos-arm64" ;;
    Darwin:x86_64) echo "macos-x64" ;;
    Linux:x86_64) echo "linux-x64" ;;
    Linux:aarch64) echo "linux-arm64" ;;
    *) echo "$(printf '%s-%s' "$uname_s" "$uname_m" | tr '[:upper:]' '[:lower:]')" ;;
  esac
}

resolve_codex_cmd() {
  if [[ -n "${CODEX_CMD:-}" ]]; then
    echo "$CODEX_CMD"
    return
  fi
  command -v codex-vl 2>/dev/null || true
}

resolve_node_entry() {
  local codex_cmd="$1"
  resolve_realpath "$codex_cmd"
}

resolve_package_root() {
  local node_entry="$1"
  dirname "$(dirname "$node_entry")"
}

resolve_codex_exec_cmd() {
  if [[ -n "${CODEX_EXEC_CMD:-}" ]]; then
    echo "$CODEX_EXEC_CMD"
    return
  fi
  if command -v codex-exec >/dev/null 2>&1; then
    command -v codex-exec
    return
  fi
  local package_root="$1"
  find "$package_root/vendor" -path '*/codex/codex-exec' -type f 2>/dev/null | head -n 1 || true
}

resolve_installed_binary() {
  local package_root="$1"
  find "$package_root/vendor" -path '*/codex/codex' -type f 2>/dev/null | head -n 1 || true
}

safe_name() {
  printf '%s' "$1" | tr ' /:' '___' | tr -cd '[:alnum:]_.-'
}

CURRENT_PLATFORM="$(detect_platform_label)"
TIMESTAMP="$(date '+%Y%m%d-%H%M%S')"
OUT_DIR="$REPORT_ROOT/$CURRENT_PLATFORM/$TIMESTAMP"
LOG_DIR="$OUT_DIR/logs"
SCHEMA_DIR="$OUT_DIR/schema"
mkdir -p "$LOG_DIR" "$SCHEMA_DIR"

REPORT_MD="$OUT_DIR/CODEX_VL_RUNTIME_REPORT_${CURRENT_PLATFORM}_${TIMESTAMP}.md"
RESULTS_TSV="$OUT_DIR/CLI_RUNTIME_RESULTS_${TIMESTAMP}.tsv"

CODEX_CMD="$(resolve_codex_cmd)"
if [[ -z "$CODEX_CMD" ]]; then
  echo "codex-vl command not found. Set CODEX_CMD=/absolute/path/to/codex-vl" >&2
  exit 2
fi

CODEX_CMD_REAL="$(resolve_realpath "$CODEX_CMD")"
NODE_ENTRY="${CODEX_NODE_ENTRY:-$(resolve_node_entry "$CODEX_CMD")}"
PACKAGE_ROOT="$(resolve_package_root "$NODE_ENTRY")"
CODEX_EXEC_CMD="$(resolve_codex_exec_cmd "$PACKAGE_ROOT")"
INSTALLED_BINARY="$(resolve_installed_binary "$PACKAGE_ROOT")"

WORKSPACE_DIR="$OUT_DIR/workspace"
mkdir -p "$WORKSPACE_DIR"

PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0
CHECK_LINES=()

printf 'status\tcheck\tcommand\tlog\n' >"$RESULTS_TSV"

record_result() {
  local status="$1"
  local check_name="$2"
  local command_text="$3"
  local log_file="$4"

  case "$status" in
    PASS) PASS_COUNT=$((PASS_COUNT + 1)) ;;
    FAIL) FAIL_COUNT=$((FAIL_COUNT + 1)) ;;
    SKIP) SKIP_COUNT=$((SKIP_COUNT + 1)) ;;
  esac

  CHECK_LINES+=("- \`$status\` $check_name")
  printf '%s\t%s\t%s\t%s\n' "$status" "$check_name" "$command_text" "$log_file" >>"$RESULTS_TSV"
}

run_check() {
  local check_name="$1"
  local command_text="$2"
  local log_file="$LOG_DIR/$(safe_name "$check_name").log"

  if bash -lc "$command_text" >"$log_file" 2>&1; then
    record_result "PASS" "$check_name" "$command_text" "$log_file"
  else
    record_result "FAIL" "$check_name" "$command_text" "$log_file"
  fi
}

skip_check() {
  local check_name="$1"
  local reason="$2"
  local log_file="$LOG_DIR/$(safe_name "$check_name").log"
  printf '%s\n' "$reason" >"$log_file"
  record_result "SKIP" "$check_name" "$reason" "$log_file"
}

run_check "version" "$(printf '%q --version' "$CODEX_CMD")"
run_check "top-level help" "$(printf '%q --help' "$CODEX_CMD")"
run_check "exec help" "$(printf '%q exec --help' "$CODEX_CMD")"
run_check "review help" "$(printf '%q review --help' "$CODEX_CMD")"
run_check "login help" "$(printf '%q login --help' "$CODEX_CMD")"
run_check "logout help" "$(printf '%q logout --help' "$CODEX_CMD")"
run_check "resume help" "$(printf '%q resume --help' "$CODEX_CMD")"
run_check "fork help" "$(printf '%q fork --help' "$CODEX_CMD")"
run_check "mcp help" "$(printf '%q mcp --help' "$CODEX_CMD")"
run_check "sandbox help" "$(printf '%q sandbox --help' "$CODEX_CMD")"
run_check "app-server help" "$(printf '%q app-server --help' "$CODEX_CMD")"
run_check "completion bash" "$(printf '%q completion bash' "$CODEX_CMD")"
run_check "login status" "$(printf '%q login status' "$CODEX_CMD")"
run_check "mcp list" "$(printf '%q mcp list' "$CODEX_CMD")"
run_check "features list" "$(printf '%q features list' "$CODEX_CMD")"
run_check "schema generate help" "$(printf '%q app-server generate-json-schema --help' "$CODEX_CMD")"
run_check "schema generate out" "$(printf '%q app-server generate-json-schema --out %q' "$CODEX_CMD" "$SCHEMA_DIR")"

if command -v node >/dev/null 2>&1; then
  run_check "node wrapper help" "$(printf 'node %q --help' "$NODE_ENTRY")"
  run_check "node wrapper exec help" "$(printf 'node %q exec --help' "$NODE_ENTRY")"
  run_check "node wrapper review help" "$(printf 'node %q review --help' "$NODE_ENTRY")"
  run_check "node wrapper resume help" "$(printf 'node %q resume --help' "$NODE_ENTRY")"
else
  skip_check "node wrapper checks" "node not found"
fi

if [[ -n "$CODEX_EXEC_CMD" ]]; then
  run_check "codex-exec version" "$(printf '%q --version' "$CODEX_EXEC_CMD")"
else
  skip_check "codex-exec version" "codex-exec not found in PATH or package vendor tree"
fi

if [[ -n "$INSTALLED_BINARY" ]]; then
  case "$(uname -s)" in
    Darwin)
      if command -v otool >/dev/null 2>&1; then
        run_check "installed binary linkage" "$(printf 'otool -L %q' "$INSTALLED_BINARY")"
      else
        skip_check "installed binary linkage" "otool not available"
      fi
      ;;
    *)
      if command -v readelf >/dev/null 2>&1; then
        run_check "installed binary linkage" "$(printf 'readelf -d %q' "$INSTALLED_BINARY")"
      else
        skip_check "installed binary linkage" "readelf not available"
      fi
      ;;
  esac
else
  skip_check "installed binary linkage" "installed codex binary not found under vendor tree"
fi

if [[ "$LIVE_CHECKS" -eq 1 ]]; then
  run_check \
    "live exec ephemeral" \
    "$(printf 'cd %q && %q exec --skip-git-repo-check --ephemeral %q' "$WORKSPACE_DIR" "$CODEX_CMD" 'Reply with exactly: OK')"

  if [[ -n "$CODEX_EXEC_CMD" ]]; then
    run_check \
      "workspace list-files smoke" \
      "$(printf 'cd %q && %q --sandbox workspace-write --skip-git-repo-check --json %q' "$WORKSPACE_DIR" "$CODEX_EXEC_CMD" 'print current directory and list files')"
    run_check \
      "workspace write-read smoke" \
      "$(printf 'cd %q && %q --sandbox workspace-write --skip-git-repo-check --json %q' "$WORKSPACE_DIR" "$CODEX_EXEC_CMD" \"create hello.txt with content 'hello' and then read it back\")"
    run_check \
      "network smoke" \
      "$(printf 'cd %q && %q --sandbox workspace-write --skip-git-repo-check --json %q' "$WORKSPACE_DIR" "$CODEX_EXEC_CMD" 'run one network check with curl -I https://www.google.com and report the first HTTP status line only')"
  else
    skip_check "live codex-exec smokes" "codex-exec not available for live checks"
  fi
else
  skip_check "live exec smokes" "run with --live to enable real exec/workspace/network checks"
fi

OVERALL_STATUS="PASS"
if [[ "$FAIL_COUNT" -gt 0 ]]; then
  OVERALL_STATUS="FAIL"
fi

{
  echo "# CODEX VL RUNTIME REPORT"
  echo
  echo "- Date: $(date '+%Y-%m-%d %H:%M %Z')"
  echo "- Platform: \`$CURRENT_PLATFORM\`"
  echo "- Installed command under test: \`$CODEX_CMD\`"
  echo "- Resolved command path: \`$CODEX_CMD_REAL\`"
  echo "- Node entry: \`$NODE_ENTRY\`"
  echo "- Package root: \`$PACKAGE_ROOT\`"
  if [[ -n "$CODEX_EXEC_CMD" ]]; then
    echo "- codex-exec path: \`$CODEX_EXEC_CMD\`"
  fi
  if [[ -n "$INSTALLED_BINARY" ]]; then
    echo "- Installed binary path: \`$INSTALLED_BINARY\`"
  fi
  echo "- Live checks: \`$([[ "$LIVE_CHECKS" -eq 1 ]] && echo enabled || echo disabled)\`"
  echo "- Report dir: \`$OUT_DIR\`"
  echo
  echo "## Status"
  echo
  echo "- Overall: \`$OVERALL_STATUS\`"
  echo "- Pass: \`$PASS_COUNT\`"
  echo "- Fail: \`$FAIL_COUNT\`"
  echo "- Skip: \`$SKIP_COUNT\`"
  echo
  echo "## Checks"
  echo
  printf '%s\n' "${CHECK_LINES[@]}"
  echo
  echo "## Artifacts"
  echo
  echo "- TSV results: \`$RESULTS_TSV\`"
  echo "- Logs dir: \`$LOG_DIR\`"
  echo "- Schema dir: \`$SCHEMA_DIR\`"
} >"$REPORT_MD"

echo
echo "Runtime suite complete"
echo "Overall: $OVERALL_STATUS"
echo "Report: $REPORT_MD"
echo "TSV:    $RESULTS_TSV"

if [[ "$FAIL_COUNT" -gt 0 ]]; then
  exit 1
fi
