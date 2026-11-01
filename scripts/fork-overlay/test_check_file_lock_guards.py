#!/usr/bin/env python3
"""Tests for ``check_file_lock_guards.py``: the gate itself must be proven.

Negative proofs below are the audit counterexamples: an unguarded lock at file
level and an unguarded lock inside a plain ``mod`` (no ``cfg(test)``) must both
be reported, i.e. they must make the gate fail. The positive proof is that only
test code is excluded from the scan.
"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_file_lock_guards as gate  # noqa: E402


PROD_FILE_LOCK = """\
use std::fs::OpenOptions;

fn prepare() {
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open("/tmp/issue.lock")
        .unwrap();
    lock_file.try_lock().unwrap();
}
"""

PROD_MOD_LOCK = """\
use std::fs::OpenOptions;

pub mod production {
    fn prepare() {
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open("/tmp/issue.lock")
            .unwrap();
        lock_file.try_lock().unwrap();
    }
}
"""

TEST_MOD_LOCK = """\
use std::fs::OpenOptions;

#[cfg(test)]
mod tests {
    fn prepare() {
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/tmp/issue.lock")
            .unwrap();
        lock_file.try_lock().unwrap();
    }
}
"""


class StripCfgTestBlocks(unittest.TestCase):
    def test_top_level_production_code_is_preserved(self):
        self.assertIn("lock_file.try_lock()", gate.strip_cfg_test_blocks(PROD_FILE_LOCK))

    def test_plain_mod_body_is_preserved(self):
        stripped = gate.strip_cfg_test_blocks(PROD_MOD_LOCK)
        self.assertIn("mod production", stripped)
        self.assertIn("lock_file.try_lock()", stripped)

    def test_cfg_test_mod_body_is_excluded(self):
        stripped = gate.strip_cfg_test_blocks(TEST_MOD_LOCK)
        self.assertNotIn("lock_file.try_lock()", stripped)
        self.assertNotIn("mod tests", stripped)


class ScanTree(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def _write(self, rel: str, content: str) -> None:
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    def test_unguarded_lock_at_file_level_fails_the_gate(self):
        self._write("src/issue.rs", PROD_FILE_LOCK)
        violations = gate.scan_tree(self.root)
        self.assertTrue(
            any("src/issue.rs:" in v and "lock_file.try_lock()" in v for v in violations),
            f"expected the file-level unguarded lock to fail the gate, got {violations}",
        )
        self.assertEqual(gate.report(list(violations)), 1)

    def test_unguarded_lock_in_production_module_fails_the_gate(self):
        self._write("src/issue.rs", PROD_MOD_LOCK)
        violations = gate.scan_tree(self.root)
        self.assertTrue(
            any("src/issue.rs:" in v and "lock_file.try_lock()" in v for v in violations),
            f"expected the production-module unguarded lock to fail the gate, got {violations}",
        )

    def test_cfg_test_module_is_not_scanned(self):
        self._write("src/fine.rs", TEST_MOD_LOCK)
        self.assertEqual(gate.scan_tree(self.root), [])
        self.assertEqual(gate.report([]), 0)

    def test_test_trees_and_test_files_are_not_scanned(self):
        self._write("tests/suite/issue.rs", PROD_FILE_LOCK)
        self._write("src/issue_tests.rs", PROD_FILE_LOCK)
        self.assertEqual(gate.scan_tree(self.root), [])


if __name__ == "__main__":
    unittest.main()
