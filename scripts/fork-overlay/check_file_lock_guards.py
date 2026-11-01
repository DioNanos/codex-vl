#!/usr/bin/env python3
"""Static gate: std ``File`` advisory-lock call sites must degrade on ENOTSUP.

Replaces the fixed eight-file ENOTSUP inventory in ``verify-patches.sh`` with a
workspace-wide scan. On filesystems whose locks are unavailable (the Android
target before Rust 1.99 returns ``ErrorKind::Unsupported`` from every
``File::lock`` call), an unguarded lock does not fail any test: the affected
flow simply stops working on the device. This gate exists so the next upstream
merge introduces no new unguarded site unnoticed.

Method (heuristic, no type checking):
  * collect ``x.lock()`` / ``x.try_lock()`` / shared variants in production
    Rust code (test modules, ``tests/`` trees and ``*_tests.rs`` excluded);
  * treat a receiver as a ``File`` when the same file opens it through
    ``OpenOptions`` / ``File::options`` / ``.open(...)`` shortly before the
    call site, or when a lock-named receiver sits right after such an open;
  * a call site is guarded when ``is_unsupported_file_lock_error`` or
    ``ErrorKind::Unsupported`` appears within a window around it;
  * everything else must be listed in ``ALLOWED_UNGUARDED`` below with a
    reason, or the gate fails.

Limits: receivers typed as ``File`` only via a function parameter or a struct
field are not recognized; review lock-named identifiers by hand at each merge.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CODEX_RS = ROOT / "codex-rs"

CALL_RE = re.compile(
    r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\.\s*(try_lock_shared|try_lock|lock_shared|lock)\s*\("
)
OPEN_RE = re.compile(
    r"OpenOptions|File::options|File::create|File::open|\.open\s*\("
)
LET_RE = re.compile(r"\b(?:let(?:\s+mut)?)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=")
GUARD_RE = re.compile(r"is_unsupported_file_lock_error|ErrorKind::Unsupported")
# Only a cfg(test) attribute makes a block excludable: a plain `mod` is
# production code and its body must stay visible to the scan.
CFG_TEST_RE = re.compile(r"^\s*#\[[^\]]*cfg\(test\)[^\]]*\]\s*mod\s")
# Window (lines around a call site) used to detect an adjacent open or guard.
# Guards often live in a match arm well below the call site.
NEARBY = 15
GUARD_WINDOW = 25

# Reviewed exceptions: (path, receiver, method) -> why it stays unguarded.
ALLOWED_UNGUARDED: dict[tuple[str, str, str], str] = {
    # Rotating credentials and CA lifetimes stay fail-closed by design
    # (fork rule: bypassing them would be a security regression).
    ("rmcp-client/src/oauth/refresh_lock.rs", "file", "try_lock"): (
        "MCP OAuth refresh coordination: fail-closed by design"
    ),
    ("rmcp-client/src/oauth/store_lock.rs", "file", "try_lock"): (
        "MCP OAuth aggregate store: fail-closed by design"
    ),
    ("rmcp-client/src/oauth/store_lock.rs", "file", "try_lock_shared"): (
        "MCP OAuth aggregate store: fail-closed by design"
    ),
    ("network-proxy/src/certs.rs", "lock", "try_lock"): (
        "managed CA artifact lifetime: fail-closed by design"
    ),
    # Pre-existing unguarded sites already shipped in 0.165 (platform audit
    # A.3.4 or first run of this check): not regressions of the 0.166 merge.
    # Extending the degrade pattern to them is a separate, reviewed change.
    ("login/src/oauth_session_storage.rs", "file", "try_lock"): (
        "pre-existing since 0.165; audit A.3.4"
    ),
    ("rollout/src/maintenance.rs", "file", "try_lock"): (
        "pre-existing since 0.165; audit A.3.4"
    ),
    ("rollout/src/writer_lock.rs", "file", "try_lock"): (
        "publication try-lock; audit A.3.4"
    ),
    ("memories/write/src/workspace_lock.rs", "file", "try_lock"): (
        "pre-existing since 0.165; audit A.3.4"
    ),
    ("tui/src/bottom_pane/mention_usage.rs", "lock_file", "try_lock"): (
        "pre-existing since 0.165; audit A.3.4"
    ),
    ("user-verification/src/lifecycle_lock.rs", "file", "try_lock"): (
        "pre-existing since 0.165; audit A.3.4"
    ),
    ("file-system/src/local_logs.rs", "directory", "try_lock_shared"): (
        "pre-existing since 0.165; audit A.3.4"
    ),
    ("app-server/src/request_processors/remote_control_processor.rs", "lock", "try_lock"): (
        "pre-existing since 0.165; found by this check"
    ),
    ("core-plugins/src/marketplace_staging.rs", "lock", "lock"): (
        "pre-existing since 0.165; found by this check"
    ),
    ("core-plugins/src/marketplace_staging.rs", "lock", "try_lock"): (
        "pre-existing since 0.165; found by this check"
    ),
    ("core-plugins/src/startup_sync.rs", "lock_file", "lock"): (
        "pre-existing since 0.165; found by this check"
    ),
    ("state/src/runtime/reclamation.rs", "lock", "try_lock"): (
        "pre-existing since 0.165; found by this check"
    ),
}


def strip_cfg_test_blocks(text: str) -> str:
    """Blank out ``#[cfg(test)] mod``/``fn`` bodies so tests are not scanned.

    The ``cfg(test)`` attribute is required for exclusion: a plain ``mod``
    holds production code and must never be blanked out here.
    """
    out: list[str] = []
    lines = text.splitlines(keepends=True)
    i = 0
    while i < len(lines):
        line = lines[i]
        probe = CFG_TEST_RE.search(line)
        # Only strip when the cfg(test) attribute and mod are on adjacent lines.
        starts_block = probe is not None or (
            "cfg(test)" in line and re.search(r"\b(mod|fn)\s", lines[i + 1] if i + 1 < len(lines) else "")
        )
        if starts_block:
            depth = 0
            seen_brace = False
            while i < len(lines):
                for ch in lines[i]:
                    if ch == "{":
                        depth += 1
                        seen_brace = True
                    elif ch == "}":
                        depth -= 1
                out.append("")
                i += 1
                if seen_brace and depth <= 0:
                    break
                if not seen_brace and i < len(lines) and lines[i].strip().startswith("#["):
                    continue
                if not seen_brace and i >= len(lines):
                    break
            continue
        out.append(line)
        i += 1
    return "".join(out)


def is_production_file(path: Path, root: Path = CODEX_RS) -> bool:
    parts = path.relative_to(root).parts
    if any(part in {"tests", "benches", "examples"} for part in parts[:-1]):
        return False
    stem = path.stem
    return stem != "tests" and not stem.endswith("_tests")


def receiver_is_file(name: str, lines: list[str], idx: int) -> bool:
    """True when `name` looks opened as a File near line `idx`."""
    looks_lockish = "lock" in name.lower() or "file" in name.lower()
    window = range(max(0, idx - NEARBY), idx)
    open_seen = False
    assigned_nearby = False
    for j in window:
        stripped = lines[j].strip()
        if OPEN_RE.search(stripped):
            open_seen = True
        if LET_RE.search(stripped):
            let_name = LET_RE.search(stripped)
            if let_name and let_name.group(1) == name:
                assigned_nearby = True
    if looks_lockish and open_seen:
        return True
    return assigned_nearby and open_seen


def guarded(lines: list[str], idx: int) -> bool:
    for j in range(max(0, idx - GUARD_WINDOW), min(len(lines), idx + GUARD_WINDOW)):
        if GUARD_RE.search(lines[j]):
            return True
    return False


def scan_tree(root: Path) -> list[str]:
    """Return the unguarded std File lock call sites under ``root``."""
    violations: list[str] = []
    for path in sorted(root.rglob("*.rs")):
        if not is_production_file(path, root):
            continue
        text = strip_cfg_test_blocks(path.read_text(encoding="utf-8"))
        lines = text.splitlines()
        rel = str(path.relative_to(root))
        for m in CALL_RE.finditer(text):
            receiver, method = m.group(1), m.group(2)
            idx = text.count("\n", 0, m.start())
            if method == "lock" and re.search(
                rf"\b{re.escape(receiver)}\s*\.\s*lock\s*\([^()]*\)\s*\.\s*await",
                text[m.start(): m.start() + 80],
            ):
                # Async mutex lock, not a std File lock.
                continue
            if not receiver_is_file(receiver, lines, idx):
                continue
            if guarded(lines, idx):
                continue
            key = (rel, receiver, method)
            if key in ALLOWED_UNGUARDED:
                continue
            violations.append(f"{rel}:{idx + 1}: {receiver}.{method}()")
    return violations


def report(violations: list[str]) -> int:
    if violations:
        print("unguarded std File lock call sites:")
        for v in violations:
            print(f"  {v}")
        print(
            "each site must degrade on ErrorKind::Unsupported or be added to "
            "ALLOWED_UNGUARDED in scripts/fork-overlay/check_file_lock_guards.py"
        )
        return 1
    print("file lock guards: ok")
    return 0


def main() -> int:
    return report(scan_tree(CODEX_RS))


if __name__ == "__main__":
    sys.exit(main())
