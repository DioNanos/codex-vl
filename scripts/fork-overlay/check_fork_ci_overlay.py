#!/usr/bin/env python3
"""Fail if any marker of the fork CI overlay is missing from .github/workflows.

Checks: disabled jobs (A), restore comments (B), no paid or private runner in
the matrix or runs-on of the guarded files (C), the blocking gate (D), the
postmerge permissions (E) and the result aggregators (F).
"""

import json
import re
import subprocess
import sys
import tempfile
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

NEEDS_RESULT = re.compile(r"\$\{\{\s*needs\.([A-Za-z0-9_-]+)\.result\s*\}\}")
SAFE_LINE = re.compile(
    r"^(#.*|echo .*|exit [0-9]+|fi|(if )?\[\[ .* \]\](;| then|; then| \|\| .*)?)$"
)


def run_step(script, results, env):
    """Run the step with every needs.<job>.result replaced by results[job]."""
    text = NEEDS_RESULT.sub(lambda m: results.get(m.group(1), "success"), script)
    if "${{" in text:
        return None
    return subprocess.run(
        ["bash", "-c", text],
        env={"PATH": "/usr/bin:/bin", **env},
        cwd=tempfile.gettempdir(),
        capture_output=True,
        text=True,
        timeout=20,
        check=False,
    )


def check_aggregator_step(name, agg, step, off_jobs):
    script = str(step.get("run", ""))
    mentioned = [j for j in off_jobs if f"needs.{j}.result" in script]
    if not mentioned:
        return []
    for line in (ln.strip() for ln in script.splitlines()):
        if line and not SAFE_LINE.match(line):
            return [
                f"{name}: job '{agg}' has a command the guard does not run: {line[:60]!r}; extend the guard"
            ]
    env = {key: "true" for key in (step.get("env") or {})}
    skipped = {j: "skipped" for j in off_jobs}
    errs = []
    ran = run_step(script, skipped, env)
    if ran is None:
        return [f"{name}: job '{agg}' uses an expression the guard cannot resolve"]
    if ran.returncode != 0:
        errs.append(
            f"{name}: job '{agg}' rejects 'skipped' for a disabled job "
            f"(exit {ran.returncode}: {(ran.stdout.strip().splitlines() or [''])[-1][:80]})"
        )
    for job in mentioned:
        if not any(
            f"needs.{job}.result" in ln and ln.strip().startswith("[[")
            for ln in script.splitlines()
        ):
            continue  # only echoed: nothing is asserted about this job
        ran = run_step(script, {**skipped, job: "failure"}, env)
        if ran is not None and ran.returncode == 0:
            errs.append(
                f"{name}: job '{agg}' accepts a failed '{job}'; the assertion is not enforced"
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
    # F: the aggregator step must accept `skipped` for every disabled job and
    # must still fail when such a job fails. The step is run, not pattern
    # matched, in an isolated shell, so an operator slip such as `&&` for `||`
    # cannot hide behind the right words being present.
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
