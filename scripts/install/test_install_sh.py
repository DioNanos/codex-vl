#!/usr/bin/env python3
"""Offline regression suite for scripts/install/install.sh.

Runs the real installer against fully isolated HOME/CODEX_HOME trees with
locally built release assets (``--from-dir``) and fixture shell payloads, so
no network access is ever required. The cases reproduce the counterexamples
from the 0.166.0-vl.1 final audit:

* command ownership: only the exact link managed by the installer is its own,
  and a foreign command at the destination is refused before any write;
* conflict resolution happens before the first mutation (a declined or
  impossible consent leaves the system untouched);
* the payload is verified in staging (manifest, target, version, per-target
  extras) before promotion; a rejection preserves the previous destination
  and selection;
* a wrong checksum or wrong version is refused;
* the embedded (release-pinned) variant refuses a different version before
  any download;
* uninstall runs under the install lock and never deletes foreign commands;
* Android (Termux) detection selects the Android target.

Usage: python3 scripts/install/test_install_sh.py [-v]
"""

from __future__ import annotations

import fcntl
import hashlib
import io
import json
import os
import platform
import subprocess
import tarfile
import tempfile
import time
import unittest
from pathlib import Path

INSTALLER = Path(__file__).resolve().parent / "install.sh"
VERSION = "0.166.0-vl.1"
OLD_VERSION = "0.166.0-vl.0"
PACKAGE_PREFIX = "codex-vl-package-"
CHECKSUM_ASSET = "SHA256SUMS"

BASE_ENV_KEYS = ("PATH", "LANG", "LC_ALL", "TERM", "TMPDIR", "HOME")


def host_target() -> str:
    machine = platform.machine().lower()
    arch = "x86_64" if machine in ("x86_64", "amd64") else "aarch64"
    return f"{arch}-unknown-linux-musl"


def fake_binary(label: str) -> bytes:
    script = f"""#!/bin/sh
case "$1" in
  --version) printf 'codex-vl {label}\\n' ;;
  *) printf 'fake {label} payload\\n' ;;
esac
exit 0
"""
    return script.encode()


def helper_binary() -> bytes:
    return b"#!/bin/sh\nexit 0\n"


