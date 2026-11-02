#!/usr/bin/env python3
"""Repack frozen codex-vl npm tarballs into standalone installer packages.

Deterministic release/packaging step. Run it AFTER the five npm tarballs are
frozen and BEFORE the pre-publish audit:

- for every published binary target, extract ``package/vendor/<target>/`` from
  the frozen platform tarball and repack it as
  ``codex-vl-package-<target>.tar.gz`` (archive root is the vendor directory
  itself: ``codex-package.json``, ``bin/``, ``codex-path/``,
  ``codex-resources/``);
- verify every repacked member is byte-identical to the frozen npm tarball
  member (the packaged binaries are the already-audited npm ones: no rebuild,
  no second build);
- verify the vendor manifest version/target and the expected per-target
  payload;
- append the repacked archives (and the pinned installer) to the release
  ``SHA256SUMS`` manifest, so one manifest covers every asset;
- emit ``install.sh`` — a release-pinned variant of
  ``scripts/install/install.sh`` with the archive digests embedded — ready to
  be attached to the GitHub release.

The darwin-arm64 npm package is a source build (no bundled binaries), so it is
intentionally not repacked; the installer tells macOS users to use npm.

Archives are byte-for-byte reproducible: members are sorted by path, timestamps
are zeroed, ownership is dropped, and gzip is written with a fixed mtime.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import os
import stat
import sys
import tarfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
DEFAULT_DIST = REPO_ROOT / "dist"
INSTALL_SH = REPO_ROOT / "scripts" / "install" / "install.sh"

SUMS_NAME = "SHA256SUMS"
INSTALLER_ASSET_NAME = "install.sh"

# Binary targets we publish prebuilt packages for. The darwin npm package is
# source-only and is deliberately absent (see module docstring).
# (rust target triple, npm platform suffix)
TARGETS: tuple[tuple[str, str], ...] = (
    ("x86_64-unknown-linux-musl", "linux-x64"),
    ("aarch64-unknown-linux-musl", "linux-arm64"),
    ("aarch64-linux-android", "android-arm64"),
)

# Payload every target must carry, plus target-specific extras.
REQUIRED_COMMON = ("codex-package.json", "bin/codex", "bin/codex-code-mode-host")
REQUIRED_PER_TARGET: dict[str, tuple[str, ...]] = {
    "x86_64-unknown-linux-musl": ("codex-path/rg", "codex-resources/bwrap"),
    "aarch64-unknown-linux-musl": ("codex-path/rg", "codex-resources/bwrap"),
    "aarch64-linux-android": ("bin/libc++_shared.so",),
}

# Forbidden substrings in repacked text payloads and in the emitted installer.
# The packaged binaries are verified byte-identical to the frozen npm tarballs
# (which the pre-publish audit scans separately), so only text members are
# scanned here.
#
# The needles are assembled from fragments at import time so that this file,
# which ships in the public tree, never carries the assembled internal strings
# as literals (same principle as codex-rs/fork-gate/tests/sanitization_gate.rs
# and tui/tests/fork_identity_pins.rs).
def _joined(parts: tuple[str, ...]) -> str:
    return "".join(parts)


LEAK_NEEDLES: tuple[str, ...] = tuple(
    _joined(parts)
    for parts in (
        ("Docs", "Hub"),
        ("Nexus", "Files"),
        ("/home/", "dag"),
        ("git@", "forge"),
        ("alpaca", "libre"),
        ("Co-", "Authored-", "By"),
        ("Generated", " with"),
    )
)


class RepackError(Exception):
    """Fatal, reportable repack failure."""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def is_text_member(name: str) -> bool:
    return name.endswith((".json", ".md", ".txt", ".toml", ".js", ".py"))


def leak_scan(label: str, data: bytes) -> None:
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError:
        return
    for needle in LEAK_NEEDLES:
        if needle in text:
            raise RepackError(f"{label}: forbidden substring {needle!r} in repacked payload")


def npm_tarball_name(suffix: str, version: str) -> str:
    return f"codex-npm-{suffix}-{version}.tgz"


def package_name(target: str, version: str) -> str:
    return f"codex-vl-package-{target}.tar.gz"


def read_vendor_members(tar_path: Path, target: str) -> dict[str, tuple[tarfile.TarInfo, bytes]]:
    """Return {relative_path: (info, data)} for package/vendor/<target>/**."""
    prefix = f"package/vendor/{target}/"
    members: dict[str, tuple[tarfile.TarInfo, bytes]] = {}
    with tarfile.open(tar_path, "r:gz") as tar:
        for info in tar:
            if not info.name.startswith(prefix) or info.isdir():
                continue
            rel = info.name[len(prefix) :]
            if not rel or rel.startswith(".."):
                raise RepackError(f"{tar_path.name}: unexpected member {info.name!r}")
            handle = tar.extractfile(info)
            if handle is None:
                # Symlinks and special members carry no data stream.
                members[rel] = (info, b"")
                continue
            members[rel] = (info, handle.read())
    if not members:
        raise RepackError(f"{tar_path.name}: no members under {prefix}")
    return members


def check_manifest(members: dict[str, tuple[tarfile.TarInfo, bytes]], target: str, version: str) -> None:
    name = "codex-package.json"
    if name not in members:
        raise RepackError(f"{target}: missing {name} in vendor payload")
    try:
        manifest = json.loads(members[name][1].decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as err:
        raise RepackError(f"{target}: invalid {name}: {err}") from err
    if str(manifest.get("version")) != version:
        raise RepackError(
            f"{target}: {name} version {manifest.get('version')!r} != expected {version!r}"
        )
    if str(manifest.get("target")) != target:
        raise RepackError(
            f"{target}: {name} target {manifest.get('target')!r} != expected {target!r}"
        )


def check_payload(members: dict[str, tuple[tarfile.TarInfo, bytes]], target: str) -> None:
    required = REQUIRED_COMMON + REQUIRED_PER_TARGET[target]
    missing = [name for name in required if name not in members]
    if missing:
        raise RepackError(f"{target}: missing required payload entries: {', '.join(missing)}")
    for name, (info, _) in members.items():
        if info.isfile() and not (info.mode & 0o111) and (
            name.startswith("bin/") or name.startswith("codex-path/")
        ):
            # Binaries and bundled helpers must stay executable through the
            # repack; the installer re-asserts this, but a frozen tarball that
            # lost the bit is a packaging defect worth catching early.
            raise RepackError(f"{target}: {name} is not executable in the frozen tarball")


def normalize_info(info: tarfile.TarInfo) -> tarfile.TarInfo:
    normalized = tarfile.TarInfo(info.name)
    normalized.size = info.size
    normalized.mtime = 0
    normalized.mode = info.mode
    normalized.uid = 0
    normalized.gid = 0
    normalized.uname = ""
    normalized.gname = ""
    normalized.type = info.type
    normalized.linkname = info.linkname
    return normalized


def build_archive(members: dict[str, tuple[tarfile.TarInfo, bytes]]) -> bytes:
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.PAX_FORMAT) as tar:
        for rel in sorted(members):
            info, data = members[rel]
            normalized = normalize_info(info)
            normalized.name = rel
            if info.isfile():
                tar.addfile(normalized, io.BytesIO(data))
            else:
                # Non-regular members (e.g. symlinks) carry no data payload.
                normalized.size = 0
                tar.addfile(normalized)
    out = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=out, mtime=0) as gz:
        gz.write(raw.getvalue())
    return out.getvalue()


def verify_roundtrip(archive: bytes, members: dict[str, tuple[tarfile.TarInfo, bytes]]) -> None:
    """Re-read the produced archive and prove member bytes match the source."""
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
        seen = set()
        for info in tar:
            rel = info.name
            if rel not in members:
                raise RepackError(f"repack produced unexpected member {rel!r}")
            seen.add(rel)
            if info.isfile():
                handle = tar.extractfile(info)
                data = handle.read() if handle is not None else b""
                if sha256_bytes(data) != sha256_bytes(members[rel][1]):
                    raise RepackError(f"repacked member {rel!r} differs from the frozen tarball")
        missing = set(members) - seen
        if missing:
            raise RepackError(f"repack lost members: {', '.join(sorted(missing))}")


EMBED_BEGIN = "# --- BEGIN embedded release checksums (release packaging fills this block) ---"
EMBED_END = "# --- END embedded release checksums ---"


def render_embedded_block(version: str, digests: dict[str, str]) -> str:
    lines = [
        "# --- BEGIN embedded release checksums (release packaging fills this block) ---",
        f'EMBEDDED_RELEASE_VERSION="{version}"',
        "embedded_package_digest() {",
        '  case "$1" in',
    ]
    for target, digest in digests.items():
        lines.append(f"    {target}) printf '%s\\n' '{digest}' ;;")
    lines.extend(
        [
            "    *) return 1 ;;",
            "  esac",
            "}",
            "# --- END embedded release checksums ---",
        ]
    )
    return "\n".join(lines)


def emit_pinned_installer(dist: Path, version: str, digests: dict[str, str]) -> bytes:
    source = INSTALL_SH.read_text(encoding="utf-8")
    begin = source.find(EMBED_BEGIN)
    end = source.find(EMBED_END)
    if begin == -1 or end == -1 or end < begin:
        raise RepackError(f"{INSTALL_SH}: embedded checksum block markers not found")
    end += len(EMBED_END)
    rendered = source[:begin] + render_embedded_block(version, digests) + source[end:]
    data = rendered.encode("utf-8")
    leak_scan(INSTALLER_ASSET_NAME, data)
    return data


def load_sums(dist: Path) -> dict[str, str]:
    sums: dict[str, str] = {}
    path = dist / SUMS_NAME
    if path.exists():
        for line in path.read_text(encoding="utf-8").splitlines():
            parts = line.split()
            if len(parts) == 2 and len(parts[0]) == 64:
                sums[parts[1]] = parts[0].lower()
    return sums


def write_sums(dist: Path, sums: dict[str, str]) -> None:
    lines = [f"{digest}  {name}" for name, digest in sorted(sums.items())]
    (dist / SUMS_NAME).write_text("\n".join(lines) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--dist",
        type=Path,
        default=DEFAULT_DIST,
        help="directory holding the frozen npm tarballs and SHA256SUMS (default: dist/)",
    )
    parser.add_argument(
        "--version",
        help="expected frozen version (default: read from the first platform manifest)",
    )
    parser.add_argument(
        "--skip-install-sh",
        action="store_true",
        help="only repack the package archives; do not emit the pinned installer",
    )
    parser.add_argument(
        "--target",
        action="append",
        choices=[target for target, _ in TARGETS],
        help="repack only the given target triple (repeatable; default: all)",
    )
    args = parser.parse_args(argv)

    targets = tuple(
        (target, suffix)
        for target, suffix in TARGETS
        if args.target is None or target in args.target
    )

    dist = args.dist.resolve()
    if not dist.is_dir():
        print(f"error: {dist} is not a directory", file=sys.stderr)
        return 1

    try:
        version = args.version
        if version is None:
            # Read the version from any frozen platform tarball manifest.
            candidate = sorted(dist.glob("codex-npm-*.tgz"))
            if not candidate:
                raise RepackError(f"no frozen codex-npm-*.tgz tarballs found in {dist}")
            stem = candidate[0].name[: -len(".tgz")]
            version = None
            for _, suffix in TARGETS:
                prefix = f"codex-npm-{suffix}-"
                if stem.startswith(prefix):
                    version = stem[len(prefix) :]
                    break
            if version is None:
                raise RepackError(
                    f"cannot infer the frozen version from {candidate[0].name}; pass --version"
                )

        digests: dict[str, str] = {}
        for target, suffix in targets:
            tar_path = dist / npm_tarball_name(suffix, version)
            if not tar_path.is_file():
                raise RepackError(f"frozen tarball not found: {tar_path.name}")
            frozen_sha = sha256_bytes(tar_path.read_bytes())
            members = read_vendor_members(tar_path, target)
            check_manifest(members, target, version)
            check_payload(members, target)
            for rel, (_, data) in members.items():
                if is_text_member(rel):
                    leak_scan(f"{tar_path.name}:{rel}", data)
            archive = build_archive(members)
            verify_roundtrip(archive, members)
            out_path = dist / package_name(target, version)
            out_path.write_bytes(archive)
            digests[target] = sha256_bytes(archive)
            print(
                f"repacked {out_path.name} ({len(archive)} bytes) "
                f"from {tar_path.name} (sha256 {frozen_sha[:12]}…)"
            )

        sums = load_sums(dist)
        for target, digest in digests.items():
            sums[package_name(target, version)] = digest
        if not args.skip_install_sh:
            installer = emit_pinned_installer(dist, version, digests)
            installer_path = dist / INSTALLER_ASSET_NAME
            installer_path.write_bytes(installer)
            os.chmod(installer_path, 0o755)
            sums[INSTALLER_ASSET_NAME] = sha256_bytes(installer)
            print(f"emitted {installer_path.name} (pinned to {version})")
        write_sums(dist, sums)
        print(f"updated {SUMS_NAME} ({len(sums)} entries)")
    except RepackError as err:
        print(f"error: {err}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
