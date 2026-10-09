"""write_link_sdk must expose the runtime's DT_NEEDED sonames inside the SDK.

The linker resolves the transitive libraries of the pinned .so by their
DT_NEEDED name, which is the versioned soname, not the development alias:
without a matching symlink in the SDK the gnu helper link fails with
undefined references into the runtime. The names come from readelf, not
from a hand-written list.
"""

import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import stage_voice_runtime as stage


def dt_needed(path: Path) -> list:
    output = subprocess.run(
        ["readelf", "-d", str(path)], check=True, capture_output=True, text=True
    ).stdout
    return re.findall(r"Shared library: \[([^]]+)\]", output)


class SonameSymlinksTest(unittest.TestCase):
    def setUp(self):
        gcc = shutil.which("gcc")
        if gcc is None:
            self.skipTest("gcc not available")
        self.work = Path(tempfile.mkdtemp(prefix="voice sonames "))
        self.addCleanup(shutil.rmtree, self.work, ignore_errors=True)
        lib = self.work / "runtime" / "lib"
        lib.mkdir(parents=True)
        source = self.work / "src"
        source.mkdir()
        (source / "tfoo.c").write_text("int tfoo(void) { return 7; }\n")
        (source / "tbar.c").write_text(
            "extern int tfoo(void);\nint tbar(void) { return tfoo() + 1; }\n"
        )
        for command in (
            [
                gcc, "-shared", "-fPIC", "-Wl,-soname,libvrtfoo.so.2",
                "-o", str(lib / "libvrtfoo.so.2"), str(source / "tfoo.c"),
            ],
            [
                gcc, "-shared", "-fPIC", "-Wl,-soname,libvrtbar.so.1",
                "-o", str(lib / "libvrtbar.so.1"), str(source / "tbar.c"),
                str(lib / "libvrtfoo.so.2"),
            ],
        ):
            subprocess.run(command, check=True, capture_output=True)

    def test_dt_needed_sonames_have_sdk_symlinks(self):
        lib = self.work / "runtime" / "lib"
        # The fixture carries only its own libraries: the real package table
        # is replaced so the self-check walks the fixture set.
        with patch.object(
            stage,
            "PKG_CONFIG_LIBRARIES",
            {"vrtbar-1.0": "vrtbar", "vrtfoo-1.0": "vrtfoo"},
        ):
            sdk = stage.write_link_sdk(lib.parent, self.work)
        checked = []
        for library in sorted(lib.glob("lib*.so*")):
            for needed in dt_needed(library):
                if needed == library.name:
                    continue
                if not (lib / needed).exists():
                    continue  # system library (libc, libm, ...): not shipped
                link = sdk / "lib" / needed
                self.assertTrue(
                    link.is_symlink(), f"SDK is missing DT_NEEDED symlink {needed}"
                )
                self.assertTrue(link.resolve().is_file())
                checked.append(needed)
        self.assertIn("libvrtfoo.so.2", checked)


if __name__ == "__main__":
    unittest.main()
