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

# The aggregator step is judged by matching each line against the closed set of
# forms it has today, never by running it: the guard runs on our machines after
# every upstream merge and must not execute anything that comes from a workflow.
_JOB = r"[A-Za-z0-9_-]+"
_COND = r"\"\$\{NEEDS_CHANGED_OUTPUTS_[A-Z_]+\}\" (?:==|!=) 'true'"
LINE_TEMPLATES = [
    re.compile(r"#.*"),
    re.compile(r"exit 0"),
    re.compile(r"fi"),
    re.compile(rf"if \[\[ {_COND}(?: (?:&&|\|\|) {_COND})* \]\]; then"),
    re.compile(r"echo '[A-Za-z0-9 ._>-]+'"),
    re.compile(rf"echo \"[A-Za-z0-9 :_.-]+\$\{{\{{ needs\.{_JOB}\.result \}}\}}\""),
]
STRICT_ASSERT = re.compile(
    rf"\[\[ '\$\{{\{{ needs\.(?P<job>{_JOB})\.result \}}\}}' == 'success' \]\]"
    r" \|\| \{ echo '(?P=job) failed'; exit 1; \}"
)
SKIP_ASSERT = re.compile(
    rf"\[\[ '\$\{{\{{ needs\.(?P<job>{_JOB})\.result \}}\}}' == 'success'"
    r" \|\| '\$\{\{ needs\.(?P=job)\.result \}\}' == 'skipped' \]\]"
    r" \|\| \{ echo '(?P=job) failed'; exit 1; \}"
)


def check_aggregator_step(name, agg, step, off_jobs):
    script = str(step.get("run", ""))
    if not any(f"needs.{j}.result" in script for j in off_jobs):
        return []
    errs = []
    for raw in script.splitlines():
        line = re.sub(r"\s+", " ", raw.strip())
        if not line:
            continue
        strict = STRICT_ASSERT.fullmatch(line)
        skip = SKIP_ASSERT.fullmatch(line)
        if strict:
            if strict["job"] in off_jobs:
                errs.append(
                    f"{name}: job '{agg}' requires 'success' from the disabled job '{strict['job']}'"
                )
        elif skip:
            if skip["job"] not in off_jobs:
                errs.append(
                    f"{name}: job '{agg}' accepts 'skipped' from '{skip['job']}', which is not disabled"
                )
        elif not any(tpl.fullmatch(line) for tpl in LINE_TEMPLATES):
            errs.append(
                f"{name}: job '{agg}' has a line outside the known forms: {line[:90]!r}"
            )
    return errs


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
    # F: every line of an aggregator step that names a disabled job must be one
    # of the known forms, and a disabled job may only be asserted with the form
    # that accepts `skipped`. Nothing is executed.
    for name, jobs in OFF_JOBS.items():
        y = yaml.safe_load(load(name))
        for agg, spec in y["jobs"].items():
            if agg in jobs:
                continue  # a disabled job never runs, so its own steps are not judged
            for step in spec.get("steps") or []:
                errs.extend(check_aggregator_step(name, agg, step, jobs))
    for e in errs:
        print("FAIL:", e)
    print(
        "fork CI overlay: " + ("INTACT" if not errs else f"{len(errs)} markers missing")
    )
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "."))
