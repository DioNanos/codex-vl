#!/usr/bin/env python3

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import sys
import tomllib
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REPOSITORY = "DioNanos/codex-termux"
DEFAULT_TARGET = "aarch64-linux-android"
MANIFEST_PATH = ROOT / "third_party" / "v8" / "android-artifacts.toml"


def resolved_v8_crate_version() -> str:
    cargo_lock = tomllib.loads((ROOT / "codex-rs" / "Cargo.lock").read_text())
    versions = sorted(
        {
            package["version"]
            for package in cargo_lock["package"]
            if package["name"] == "v8"
        }
    )
    if len(versions) != 1:
        raise SystemExit(f"expected exactly one resolved v8 version, found: {versions}")
    return versions[0]


def auth_headers_for_base_url(base_url: str) -> dict[str, str]:
    if "github.com" in base_url:
        return {}
    token = os.environ.get("RUSTY_V8_AUTH_TOKEN")
    if not token:
        return {}
    return {"Authorization": f"token {token}"}


def download(url: str, destination: Path, headers: dict[str, str] | None = None) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    request = urllib.request.Request(url, headers=headers or {})
    with urllib.request.urlopen(request) as response, destination.open("wb") as output:
        shutil.copyfileobj(response, output)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Fetch Android rusty_v8 artifacts for codex-vl Cargo builds."
    )
    parser.add_argument(
        "--repository",
        default=DEFAULT_REPOSITORY,
        help=f"GitHub repository that publishes rusty_v8 artifacts (default: {DEFAULT_REPOSITORY})",
    )
    parser.add_argument(
        "--base-url",
        action="append",
        default=[],
        help=(
            "Explicit release base URL to try first. Repeatable. "
            "Example: https://host/owner/repo/releases/download/rusty-v8-v146.4.0"
        ),
    )
    parser.add_argument(
        "--target",
        default=DEFAULT_TARGET,
        help=f"Rust target triple to fetch (default: {DEFAULT_TARGET})",
    )
    parser.add_argument(
        "--release-tag",
        help="Optional release tag. Defaults to rusty-v8-v<resolved_v8_version>.",
    )
    parser.add_argument(
        "--output-dir",
        default=str(ROOT / ".artifacts" / "rusty_v8"),
        help="Directory where the archive and binding will be stored.",
    )
    return parser.parse_args()


def load_manifest() -> dict[str, object]:
    if not MANIFEST_PATH.exists():
        return {}
    return tomllib.loads(MANIFEST_PATH.read_text())


def manifest_entry(version: str, target: str) -> dict[str, Any] | None:
    manifest = load_manifest()
    versions = manifest.get("versions")
    if not isinstance(versions, dict):
        return None
    version_entry = versions.get(version)
    if not isinstance(version_entry, dict):
        return None
    targets = version_entry.get("targets")
    if not isinstance(targets, dict):
        return None
    target_entry = targets.get(target)
    if not isinstance(target_entry, dict):
        return None
    return dict(target_entry)


def github_release_base_url(repository: str, release_tag: str) -> str:
    return f"https://github.com/{repository}/releases/download/{release_tag}"


def manifest_base_urls(
    manifest: dict[str, Any] | None, release_tag: str
) -> list[str]:
    if not manifest:
        return []

    base_urls = manifest.get("base_urls")
    if isinstance(base_urls, list):
        values = [value.strip().rstrip("/") for value in base_urls if isinstance(value, str)]
        return [value for value in values if value]

    repository = manifest.get("repository")
    if isinstance(repository, str):
        return [github_release_base_url(repository, release_tag)]

    return []


def checksum_matches(path: Path, expected: str | None) -> bool:
    return expected is None or sha256(path) == expected


