#!/usr/bin/env python3
"""Stage the private voice runtime into codex-vl vendor payloads.

The realtime voice helper (`/voice`) only starts when the installation package
carries `codex-resources/voice/bin/codex-voice-host` next to the GStreamer
runtime it loads. This tool produces that directory for one app target.

Two inputs are deliberately separated:

* The native GStreamer runtime comes from the pinned upstream package for the
  app target, verified against `voice_runtime_pins.json` before extraction. Its
  native sources are the ones already pinned in `third_party/voice/sources.json`
  (verified byte-for-byte by this script), so the payload cannot drift from the
  sources this repository declares.
* The helper binary is built from *this* repository. The voice control protocol
  handshakes on the build commit (`realtime-webrtc/src/client.rs` sends
  `Hello { protocol, build_commit }`, `voice-host/src/main.rs` rejects anything
  but its own `STABLE_GIT_COMMIT`), so a helper built by the upstream release
  would be refused by our app and vice versa.

Subcommands:
  stage     fetch+verify the pinned payload, extract and validate the runtime,
            write the pkg-config SDK used to link the helper
  build     compile codex-voice-host from this repository against that SDK
  assemble  write codex-resources/voice with manifest.json bound to our commit
  verify    re-check an assembled voice directory
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[1]
VOICE_TOOLS = REPO_ROOT / "third_party" / "voice"
PINS_PATH = SCRIPT_DIR / "voice_runtime_pins.json"
RUNTIME_DIR_NAME = "codex-resources/voice"
SOURCES_PINS = VOICE_TOOLS / "sources.json"
DOWNLOAD_LIMIT = 512 * 1024 * 1024
DOWNLOAD_TIMEOUT = 300

# Every pkg-config package the crates in the helper's dependency graph probe
# through system-deps, with the library each one links. Derived from
# codex-rs/Cargo.lock (`*-sys` crates reachable from codex-voice-host) and from
# the `[package.metadata.system-deps]` tables of those crates: the set is
# enumerated, never grown one failure at a time. The GStreamer families keep a
# library name different from their package name, exactly as in the runtime the
# package ships: -lgstbase-1.0, -lgstapp-1.0, -lgstaudio-1.0.
#
# `alsa` is the only system-static entry. It was absent from this table before
# the V1 packaging because the table originally enumerated the libraries that
# ship inside the pinned GStreamer runtime, and the gnu helper build never
# reached alsa-sys in CI (cpal only compiles on linux-gnu; musl and darwin
# don't). Upstream resolves the same dependency by injecting a Bazel-built
# @alsa_lib into alsa-sys (MODULE.bazel, bazel_dep alsa_lib 1.2.9.bcr.4); here
# alsa-lib 1.2.9 is compiled from pinned sources into the build SDK and linked
# statically, so unlike the families above it is not looked up in the shipped
# runtime (see SYSTEM_STATIC_PACKAGES).
PKG_CONFIG_LIBRARIES: dict[str, str] = {
    "glib-2.0": "glib-2.0",
    "gobject-2.0": "gobject-2.0",
    "gio-2.0": "gio-2.0",
    "gstreamer-1.0": "gstreamer-1.0",
    "gstreamer-base-1.0": "gstbase-1.0",
    "gstreamer-app-1.0": "gstapp-1.0",
    "gstreamer-audio-1.0": "gstaudio-1.0",
    "alsa": "asound",
}

# Entries of PKG_CONFIG_LIBRARIES linked statically from the build SDK instead
# of shipped in the runtime. write_link_sdk skips them when checking that the
# runtime carries every linked library, and cmd_stage verifies their build
# artifacts in the SDK instead.
SYSTEM_STATIC_PACKAGES = ("alsa",)
GLIB_PACKAGES = ("glib-2.0", "gobject-2.0", "gio-2.0")

LICENSE_FILES = (
    "NOTICE.md",
    "sources.json",
    "licenses/LGPL-2.1.txt",
    "licenses/Opus.txt",
    "licenses/PCRE2.md",
    "licenses/libffi.txt",
    "licenses/proxy-libintl.txt",
    "licenses/sljit.txt",
    "licenses/zlib.txt",
)


def digest(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def digest_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_pins() -> dict:
    return json.loads(PINS_PATH.read_text(encoding="utf-8"))


def pinned_asset(app_target: str) -> dict:
    assets = load_pins()["assets"]
    if app_target not in assets:
        raise SystemExit(
            f"no pinned voice runtime for app target {app_target!r}; "
            f"known targets: {', '.join(sorted(assets))}"
        )
    return assets[app_target]


def download_verified(asset: dict, cache_dir: Path) -> Path:
    pins = load_pins()
    url = pins["urlTemplate"].format(asset=asset["asset"])
    cache_dir.mkdir(parents=True, exist_ok=True)
    target = cache_dir / asset["asset"]
    if target.is_file() and digest(target) == asset["sha256"]:
        print(f"using cached {target.name} (sha256 verified)")
        return target
    print(f"downloading {url}")
    request = urllib.request.Request(url, headers={"User-Agent": "codex-vl-packaging"})
    with urllib.request.urlopen(request, timeout=DOWNLOAD_TIMEOUT) as response:
        if response.status != 200:
            raise SystemExit(f"unexpected status {response.status} for {url}")
        payload = response.read(DOWNLOAD_LIMIT + 1)
    if len(payload) > DOWNLOAD_LIMIT:
        raise SystemExit(f"{url} exceeds the {DOWNLOAD_LIMIT} byte limit")
    actual = digest_bytes(payload)
    if actual != asset["sha256"]:
        raise SystemExit(
            f"digest mismatch for {asset['asset']}: expected {asset['sha256']}, got {actual}"
        )
    target.write_bytes(payload)
    print(f"verified {asset['asset']} sha256 {actual}")
    return target


def extract_runtime(archive: Path, work: Path) -> Path:
    runtime = work / "runtime"
    if runtime.exists():
        shutil.rmtree(runtime)
    runtime.mkdir(parents=True)
    prefix = f"{RUNTIME_DIR_NAME}/"
    extracted = 0
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar.getmembers():
            if not member.name.startswith(prefix) or member.isdir():
                continue
            relative = member.name[len(prefix) :]
            if not relative or member.name.endswith("/"):
                continue
            if member.issym() or member.islnk():
                raise SystemExit(f"payload member {member.name} is a link, refusing")
            if not member.isfile():
                continue
            tar.extract(member, path=work / "extract", filter="data")
            destination = runtime / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(work / "extract" / member.name, destination)
            extracted += 1
    if not extracted:
        raise SystemExit(f"{archive.name} carries no {RUNTIME_DIR_NAME} payload")
    shutil.rmtree(work / "extract")
    print(f"extracted {extracted} voice runtime files from {archive.name}")
    return runtime


def validate_runtime(runtime: Path, voice_target: str, work: Path) -> dict:
    sys.path.insert(0, str(VOICE_TOOLS))
    from package_runtime import runtime_files  # noqa: PLC0415

    if digest(SOURCES_PINS) != json.loads(
        (runtime / "runtime.json").read_text(encoding="utf-8")
    ).get("sourceManifestSha256"):
        raise SystemExit(
            "pinned payload was not built from third_party/voice/sources.json; "
            "refusing to ship a runtime whose sources this repository does not pin"
        )
    files = runtime_files(runtime, voice_target, public_release=True)
    for relative in LICENSE_FILES:
        shipped = runtime / relative
        declared = VOICE_TOOLS / relative
        if not shipped.is_file() or not declared.is_file():
            raise SystemExit(f"license input {relative} is missing")
        if digest(shipped) != digest(declared):
            raise SystemExit(
                f"license input {relative} differs between the pinned payload and this repository"
            )
    inventory = sorted(files)
    (work / "runtime-files.json").write_text(
        json.dumps({"target": voice_target, "files": files}, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"runtime validated: {len(inventory)} files for {voice_target}")
    return files


def _library_present(libdir: Path, library: str) -> bool:
    """True when the runtime carries the library under a name the linker accepts."""
    for pattern in (
        f"lib{library}.so*",
        f"lib{library}.dylib",
        f"lib{library}.*.dylib",
    ):
        if any(libdir.glob(pattern)):
            return True
    return False


def source_versions() -> dict:
    pins = json.loads(SOURCES_PINS.read_text(encoding="utf-8"))
    versions = {entry["name"]: entry["version"] for entry in pins["sources"]}
    for required in ("glib", "gstreamer"):
        if required not in versions:
            raise SystemExit(f"{SOURCES_PINS} does not pin {required}")
    return versions


def write_link_sdk(runtime: Path, work: Path) -> Path:
    sdk = work / "sdk"
    if sdk.exists():
        shutil.rmtree(sdk)
    libdir = sdk / "lib"
    pkgconfig = sdk / "pkgconfig"
    libdir.mkdir(parents=True)
    pkgconfig.mkdir(parents=True)

    # The shipped runtime carries versioned sonames only; the linker needs the
    # development name. Symlink, never copy: the digests of the shipped files
    # must stay the ones recorded in runtime.json.
    for library in sorted((runtime / "lib").iterdir()):
        if not library.is_file():
            continue
        name = library.name
        if name.endswith(".dylib"):
            # The packaged dylibs carry their ABI version in the file name
            # (libglib-2.0.0.dylib) while the linker asks for the development
            # name from the pkg-config file (-lglib-2.0 -> libglib-2.0.dylib).
            os.symlink(library.resolve(), libdir / name)
            alias = re.fullmatch(r"(lib[A-Za-z0-9_+.-]+)\.\d+(?:\.\d+)*\.dylib", name)
            if alias:
                os.symlink(library.resolve(), libdir / f"{alias.group(1)}.dylib")
            continue
        if name.endswith(".dll"):
            os.symlink(library.resolve(), libdir / name)
            continue
        match = re.fullmatch(r"(lib[A-Za-z0-9_+.-]+\.so)\.(\d+(?:\.\d+)*)", name)
        if not match:
            continue
        # The pinned .so declares its dependencies with the versioned soname
        # (libintl.so.8, libgsttag-1.0.so.0): the linker resolves those names
        # through -rpath-link, so the SDK must carry them, not just the
        # development aliases the -l flags ask for.
        os.symlink(library.resolve(), libdir / name)
        os.symlink(library.resolve(), libdir / match.group(1))

    versions = source_versions()
    glib_version = versions["glib"]
    gstreamer_version = versions["gstreamer"]
    for package, library in sorted(PKG_CONFIG_LIBRARIES.items()):
        if package in SYSTEM_STATIC_PACKAGES:
            # alsa's .pc is written by build_alsa_static, which only runs for
            # gnu Linux targets: the darwin SDK must carry no dangling alsa.pc.
            continue
        is_glib = package in GLIB_PACKAGES
        requires = () if package == "gstreamer-1.0" else ("gstreamer-1.0",)
        if is_glib:
            requires = ()
        (pkgconfig / f"{package}.pc").write_text(
            "\n".join(
                [
                    "prefix=${pcfiledir}/..",
                    "exec_prefix=${prefix}",
                    "libdir=${prefix}/lib",
                    "",
                    f"Name: {package}",
                    f"Version: {glib_version if is_glib else gstreamer_version}",
                    "Description: pinned voice runtime (link metadata only)",
                    *( [f"Requires: {' '.join(requires)}"] if requires else [] ),
                    "",
                    f"Libs: -L${{libdir}} -l{library}",
                    "Cflags:",
                    "",
                ]
            ),
            encoding="utf-8",
        )
    missing = [
        f"-l{library}"
        for package, library in sorted(PKG_CONFIG_LIBRARIES.items())
        if package not in SYSTEM_STATIC_PACKAGES
        and not _library_present(runtime / "lib", library)
    ]
    if missing:
        raise SystemExit(
            "the pinned runtime does not carry every library the helper links: "
            + ", ".join(missing)
        )
    print(
        f"link SDK written: {sdk} ({len(PKG_CONFIG_LIBRARIES)} pkg-config packages)"
    )
    return sdk


def source_pin() -> dict:
    pins = load_pins()
    source = pins.get("alsaSource")
    if not source:
        raise SystemExit("voice_runtime_pins.json is missing the alsaSource entry")
    return source


def download_source_verified(asset: dict, cache_dir: Path) -> Path:
    """Same verification contract as download_verified, for a plain URL pin."""
    url = asset["url"]
    cache_dir.mkdir(parents=True, exist_ok=True)
    target = cache_dir / url.rsplit("/", 1)[-1]
    if target.is_file() and digest(target) == asset["sha256"]:
        print(f"using cached {target.name} (sha256 verified)")
        return target
    print(f"downloading {url}")
    request = urllib.request.Request(url, headers={"User-Agent": "codex-vl-packaging"})
    with urllib.request.urlopen(request, timeout=DOWNLOAD_TIMEOUT) as response:
        if response.status != 200:
            raise SystemExit(f"unexpected status {response.status} for {url}")
        payload = response.read(DOWNLOAD_LIMIT + 1)
    if len(payload) > DOWNLOAD_LIMIT:
        raise SystemExit(f"{url} exceeds the {DOWNLOAD_LIMIT} byte limit")
    actual = digest_bytes(payload)
    if actual != asset["sha256"]:
        raise SystemExit(
            f"digest mismatch for {target.name}: expected {asset['sha256']}, got {actual}"
        )
    target.write_bytes(payload)
    print(f"verified {target.name} sha256 {actual}")
    return target


def _run_checked(command: list, cwd: Path, environment: dict) -> None:
    result = subprocess.run(command, cwd=cwd, env=environment, capture_output=True, text=True)
    if result.returncode != 0:
        tail = "\n".join((result.stderr or result.stdout).splitlines()[-12:])
        raise SystemExit(f"`{' '.join(command)}` failed in {cwd}:\n{tail}")


# alsa-lib compiles with the toolchain that also links the helper: the x86_64
# gnu helper builds natively on the runner, aarch64 uses the distro cross gcc.
# ALSA_CC overrides the cross compiler (set in the packaging workflow if the
# default cross package changes).
ALSA_CROSS_CC = {"aarch64-unknown-linux-gnu": ("aarch64-linux-gnu-gcc", "aarch64-linux-gnu")}


def build_alsa_static(sdk: Path, voice_target: str, cache_dir: Path) -> None:
    """Compile pinned alsa-lib sources into `sdk` as a static archive.

    Mirrors the upstream Bazel build (cc_library `asound` with the default
    compile-time dirs /usr/share/alsa and /dev/snd), except that autotools
    never writes to the real system directories: the install lands in a
    DESTDIR staging tree and only the static archive plus headers are
    projected into the SDK. The .pc file is written here, like the GStreamer
    ones.
    """
    marker = sdk / "lib" / "libasound.a"
    if marker.is_file():
        print("alsa static library already present in the SDK")
        return
    cache_dir = cache_dir.resolve()
    source = download_source_verified(source_pin(), cache_dir)
    build = cache_dir / "alsa-build"
    if build.exists():
        shutil.rmtree(build)
    build.mkdir(parents=True)
    with tarfile.open(source, "r:bz2") as archive:
        archive.extractall(build, filter="data")
    members = [item for item in build.iterdir() if item.is_dir()]
    if len(members) != 1:
        raise SystemExit(
            "unexpected alsa-lib archive layout: "
            + ", ".join(sorted(item.name for item in build.iterdir()))
        )
    tree = members[0]
    environment = dict(os.environ)
    host = None
    if voice_target in ALSA_CROSS_CC:
        cc, host = ALSA_CROSS_CC[voice_target]
        environment["CC"] = os.environ.get("ALSA_CC", cc)
    command = ["./configure", "--prefix=/usr", "--enable-static", "--disable-shared", "--disable-python"]
    if host:
        command.append(f"--host={host}")
    _run_checked(command, tree, environment)
    _run_checked(["make", "-j2"], tree, environment)
    staging = build / "stage"
    _run_checked(["make", "install", f"DESTDIR={staging}"], tree, environment)
    installed = staging / "usr"
    archive = installed / "lib" / "libasound.a"
    headers = installed / "include" / "alsa"
    for required in (archive, headers):
        if not required.exists():
            raise SystemExit(f"alsa-lib install staging is missing {required}")
    (sdk / "lib").mkdir(parents=True, exist_ok=True)
    shutil.copy2(archive, marker)
    shutil.copytree(headers, sdk / "include" / "alsa", dirs_exist_ok=True)
    pkgconfig = sdk / "pkgconfig"
    pkgconfig.mkdir(parents=True, exist_ok=True)
    (pkgconfig / "alsa.pc").write_text(
        f"prefix={sdk}\n"
        "libdir=${prefix}/lib\n"
        "includedir=${prefix}/include\n"
        "\n"
        "Name: alsa\n"
        "Description: ALSA sound library (static build for the voice helper SDK)\n"
        f"Version: {source_pin()['version']}\n"
        "Libs: -L${libdir} -lasound\n"
        "Cflags: -I${includedir}\n",
        encoding="utf-8",
    )
    print(f"alsa static SDK written: {marker} ({marker.stat().st_size} bytes)")


def cmd_stage(args: argparse.Namespace) -> int:
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    asset = pinned_asset(args.app_target)
    archive = download_verified(asset, args.cache.resolve())
    runtime = extract_runtime(archive, work)
    validate_runtime(runtime, asset["voiceTarget"], work)
    write_link_sdk(runtime, work)
    if asset["voiceTarget"].endswith("-unknown-linux-gnu"):
        # alsa only exists in the gnu helper's dependency graph (cpal); darwin
        # uses coreaudio and musl builds don't ship the gnu helper at all.
        build_alsa_static(work / "sdk", asset["voiceTarget"], args.cache.resolve())
    (work / "voice-target").write_text(asset["voiceTarget"] + "\n", encoding="utf-8")
    return 0


def helper_environment(sdk: Path, voice_target: str, build_commit: str) -> dict:
    environment = dict(os.environ)
    pkgconfig = str(sdk / "pkgconfig")
    environment["PKG_CONFIG_PATH"] = pkgconfig
    environment["PKG_CONFIG_LIBDIR"] = pkgconfig
    environment["STABLE_GIT_COMMIT"] = build_commit
    rpath = "@loader_path/../lib" if voice_target.endswith("-apple-darwin") else "$ORIGIN/../lib"
    # Single token, same shape the packaging workflows use, so a whitespace
    # split of RUSTFLAGS cannot separate the option from its value.
    flags = [f"-Clink-arg=-Wl,-rpath,{rpath}"]
    if voice_target.endswith("-unknown-linux-gnu"):
        # Transitive libraries of the pinned .so are resolved at link time by
        # their versioned soname, and -L does not apply to DT_NEEDED search.
        flags.append(f"-Clink-arg=-Wl,-rpath-link,{sdk}/lib")
    # The packaged tree must not carry build-machine paths: the release
    # workflows reject a tarball that still mentions the CI checkout.
    cargo_home = Path(environment.get("CARGO_HOME") or Path.home() / ".cargo")
    target_dir = Path(environment.get("CARGO_TARGET_DIR") or REPO_ROOT / "codex-rs" / "target")
    for source, replacement in (
        (REPO_ROOT, "/codex-vl"),
        (cargo_home, "/cargo"),
        (target_dir, "/target"),
    ):
        # rustc takes this as a top-level flag, not as a codegen option: the
        # packaging workflows pass it the same way.
        flags.append(f"--remap-path-prefix={source}={replacement}")
    if environment.get("RUSTFLAGS"):
        flags = environment["RUSTFLAGS"].split() + flags
    environment["RUSTFLAGS"] = " ".join(flags)
    return environment


def cmd_build(args: argparse.Namespace) -> int:
    work = args.work.resolve()
    sdk = work / "sdk"
    if not (sdk / "pkgconfig").is_dir():
        raise SystemExit(f"run `stage` first: {sdk} is missing")
    environment = helper_environment(sdk, args.voice_target, args.build_commit)
    command = [
        "cargo",
        "build",
        "--manifest-path",
        str(REPO_ROOT / "codex-rs" / "Cargo.toml"),
        "--package",
        "codex-voice-host",
        "--bin",
        "codex-voice-host",
        "--target",
        args.voice_target,
        "--release",
    ]
    print("+ STABLE_GIT_COMMIT=" + args.build_commit)
    print("+ PKG_CONFIG_LIBDIR=" + environment["PKG_CONFIG_LIBDIR"])
    print("+ RUSTFLAGS=" + environment["RUSTFLAGS"])
    print("+ " + " ".join(command), flush=True)
    subprocess.run(command, check=True, cwd=REPO_ROOT, env=environment)
    built = (
        Path(environment.get("CARGO_TARGET_DIR", str(REPO_ROOT / "codex-rs" / "target")))
        / args.voice_target
        / "release"
        / "codex-voice-host"
    )
    if not built.is_file():
        raise SystemExit(f"helper was not produced: {built}")
    helper_dir = work / "helper"
    helper_dir.mkdir(parents=True, exist_ok=True)
    destination = helper_dir / "codex-voice-host"
    shutil.copy2(built, destination)
    destination.chmod(0o755)
    print(f"helper staged: {destination} ({destination.stat().st_size} bytes)")
    return 0


def cmd_assemble(args: argparse.Namespace) -> int:
    work = args.work.resolve()
    output = args.output.resolve()
    voice = work / "runtime"
    helper = args.helper.resolve()
    if output.exists():
        raise SystemExit(f"output must be fresh: {output} already exists")
    if not helper.is_file():
        raise SystemExit(f"helper not found: {helper}")
    if not args.voice_target.endswith("-unknown-linux-gnu") and not args.voice_target.endswith(
        "-apple-darwin"
    ):
        raise SystemExit(f"unsupported voice target {args.voice_target}")
    files = json.loads((work / "runtime-files.json").read_text(encoding="utf-8"))["files"]
    app_binary = args.app_binary.resolve() if args.app_binary is not None else None
    if app_binary is not None and not app_binary.is_file():
        raise SystemExit(f"app binary not found: {app_binary}")

    output.mkdir(parents=True)
    bin_dir = output / "bin"
    bin_dir.mkdir()
    shutil.copy2(helper, bin_dir / "codex-voice-host")
    (bin_dir / "codex-voice-host").chmod(0o755)
    for relative in files:
        source = voice / relative
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
    for relative in LICENSE_FILES:
        source = voice / relative
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
    if args.voice_target.endswith("-unknown-linux-gnu"):
        # alsa-lib is statically linked on Linux, so it never appears as a
        # shipped file: the notice is the only place the package discloses it.
        source_pin_data = source_pin()
        notice = output / "NOTICE.md"
        notice.write_text(
            notice.read_text(encoding="utf-8")
            + (
                "\n## ALSA library (alsa-lib)\n\n"
                "Linux voice helpers link alsa-lib statically (alsa-lib "
                f"{source_pin_data['version']}, LGPL-2.1; see "
                "licenses/LGPL-2.1.txt). Unlike the native libraries above, "
                "alsa-lib is not shipped as a separate file: its object code "
                "is inside bin/codex-voice-host. The source archive URL and "
                "SHA-256 digest are pinned in the packaging script "
                "(voice_runtime_pins.json). At runtime the helper reads the "
                "system ALSA configuration from /usr/share/alsa and the device "
                "nodes from /dev/snd, when present.\n"
            ),
            encoding="utf-8",
        )

    # The macOS package builds its app binary locally at install time, so the
    # manifest records only what this package actually ships there.
    digests = {"bin/codex": digest(app_binary)} if app_binary is not None else {}
    digests[f"{RUNTIME_DIR_NAME}/bin/codex-voice-host"] = digest(bin_dir / "codex-voice-host")
    for path in sorted(output.rglob("*")):
        if path.is_file():
            digests[f"{RUNTIME_DIR_NAME}/{path.relative_to(output).as_posix()}"] = digest(path)
    manifest = {
        "schemaVersion": 1,
        "buildCommit": args.build_commit,
        "appTarget": args.app_target,
        "voiceTarget": args.voice_target,
        "appVersion": args.release_version,
        "sha256": dict(sorted(digests.items())),
    }
    (output / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    print(f"voice directory assembled: {output} ({len(manifest['sha256'])} digests)")
    return 0


def cmd_verify(args: argparse.Namespace) -> int:
    voice = args.voice_dir.resolve()
    manifest_path = voice / "manifest.json"
    if not manifest_path.is_file():
        raise SystemExit(f"manifest missing: {manifest_path}")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("buildCommit") != args.build_commit:
        raise SystemExit(
            f"manifest buildCommit {manifest.get('buildCommit')} != {args.build_commit}"
        )
    prefix = f"{RUNTIME_DIR_NAME}/"
    checked = 0
    for relative, expected in manifest.get("sha256", {}).items():
        if not relative.startswith(prefix):
            continue
        path = voice / relative[len(prefix) :]
        if not path.is_file():
            raise SystemExit(f"missing voice file {relative}")
        actual = digest(path)
        if actual != expected:
            raise SystemExit(f"digest mismatch for {relative}: {actual} != {expected}")
        checked += 1
    helper = voice / "bin" / "codex-voice-host"
    if not helper.is_file():
        raise SystemExit(f"helper missing: {helper}")
    if not helper.stat().st_mode & 0o111:
        raise SystemExit(f"helper is not executable: {helper}")
    runtime_library = (
        "lib/libgstreamer-1.0.0.dylib"
        if args.voice_target.endswith("-apple-darwin")
        else "lib/libgstreamer-1.0.so.0"
    )
    if not (voice / runtime_library).is_file():
        raise SystemExit(f"runtime library missing: {voice / runtime_library}")
    for relative in LICENSE_FILES:
        if not (voice / relative).is_file():
            raise SystemExit(f"license file missing from the package: {relative}")
    if args.skip_helper_exec:
        # A CI job can verify a foreign target's package only by bytes: the
        # helper cannot run on the build host.
        print(f"voice payload verified by digest: {checked} files, helper not executed")
        return 0
    reported = subprocess.run(
        [str(helper), "--build-commit"], check=True, capture_output=True, text=True
    ).stdout.strip()
    if reported != args.build_commit:
        raise SystemExit(f"helper reports build commit {reported}, expected {args.build_commit}")
    print(f"voice payload verified: {checked} files, helper commit {reported}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    stage = subparsers.add_parser("stage", help="fetch and validate the pinned runtime")
    stage.add_argument("--app-target", required=True)
    stage.add_argument("--work", type=Path, required=True)
    stage.add_argument("--cache", type=Path, default=Path(".voice-runtime-cache"))
    stage.set_defaults(func=cmd_stage)

    build = subparsers.add_parser("build", help="build codex-voice-host from this repository")
    build.add_argument("--work", type=Path, required=True)
    build.add_argument("--voice-target", required=True)
    build.add_argument("--build-commit", required=True)
    build.set_defaults(func=cmd_build)

    assemble = subparsers.add_parser("assemble", help="write codex-resources/voice")
    assemble.add_argument("--work", type=Path, required=True)
    assemble.add_argument(
        "--app-binary",
        type=Path,
        help="built app binary to record in the manifest (optional for source packages)",
    )
    assemble.add_argument("--app-target", required=True)
    assemble.add_argument("--voice-target", required=True)
    assemble.add_argument("--helper", type=Path, required=True)
    assemble.add_argument("--build-commit", required=True)
    assemble.add_argument("--release-version", required=True)
    assemble.add_argument("--output", type=Path, required=True)
    assemble.set_defaults(func=cmd_assemble)

    verify = subparsers.add_parser("verify", help="re-check an assembled voice directory")
    verify.add_argument("--voice-dir", type=Path, required=True)
    verify.add_argument("--voice-target", required=True)
    verify.add_argument("--build-commit", required=True)
    verify.add_argument(
        "--skip-helper-exec",
        action="store_true",
        help="verify digests only (foreign target: the helper cannot run here)",
    )
    verify.set_defaults(func=cmd_verify)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
