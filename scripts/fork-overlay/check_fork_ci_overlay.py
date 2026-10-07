#!/usr/bin/env python3
"""Fail if any marker of the fork CI overlay is missing from .github/workflows.

Checks: disabled jobs (A), restore comments (B), no paid or private runner in
the matrix or runs-on of the guarded files (C), the blocking gate (D), the
postmerge permissions (E) and the result aggregators (F).
"""

import json
import re
import sys
from pathlib import Path

import yaml

OFF_JOBS = {
    "bazel.yml": [
        "test",
        "test-windows-shard",
        "test-windows",
        "test-windows-native-main",
        "clippy",
        "verify-release-build",
    ],
    "rust-ci-full.yml": [
        "argument_comment_lint_prebuilt",
        "lint_build",
        "tests_macos_aarch64",
        "tests_linux_x64_remote",
        "tests_linux_arm64",
        "tests_windows_x64",
        "tests_windows_arm64",
    ],
    "rust-ci.yml": ["argument_comment_lint_prebuilt"],
    "sdk.yml": ["sdks"],
}
# Restore-comment sentinels: file -> (regex, minimum count)
SENTINELS = {
    "bazel.yml": (r"[Dd]isabled on this fork", 9),
    "blocking-ci.yml": (r"disabled on this fork|not a merge gate", 2),
    "rust-ci-full.yml": (r"[Dd]isabled on this fork", 9),
    "rust-ci.yml": (r"[Dd]isabled on this fork", 3),
    "rust-release-argument-comment-lint.yml": (r"[Dd]isabled on this fork", 2),
    "sdk.yml": (r"[Dd]isabled on this fork", 1),
    "v8-canary.yml": (r"[Dd]isabled on this fork", 1),
}
# Files where no job may carry a paid or private runner
NO_PRIVATE_RUNNER = [
    "rust-ci.yml",
    "v8-canary.yml",
    "rust-release-argument-comment-lint.yml",
]
FORBIDDEN = re.compile(r"macos-15-xlarge|-runners\b")


def main(root):
    wf = Path(root) / ".github/workflows"
    errs = []
    load = lambda n: (wf / n).read_text()
    for name, jobs in OFF_JOBS.items():
        y = yaml.safe_load(load(name))
        for j in jobs:
            if y["jobs"].get(j, {}).get("if") is not False:
                errs.append(f"{name}: job '{j}' has no `if: false`")
    for name, (rx, n) in SENTINELS.items():
        found = len(re.findall(rx, load(name)))
        if found < n:
            errs.append(f"{name}: restore comments {found} < {n}")
    for name in NO_PRIVATE_RUNNER:
        y = yaml.safe_load(load(name))
        for j, v in y["jobs"].items():
            where = json.dumps(
                {
                    "runs-on": v.get("runs-on"),
                    "matrix": (v.get("strategy") or {}).get("matrix"),
                },
                default=str,
            )
            if FORBIDDEN.search(where):
                state = "disabled" if v.get("if") is False else "active"
                errs.append(
                    f"{name}: job '{j}' ({state}) has a paid macOS runner or a private "
                    "runner group in runs-on or matrix; the leg must stay commented out"
                )
    needs = yaml.safe_load(load("blocking-ci.yml"))["jobs"]
    gate = next(v for v in needs.values() if isinstance(v, dict) and "needs" in v)
    for dead in ("bazel", "sdk"):
        if dead in gate["needs"]:
            errs.append(
                f"blocking-ci.yml: `needs` still contains '{dead}' (disabled on this fork)"
            )
    pm = yaml.safe_load(load("postmerge-ci.yml"))["jobs"]["rust-ci-full"]
    if (pm.get("permissions") or {}).get("actions") != "write":
        errs.append("postmerge-ci.yml: rust-ci-full lacks permissions.actions: write")
    # F: an aggregator assertion on a disabled job must accept `skipped`.
    # Parsed per assertion line, not counted: a strict assertion keeps the
    # aggregator red while every other marker is still in place.
    for name, jobs in OFF_JOBS.items():
        y = yaml.safe_load(load(name))
        for agg, spec in y["jobs"].items():
            for step in spec.get("steps") or []:
                for line in str(step.get("run", "")).splitlines():
                    if not line.strip().startswith("[["):
                        continue
                    for j in jobs:
                        ref = "needs." + j + ".result"
                        if ref not in line:
                            continue
                        ok = re.search(
                            r"\$\{\{\s*"
                            + re.escape(ref)
                            + r"\s*\}\}'\s*==\s*'skipped'",
                            line,
                        )
                        if not ok:
                            errs.append(
                                f"{name}: job '{agg}' asserts '{j}' without accepting 'skipped'"
                            )
    for e in errs:
        print("FAIL:", e)
    print(
        "fork CI overlay: " + ("INTACT" if not errs else f"{len(errs)} markers missing")
    )
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "."))