def fetch_from_base_url(
    base_url: str,
    archive_name: str,
    binding_name: str,
    archive_path: Path,
    binding_path: Path,
    expected_archive_sha: str | None,
    expected_binding_sha: str | None,
) -> tuple[bool, str | None]:
    archive_url = f"{base_url}/{archive_name}"
    binding_url = f"{base_url}/{binding_name}"
    headers = auth_headers_for_base_url(base_url)
    try:
        download(archive_url, archive_path, headers=headers)
        download(binding_url, binding_path, headers=headers)
    except urllib.error.HTTPError as exc:
        return False, f"missing asset or tag: {exc.url} ({exc.code})"
    except urllib.error.URLError as exc:
        return False, str(exc)

    archive_ok = checksum_matches(archive_path, expected_archive_sha)
    binding_ok = checksum_matches(binding_path, expected_binding_sha)
    if archive_ok and binding_ok:
        return True, None

    archive_actual = sha256(archive_path)
    binding_actual = sha256(binding_path)
    archive_path.unlink(missing_ok=True)
    binding_path.unlink(missing_ok=True)
    problems: list[str] = []
    if not archive_ok and expected_archive_sha is not None:
        problems.append(
            f"archive checksum mismatch: expected {expected_archive_sha}, got {archive_actual}"
        )
    if not binding_ok and expected_binding_sha is not None:
        problems.append(
            f"binding checksum mismatch: expected {expected_binding_sha}, got {binding_actual}"
        )
    return False, "; ".join(problems)


def main() -> int:
    args = parse_args()
    version = resolved_v8_crate_version()
    manifest = manifest_entry(version, args.target)
    release_tag = args.release_tag or (
        manifest.get("release_tag") if manifest else f"rusty-v8-v{version}"
    )
    output_dir = Path(args.output_dir).resolve()

    archive_name = f"librusty_v8_release_{args.target}.a.gz"
    binding_name = f"src_binding_release_{args.target}.rs"
    archive_path = output_dir / release_tag / archive_name
    binding_path = output_dir / release_tag / binding_name

    expected_archive_sha = (
        manifest.get("archive_sha256")
        if manifest and isinstance(manifest.get("archive_sha256"), str)
        else None
    )
    expected_binding_sha = (
        manifest.get("binding_sha256")
        if manifest and isinstance(manifest.get("binding_sha256"), str)
        else None
    )

    candidate_base_urls: list[str] = []
    candidate_base_urls.extend(base_url.strip().rstrip("/") for base_url in args.base_url)
    candidate_base_urls.extend(manifest_base_urls(manifest, release_tag))
    candidate_base_urls.append(github_release_base_url(args.repository, release_tag))

    seen: set[str] = set()
    ordered_base_urls: list[str] = []
    for base_url in candidate_base_urls:
        if base_url and base_url not in seen:
            seen.add(base_url)
            ordered_base_urls.append(base_url)

    selected_base_url: str | None = None
    failures: list[str] = []
    for base_url in ordered_base_urls:
        ok, reason = fetch_from_base_url(
            base_url=base_url,
            archive_name=archive_name,
            binding_name=binding_name,
            archive_path=archive_path,
            binding_path=binding_path,
            expected_archive_sha=expected_archive_sha,
            expected_binding_sha=expected_binding_sha,
        )
        if ok:
            selected_base_url = base_url
            break
        failures.append(f"{base_url}: {reason}")

    if selected_base_url is None:
        failure_text = "\n".join(failures)
        raise SystemExit(
            "failed to download rusty_v8 Android artifacts from all configured mirrors:\n"
            f"{failure_text}"
        )

    print(f"resolved v8 crate version: {version}")
    print(f"release tag: {release_tag}")
    print(f"base url: {selected_base_url}")
    print(f"archive: {archive_path}")
    print(f"archive sha256: {sha256(archive_path)}")
    print(f"binding: {binding_path}")
    print(f"binding sha256: {sha256(binding_path)}")
    print()
    print(f'export RUSTY_V8_ARCHIVE="{archive_path}"')
    print(f'export RUSTY_V8_SRC_BINDING_PATH="{binding_path}"')
    return 0


if __name__ == "__main__":
    sys.exit(main())