class InstallerSuite(unittest.TestCase):
    """Each test builds its own assets and runs the real install.sh."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="codexvl-install-test.")
        root = Path(self._tmp.name)
        self.home = root / "home"
        self.codex_home = root / "codex-home"
        self.bin_dir = root / "bin"
        self.foreign_dir = root / "npm-bin"
        self.assets = root / "assets"
        for directory in (self.home, self.codex_home, self.bin_dir, self.foreign_dir, self.assets):
            directory.mkdir()
        self.npm_target = self.foreign_dir / "codex-vl-from-npm"
        self.npm_target.write_bytes(fake_binary("npm"))
        self.npm_target.chmod(0o755)
        self.standalone_root = self.codex_home / "packages" / "standalone-vl"
        self.build_assets()

    def tearDown(self) -> None:
        self._tmp.cleanup()

    # ---- fixture helpers -------------------------------------------------

    def build_assets(
        self,
        *,
        target: str | None = None,
        version: str = VERSION,
        binary_version: str | None = None,
        manifest_version: str | None = None,
        manifest_target: str | None = None,
        with_rg: bool = True,
        checksum: str | None = None,
    ) -> Path:
        """Build a local release directory with archive + SHA256SUMS."""
        target = target or host_target()
        binary_version = version if binary_version is None else binary_version
        manifest_version = version if manifest_version is None else manifest_version
        manifest_target = target if manifest_target is None else manifest_target

        members: dict[str, bytes] = {
            "bin/codex": fake_binary(binary_version),
            "bin/codex-code-mode-host": helper_binary(),
            "codex-package.json": json.dumps(
                {
                    "layoutVersion": 1,
                    "version": manifest_version,
                    "target": manifest_target,
                    "variant": "standalone",
                    "entrypoint": "bin/codex",
                    "execServer": "bin/codex-exec-server",
                    "resourcesDir": "codex-resources",
                    "pathDir": "codex-path",
                }
            ).encode(),
        }
        if target == "aarch64-linux-android":
            members["bin/libc++_shared.so"] = b"fake shared libc++"
            if with_rg:
                members["codex-path/rg"] = helper_binary()
        else:
            if with_rg:
                members["codex-path/rg"] = helper_binary()
            members["codex-resources/bwrap"] = helper_binary()

        asset = f"{PACKAGE_PREFIX}{target}.tar.gz"
        archive_path = self.assets / asset
        with tarfile.open(archive_path, "w:gz") as archive:
            for name, data in members.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mode = 0o755
                archive.addfile(info, io.BytesIO(data))

        digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
        if checksum is not None:
            digest = checksum
        (self.assets / CHECKSUM_ASSET).write_text(f"{digest}  {asset}\n")
        return archive_path

    def base_env(self) -> dict[str, str]:
        env = {key: os.environ[key] for key in BASE_ENV_KEYS if key in os.environ}
        env["HOME"] = str(self.home)
        env["CODEX_HOME"] = str(self.codex_home)
        env["CODEX_NON_INTERACTIVE"] = "1"
        # A fixed minimal PATH keeps the host user's own commands (for
        # example a real ~/.local/bin/codex-vl) out of the conflict scan.
        env["PATH"] = "/usr/bin:/bin"
        return env

    def run_installer(
        self,
        *args: str,
        env: dict[str, str] | None = None,
        path_prefix: str = "",
        extra_path: str = "",
        timeout: float = 60.0,
    ) -> subprocess.CompletedProcess:
        run_env = dict(env or self.base_env())
        if path_prefix or extra_path:
            parts = [p for p in (path_prefix, run_env["PATH"], extra_path) if p]
            run_env["PATH"] = ":".join(parts)
        # start_new_session detaches from any controlling terminal so
        # prompt_yes_no can never read a real /dev/tty from a dev machine.
        return subprocess.run(
            ["sh", str(INSTALLER), *args],
            env=run_env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=timeout,
            start_new_session=True,
        )

    def install_ok(self, release: str = VERSION, **kwargs) -> subprocess.CompletedProcess:
        result = self.run_installer(
            "--from-dir",
            str(self.assets),
            "--release",
            release,
            "--bin-dir",
            str(self.bin_dir),
            **kwargs,
        )
        self.assertEqual(result.returncode, 0, f"installer failed:\n{result.stderr}")
        return result

    def command_link(self) -> Path:
        return self.bin_dir / "codex-vl"

    def releases_dir(self) -> Path:
        return self.standalone_root / "releases"

    def release_dir(self, version: str = VERSION, target: str | None = None) -> Path:
        return self.releases_dir() / f"{version}-{target or host_target()}"

    def link_version(self) -> str:
        output = subprocess.run(
            [str(self.command_link()), "--version"],
            capture_output=True,
            text=True,
            env=self.base_env(),
        )
        return output.stdout.strip()

    # ---- ownership of the visible command --------------------------------

    def test_install_creates_owned_link_and_release(self) -> None:
        self.install_ok()

        self.assertTrue(self.command_link().is_symlink())
        self.assertEqual(
            os.readlink(self.command_link()),
            str(self.standalone_root / "current" / "bin" / "codex"),
        )
        self.assertTrue(self.release_dir().is_dir())
        self.assertIn(VERSION, self.link_version())

    def test_dangling_owned_link_is_not_a_conflict(self) -> None:
        # A previous install left our exact link but the tree is gone: the
        # link is still ours and a reinstall must not treat it as foreign.
        os.symlink(
            str(self.standalone_root / "current" / "bin" / "codex"),
            self.command_link(),
        )
        result = self.install_ok()
        self.assertNotIn("another codex-vl", result.stderr)
        self.assertIn(VERSION, self.link_version())

    def test_reinstall_over_own_link_succeeds(self) -> None:
        self.install_ok()
        result = self.install_ok()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(VERSION, self.link_version())

    def test_foreign_link_at_destination_refused_without_effects(self) -> None:
        # Audit counterexample: a fake npm link already in the expected
        # BIN_DIR used to be silently replaced with zero warnings.
        os.symlink(str(self.npm_target), self.command_link())
        result = self.run_installer(
            "--from-dir",
            str(self.assets),
            "--release",
            VERSION,
            "--bin-dir",
            str(self.bin_dir),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("left untouched", result.stderr)
        # The conflict is resolved before any write: the foreign link is
        # intact and nothing of the standalone tree exists yet.
        self.assertTrue(self.command_link().is_symlink())
        self.assertEqual(os.readlink(self.command_link()), str(self.npm_target))
        self.assertFalse(self.standalone_root.exists())

    def test_conflict_elsewhere_declined_before_any_write(self) -> None:
        # Audit counterexample: with no tty the declined prompt used to exit
        # 1 only after the new command and current had already been created.
        foreign = self.foreign_dir / "codex-vl"
        foreign.write_bytes(fake_binary("foreign"))
        foreign.chmod(0o755)

        env = self.base_env()
        env.pop("CODEX_NON_INTERACTIVE")
        result = self.run_installer(
            "--from-dir",
            str(self.assets),
            "--release",
            VERSION,
            "--bin-dir",
            str(self.bin_dir),
            env=env,
            extra_path=str(self.foreign_dir),
        )

        self.assertEqual(result.returncode, 1)
        # stderr must still work after the lock is released: the abort
        # message is visible, not swallowed by a lingering redirect.
        self.assertIn("aborted", result.stderr)
        self.assertIn("left untouched", result.stderr)
        # No write happened at all.
        self.assertFalse(self.standalone_root.exists())
        self.assertFalse(self.command_link().exists())

    def test_conflict_elsewhere_non_interactive_only_warns(self) -> None:
        foreign = self.foreign_dir / "codex-vl"
        foreign.write_bytes(fake_binary("foreign"))
        foreign.chmod(0o755)

        result = self.install_ok(extra_path=str(self.foreign_dir))

        self.assertEqual(result.returncode, 0)
        self.assertIn("another codex-vl", result.stderr)
        # The foreign command elsewhere in PATH is never touched.
        self.assertTrue(foreign.is_file())
        self.assertIn(VERSION, self.link_version())

    # ---- staging verification before promotion ---------------------------

    def test_wrong_payload_version_rejected_in_staging(self) -> None:
        # Audit counterexample: checksum fine but bin/codex --version = 9.9.9
        # used to leave the rejected payload in the final release directory.
        self.build_assets(binary_version="9.9.9")
        result = self.run_installer(
            "--from-dir",
            str(self.assets),
            "--release",
            VERSION,
            "--bin-dir",
            str(self.bin_dir),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("9.9.9", result.stderr)
        self.assertFalse(self.release_dir().exists())
        staging = list(self.releases_dir().glob(".staging.*")) if self.releases_dir().is_dir() else []
        self.assertEqual(staging, [])

    def test_rejection_preserves_previous_installation(self) -> None:
        self.build_assets(version=OLD_VERSION)
        self.install_ok(OLD_VERSION)
        previous_link = os.readlink(self.command_link())

        self.build_assets(binary_version="9.9.9")
        result = self.run_installer(
            "--from-dir",
            str(self.assets),
            "--release",
            VERSION,
            "--bin-dir",
            str(self.bin_dir),
        )

        self.assertNotEqual(result.returncode, 0)
        # The previous destination and the selection are untouched.
        self.assertTrue(self.release_dir(OLD_VERSION).is_dir())
        self.assertTrue((self.release_dir(OLD_VERSION) / "bin" / "codex").is_file())
        self.assertEqual(os.readlink(self.command_link()), previous_link)
        self.assertFalse(self.release_dir(VERSION).exists())
        self.assertIn(OLD_VERSION, self.link_version())

    def test_wrong_checksum_rejected(self) -> None:
        self.build_assets(checksum="0" * 64)
        result = self.run_installer(
            "--from-dir",
            str(self.assets),
            "--release",
            VERSION,
            "--bin-dir",
            str(self.bin_dir),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum mismatch", result.stderr)
        self.assertFalse(self.release_dir().exists())

    def test_manifest_target_mismatch_rejected(self) -> None:
        self.build_assets(manifest_target="aarch64-unknown-linux-musl")
        result = self.run_installer(
            "--from-dir",
            str(self.assets),
            "--release",
            VERSION,
            "--bin-dir",
            str(self.bin_dir),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("manifest target", result.stderr)
        self.assertFalse(self.release_dir().exists())

    def test_missing_linux_helper_makes_release_incomplete(self) -> None:
        # Audit: the completeness predicate ignored per-target extras, so an
        # existing release with a missing helper looked reusable.
        self.build_assets(version=OLD_VERSION)
        self.install_ok(OLD_VERSION)
        rg = self.release_dir(OLD_VERSION) / "codex-path" / "rg"
        rg.unlink()

        self.build_assets(version=OLD_VERSION)
        self.install_ok(OLD_VERSION)

        self.assertTrue(rg.is_file(), "incomplete release must be reinstalled")

    # ---- embedded (release-pinned) variant --------------------------------

    def test_embedded_variant_refuses_other_version_before_download(self) -> None:
        source = INSTALLER.read_text()
        begin = "# --- BEGIN embedded release checksums"
        end = "# --- END embedded release checksums"
        head, rest = source.split(begin, 1)
        _, tail = rest.split(end, 1)
        block = f'''
EMBEDDED_RELEASE_VERSION="{VERSION}"
embedded_package_digest() {{
  case "$1" in
    x86_64-unknown-linux-musl | aarch64-unknown-linux-musl) printf '%s\\n' "{"a" * 64}' ;;
  esac
  return 1
}}
'''
        pinned_path = self.assets / "install-pinned.sh"
        pinned_path.write_text(head + begin + block + end + tail)
        pinned_path.chmod(0o755)

        env = self.base_env()
        env["CODEX_RELEASE"] = "9.9.9"
        result = subprocess.run(
            ["sh", str(pinned_path)],
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=60,
            start_new_session=True,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("pinned", result.stderr)
        # The refusal happens before any download: nothing was installed.
        self.assertFalse(self.standalone_root.exists())

    # ---- uninstall --------------------------------------------------------

    def test_uninstall_removes_owned_tree_and_link(self) -> None:
        self.install_ok()
        result = self.run_installer("--uninstall", "--bin-dir", str(self.bin_dir))

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.standalone_root.exists())
        self.assertFalse(self.command_link().exists())
        # User data under CODEX_HOME is preserved.
        self.assertTrue(self.codex_home.is_dir())

    def test_uninstall_preserves_foreign_command_at_destination(self) -> None:
        # Audit counterexample: with no standalone root, --uninstall used to
        # exit 0 and delete the foreign (npm) link anyway.
        os.symlink(str(self.npm_target), self.command_link())
        result = self.run_installer("--uninstall", "--bin-dir", str(self.bin_dir))

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.command_link().is_symlink())
        self.assertEqual(os.readlink(self.command_link()), str(self.npm_target))
        self.assertIn("not managed", result.stderr)

    def test_uninstall_preserves_foreign_link_replacing_ours(self) -> None:
        self.install_ok()
        # The user repointed the visible command at an npm install: only the
        # owned tree goes away, the foreign link stays.
        self.command_link().unlink()
        os.symlink(str(self.npm_target), self.command_link())

        result = self.run_installer("--uninstall", "--bin-dir", str(self.bin_dir))

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.standalone_root.exists())
        self.assertEqual(os.readlink(self.command_link()), str(self.npm_target))

    def test_uninstall_takes_the_install_lock(self) -> None:
        self.install_ok()

        self.standalone_root.mkdir(exist_ok=True)
        lock_file = self.standalone_root / "install.lock"
        with open(lock_file, "w") as handle:
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
            process = subprocess.Popen(
                [
                    "sh",
                    str(INSTALLER),
                    "--uninstall",
                    "--bin-dir",
                    str(self.bin_dir),
                ],
                env=self.base_env(),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
            # While the lock is held the uninstall must not complete.
            self.assertIsNone(process.poll(), "uninstall ran without the install lock")
            fcntl.flock(handle.fileno(), fcntl.LOCK_UN)

        returncode = process.wait(timeout=60)
        self.assertEqual(returncode, 0)
        self.assertFalse(self.standalone_root.exists())

    # ---- Android (Termux) detection ---------------------------------------

    def test_android_detection_selects_android_target(self) -> None:
        fake_bin = self.assets / "fake-uname"
        fake_bin.mkdir()
        (fake_bin / "uname").write_text(
            "#!/bin/sh\n"
            'case "$1" in\n'
            '  -s) printf "Linux\\n" ;;\n'
            '  -o) printf "Android\\n" ;;\n'
            '  -m) printf "aarch64\\n" ;;\n'
            '  *) printf "Linux\\n" ;;\n'
            "esac\n"
        )
        (fake_bin / "uname").chmod(0o755)

        self.build_assets(target="aarch64-linux-android")
        env = self.base_env()
        env["PREFIX"] = "/data/data/com.termux/files/usr"
        env["TERMUX_VERSION"] = "0.118"
        result = self.run_installer(
            "--from-dir",
            str(self.assets),
            "--release",
            VERSION,
            "--bin-dir",
            str(self.bin_dir),
            env=env,
            path_prefix=str(fake_bin),
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        android_release = self.release_dir(target="aarch64-linux-android")
        self.assertTrue(android_release.is_dir())
        self.assertTrue((android_release / "bin" / "libc++_shared.so").is_file())
        self.assertIn("Android", result.stdout)


if __name__ == "__main__":
    unittest.main()
