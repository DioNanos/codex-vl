#!/usr/bin/env node
const { spawnSync } = require("node:child_process");
const {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} = require("node:fs");
const { createHash } = require("node:crypto");
const os = require("node:os");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const target = "aarch64-apple-darwin";
const manifest = path.join(root, "codex-rs", "Cargo.toml");
const releaseDir = path.join(root, "codex-rs", "target", target, "release");
const vendorCodexDir = path.join(root, "vendor", target, "bin");

function fail(message) {
  console.error(`[codex-vl] ${message}`);
  process.exit(1);
}

function logCheck(ok, label, fixHint) {
  const mark = ok ? "[OK]     " : "[MISSING]";
  console.log(`[codex-vl] ${mark} ${label}`);
  if (!ok && fixHint) {
    console.log(`[codex-vl]           Fix: ${fixHint}`);
  }
}

function hasCommand(cmd, args = ["--version"]) {
  const result = spawnSync(cmd, args, { stdio: "ignore" });
  return result.status === 0;
}

function hasXcodeCLT() {
  const result = spawnSync("xcode-select", ["-p"], { stdio: "pipe" });
  if (result.status !== 0) return false;
  const stdout = result.stdout ? result.stdout.toString().trim() : "";
  return stdout.length > 0;
}

function hasRustupTarget(targetTriple) {
  const result = spawnSync("rustup", ["target", "list", "--installed"], {
    stdio: "pipe",
  });
  if (result.status !== 0) return false;
  const stdout = result.stdout ? result.stdout.toString() : "";
  return stdout
    .split("\n")
    .map((line) => line.trim())
    .includes(targetTriple);
}

function appendRustflags(env, flags) {
  const existing = env.RUSTFLAGS || "";
  if (existing.includes("target-cpu=")) {
    return env;
  }

  return {
    ...env,
    RUSTFLAGS: [existing, flags].filter(Boolean).join(" "),
  };
}

// --- ripgrep provisioning -------------------------------------------------
// The daemon refuses to install a package whose layout is incomplete
// (app-server-daemon validate_package): codex-package.json, bin/codex,
// bin/codex-code-mode-host and codex-path/rg must all exist. The source build
// produces the two binaries; the pinned ripgrep is what this postinstall has to
// add. The digest comes from the DotSlash manifest that the prebuilt linux
// payloads already use (scripts/codex_package/rg, entry macos-aarch64), staged
// next to this script as scripts/rg-manifest.
const RG_MANIFEST_REL = path.join("scripts", "rg-manifest");
const RG_PLATFORM_KEY = "macos-aarch64";
// The daemon validates a regular file for every entry, and the executable bit
// for the programs (app-server-daemon prepare_install.rs validate_package).
const PACKAGE_REQUIREMENTS = [
  { relative: "codex-package.json", program: false },
  { relative: "bin/codex", program: true },
  { relative: "bin/codex-code-mode-host", program: true },
  { relative: "codex-path/rg", program: true },
];

function readRgManifest(manifestPath) {
  let text;
  try {
    text = readFileSync(manifestPath, "utf8");
  } catch (error) {
    throw new Error(`ripgrep manifest is not readable at ${manifestPath}: ${error.message}`);
  }
  // The manifest is a DotSlash script: it carries a shebang line that
  // JSON.parse cannot see.
  const body = text.startsWith("#!") ? text.slice(text.indexOf("\n") + 1) : text;
  try {
    return JSON.parse(body);
  } catch (error) {
    throw new Error(`ripgrep manifest at ${manifestPath} is not valid JSON: ${error.message}`);
  }
}

// Every field the integrity check needs is mandatory. A missing or malformed
// size or digest must be an error - never a check that is silently skipped.
function rgPlan(manifest, platformKey = RG_PLATFORM_KEY) {
  const entry = manifest && manifest.platforms ? manifest.platforms[platformKey] : null;
  if (!entry) {
    throw new Error(`ripgrep manifest has no '${platformKey}' entry`);
  }
  const provider = entry.providers && entry.providers[0] ? entry.providers[0] : null;
  if (!provider || typeof provider.url !== "string" || provider.url.length === 0) {
    throw new Error(`ripgrep manifest entry '${platformKey}' declares no download URL`);
  }
  if (!Number.isInteger(entry.size) || entry.size <= 0) {
    throw new Error(
      `ripgrep manifest entry '${platformKey}' has no usable size: ${JSON.stringify(entry.size)}`,
    );
  }
  if (entry.hash && entry.hash !== "sha256") {
    throw new Error(
      `ripgrep manifest entry '${platformKey}' uses an unsupported hash: ${entry.hash}`,
    );
  }
  if (typeof entry.digest !== "string" || !/^[0-9a-f]{64}$/.test(entry.digest)) {
    throw new Error(
      `ripgrep manifest entry '${platformKey}' has no usable sha256 digest: ` +
        JSON.stringify(entry.digest),
    );
  }
  if (typeof entry.path !== "string" || entry.path.length === 0) {
    throw new Error(`ripgrep manifest entry '${platformKey}' declares no archive member`);
  }
  return {
    url: provider.url,
    size: entry.size,
    hash: "sha256",
    digest: entry.digest,
    member: entry.path,
  };
}

// Not every tool answers `--version`: macOS `codesign` exits 2 on it, so asking
// for a version to decide whether a command exists reports a false negative.
// Ask the shell for the executable instead (the tool is never invoked).
function commandInPath(command, { run = spawnSync, env = process.env } = {}) {
  if (!/^[\w.+-]+$/.test(command)) {
    throw new Error(`unsafe command name: ${command}`);
  }
  const result = run("/bin/sh", ["-c", `command -v ${command}`], {
    encoding: "utf8",
    env,
  });
  if (result.status !== 0) return null;
  const found = String(result.stdout || "").trim().split("\n")[0];
  return found && existsSync(found) ? found : null;
}

const CODESIGN_PATH = "/usr/bin/codesign";

function findCodesign({ run = spawnSync, env = process.env } = {}) {
  return (
    commandInPath("codesign", { run, env }) ||
    (existsSync(CODESIGN_PATH) ? CODESIGN_PATH : null)
  );
}

// tar on macOS is bsdtar; either one writes a member to stdout with -xOf.
function findArchiveTool({ run = spawnSync, env = process.env } = {}) {
  return commandInPath("tar", { run, env }) || commandInPath("bsdtar", { run, env });
}

// Bounded on purpose: a slow network must not leave the install hanging. Three
// retries, 30 s to connect, 300 s in total. Once those are exhausted the caller
// treats it as a failed download and falls back to the system ripgrep.
const CURL_ARGS = [
  "-fsSL",
  "--retry",
  "3",
  "--retry-delay",
  "2",
  "--connect-timeout",
  "30",
  "--max-time",
  "300",
];

function downloadWithCurl(url, dest) {
  if (!commandInPath("curl")) {
    throw new Error("curl is not available: cannot download the pinned ripgrep");
  }
  const result = spawnSync("curl", [...CURL_ARGS, url, "-o", dest], { stdio: "inherit" });
  if (result.status !== 0) {
    throw new Error(`download failed for ${url} (curl exited with ${result.status})`);
  }
}

function findSystemRipgrep() {
  return commandInPath("rg");
}

function extractArchiveMember(archive, member, dest, { run = spawnSync } = {}) {
  const archiveTool = findArchiveTool();
  if (!archiveTool) {
    throw new Error("neither tar nor bsdtar is available to extract ripgrep");
  }
  const extracted = run(archiveTool, ["-xOf", archive, member], {
    maxBuffer: 128 * 1024 * 1024,
  });
  if (extracted.status !== 0) {
    throw new Error(`failed to extract ${member} from ${path.basename(archive)}`);
  }
  writeFileSync(dest, extracted.stdout);
}

// The ad-hoc signature is hardening, not a condition for the binary to run:
// a file extracted from a tarball carries no quarantine attribute. So a missing
// or failing codesign is a loud warning - never a silent skip, and never a
// reason to throw away a finished 10-30 minute build.
function codesignAdhoc(target, {
  platform = os.platform(),
  find = findCodesign,
  run = spawnSync,
  log = (message) => console.log(`[codex-vl] ${message}`),
  warn = (message) => console.warn(`[codex-vl] ${message}`),
} = {}) {
  if (platform !== "darwin") return false;
  const codesign = find();
  if (!codesign) {
    warn("codesign not found: codex-path/rg stays unsigned");
    return false;
  }
  const sign = run(codesign, ["-s", "-", "-f", target], { stdio: "pipe" });
  if (sign.status !== 0) {
    warn(`codesign failed for ${target}: the file stays unsigned`);
    return false;
  }
  const verify = run(codesign, ["-v", target], { stdio: "pipe" });
  if (verify.status !== 0) {
    warn(`codesign verification failed for ${target}: the signature is not trusted`);
    return false;
  }
  log(`ad-hoc signed ${path.basename(target)}`);
  return true;
}

// Pinned download first (reproducible, digest-verified). The system ripgrep is
// the declared fallback for a download that cannot happen - offline, or the
// release asset unreachable - and it says so. Anything else (a manifest that is
// missing or unreadable, an entry without usable integrity metadata, a size or
// a digest that does not match) is an error: an unverifiable package is not
// installed quietly.
function provisionRipgrep({
  packageDir,
  manifestPath,
  platformKey = RG_PLATFORM_KEY,
  download = downloadWithCurl,
  findSystemRg = findSystemRipgrep,
  codesign = codesignAdhoc,
  log = (message) => console.log(`[codex-vl] ${message}`),
  warn = (message) => console.warn(`[codex-vl] ${message}`),
} = {}) {
  const pathDir = path.join(packageDir, "codex-path");
  const dest = path.join(pathDir, "rg");
  mkdirSync(pathDir, { recursive: true });

  // Bad metadata, a missing entry and a broken manifest are hard errors: the
  // fallback exists for a download that cannot happen, not to paper over a
  // package whose integrity cannot be verified.
  const plan = rgPlan(readRgManifest(manifestPath), platformKey);

  const archive = path.join(pathDir, path.basename(new URL(plan.url).pathname));
  let downloadError = null;
  try {
    log(`downloading ripgrep for ${platformKey} from ${plan.url}`);
    download(plan.url, archive);
  } catch (error) {
    downloadError = error;
  }

  if (downloadError) {
    const systemRg = findSystemRg();
    if (!systemRg) {
      throw new Error(
        "no ripgrep available: the pinned download failed and no system rg was found. " +
          "Install ripgrep (for example: brew install ripgrep) and reinstall this package.",
      );
    }
    warn(
      `ripgrep download failed (${downloadError.message}): falling back to the system ` +
        `ripgrep at ${systemRg}, whose version is not pinned by this package`,
    );
    copyFileSync(systemRg, dest);
  } else {
    const bytes = readFileSync(archive);
    if (bytes.length !== plan.size) {
      throw new Error(`ripgrep archive size mismatch: expected ${plan.size}, got ${bytes.length}`);
    }
    const digest = createHash(plan.hash).update(bytes).digest("hex");
    if (digest !== plan.digest) {
      throw new Error(`ripgrep checksum mismatch: expected ${plan.digest}, got ${digest}`);
    }
    extractArchiveMember(archive, plan.member, dest);
    rmSync(archive, { force: true });
  }

  chmodSync(dest, 0o755);
  codesign(dest, { log, warn });
  return dest;
}

// The exact list the daemon validates, with the daemon's own predicate: a
// regular file, and for the programs the executable bit. Keep it in one place
// so the install-time guard and the daemon cannot drift apart silently.
function verifyPackageLayout({ packageDir, stat = statSync } = {}) {
  return PACKAGE_REQUIREMENTS.filter(({ relative, program }) => {
    let info;
    try {
      info = stat(path.join(packageDir, relative));
    } catch {
      return true;
    }
    if (!info.isFile()) return true;
    return program && (info.mode & 0o111) === 0;
  }).map(({ relative }) => relative);
}

// One place decides whether the package is installable. A failed integrity or
// provisioning step must fail the install even when every path is already
// there - otherwise a checksum mismatch ends in "layout complete".
function packageLayoutFailure({ packageDir, provisionError = null } = {}) {
  if (provisionError) {
    return `ripgrep provisioning failed: ${provisionError.message}`;
  }
  const missing = verifyPackageLayout({ packageDir });
  if (missing.length > 0) {
    return `incomplete macOS package layout, missing: ${missing.join(", ")}`;
  }
  return null;
}

function main() {
  if (os.platform() !== "darwin" || os.arch() !== "arm64") {
    console.log("[codex-vl] skipping macOS local build on this platform");
    process.exit(0);
  }

  if (!existsSync(manifest)) {
    fail("source payload is missing codex-rs/Cargo.toml");
  }

  console.log("[codex-vl] preflight: checking macOS build dependencies");

  const xcodeOk = hasXcodeCLT();
  logCheck(xcodeOk, "Xcode Command Line Tools", "xcode-select --install");

  const cargoOk = hasCommand("cargo");
  logCheck(
    cargoOk,
    "Rust toolchain (cargo)",
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
  );

  let rustupOk = false;
  let targetOk = false;
  let nonRustupCargo = false;
  if (cargoOk) {
    rustupOk = hasCommand("rustup");
    if (rustupOk) {
      targetOk = hasRustupTarget(target);
      logCheck(targetOk, `Rust target ${target}`, `rustup target add ${target}`);
    } else {
      // Homebrew / standalone Rust installs ship cargo without rustup. On a
      // native arm64 macOS host the aarch64-apple-darwin target is the host
      // ABI, so cargo can build it directly without `rustup target add`. We
      // log this as informational and let cargo report the real error if any.
      nonRustupCargo = true;
      console.log(
        "[codex-vl] [INFO]    rustup not found (Homebrew/standalone Rust); assuming cargo builds aarch64-apple-darwin natively",
      );
      console.log(
        "[codex-vl]           If the build fails with a target error, run: rustup target add aarch64-apple-darwin",
      );
    }
  }

  const targetGateMissing = cargoOk && rustupOk && !targetOk;
  if (!xcodeOk || !cargoOk || targetGateMissing) {
    console.error("");
    console.error(
      "[codex-vl] missing build dependencies — install the items marked [MISSING] above, then re-run:",
    );
    console.error(
      "[codex-vl]   npm install -g @mmmbuto/codex-vl@latest --allow-scripts=@mmmbuto/codex-vl --foreground-scripts",
    );
    process.exit(1);
  }

  console.log("[codex-vl] preflight passed");
  console.log(
    "[codex-vl] compiling codex-vl natively (10-30 min on first install)",
  );
  console.log("");

  // V8 prebuilt. code-mode-runtime enables `v8_enable_sandbox`, so the v8 build
  // script derives the archive name from the enabled Cargo features and asks for
  // `librusty_v8_ptrcomp_sandbox_release_<target>`. denoland/rusty_v8 v150.4.0
  // publishes no sandbox assets at all, so the default download is a 404 and the
  // build stops there. The matching archive and binding come from this project's
  // own rusty-v8 release; both are pinned by checksum, and a mismatch aborts.
  const v8Version = "150.4.0";
  const v8Base = `https://github.com/openai/codex/releases/download/rusty-v8-v${v8Version}`;
  const v8Dir = path.join(root, ".rusty_v8");
  const v8Archive = path.join(
    v8Dir,
    `librusty_v8_ptrcomp_sandbox_release_${target}.a.gz`,
  );
  const v8Binding = path.join(
    v8Dir,
    `src_binding_ptrcomp_sandbox_release_${target}.rs`,
  );
  const v8Checksums = {
    [path.basename(v8Archive)]:
      "00adbb48798848c77550441c68673a5e8529b8e1b73eabcdee232cb39b40f4a1",
    [path.basename(v8Binding)]:
      "ca5adf0cf89c9a70ad460ae73648b2fe89b74aa113b3cb7f757b6a02b758394f",
  };

  mkdirSync(v8Dir, { recursive: true });
  for (const dest of [v8Archive, v8Binding]) {
    const name = path.basename(dest);
    if (!existsSync(dest)) {
      console.log(`[codex-vl] downloading ${name}`);
      const curl = spawnSync(
        "curl",
        ["-fsSL", `${v8Base}/${name}`, "-o", dest],
        { stdio: "inherit" },
      );
      if (curl.status !== 0) {
        fail(`failed to download ${name} from ${v8Base}`);
      }
    }
    const actual = createHash("sha256")
      .update(readFileSync(dest))
      .digest("hex");
    if (actual !== v8Checksums[name]) {
      fail(
        `checksum mismatch for ${name}: expected ${v8Checksums[name]}, got ${actual}`,
      );
    }
  }
  console.log("[codex-vl] V8 prebuilt verified");

  const build = spawnSync(
    "cargo",
    [
      "build",
      "--manifest-path",
      manifest,
      "--target",
      target,
      "--release",
      "-p",
      "codex-cli",
      // Code mode runs out-of-process since rust-v0.147.0: the CLI spawns this
      // host next to itself and fails closed without it. Building only codex-cli
      // leaves macOS installs with code mode permanently unavailable.
      "-p",
      "codex-code-mode-host",
    ],
    {
      cwd: root,
      env: {
        ...appendRustflags(process.env, "-C target-cpu=native"),
        RUSTY_V8_ARCHIVE: v8Archive,
        RUSTY_V8_SRC_BINDING_PATH: v8Binding,
      },
      stdio: "inherit",
    },
  );

  if (build.status !== 0) {
    process.exit(build.status || 1);
  }

  mkdirSync(vendorCodexDir, { recursive: true });
  for (const binary of ["codex", "codex-code-mode-host"]) {
    const src = path.join(releaseDir, binary);
    const dest = path.join(vendorCodexDir, binary);
    if (!existsSync(src)) {
      fail(`expected build output missing: ${src}`);
    }
    copyFileSync(src, dest);
    chmodSync(dest, 0o755);
  }

  // The daemon requires the codex-package layout manifest next to the
  // binaries (prepare_install: "no complete local package" without it). The
  // source build IS the fork: the variant pins the identity so a staged
  // upstream binary is never executed in its place.
  const workspaceVersion = readFileSync(path.join(root, "codex-rs", "Cargo.toml"), "utf8")
    .split("\n")
    .map((line) => line.trim())
    .find((line) => line.startsWith("version = "));
  if (!workspaceVersion) {
    fail("workspace version not found in codex-rs/Cargo.toml");
  }
  const codexPackageManifest = {
    layoutVersion: 1,
    version: workspaceVersion.split('"')[1],
    target,
    variant: "codex-vl",
    entrypoint: "bin/codex",
    resourcesDir: "codex-resources",
    pathDir: "codex-path",
  };
  writeFileSync(
    path.join(root, "vendor", target, "codex-package.json"),
    `${JSON.stringify(codexPackageManifest, null, 2)}\n`,
  );
  console.log("[codex-vl] installed local macOS binaries");
  console.log(
    `[codex-vl] codex-package.json written: ${path.join(root, "vendor", target, "codex-package.json")}`,
  );

  // The source build cannot produce ripgrep, but the daemon will not install a
  // package without it: the layout is only complete after this step.
  const vendorTargetDir = path.join(root, "vendor", target);
  let provisionError = null;
  try {
    provisionRipgrep({
      packageDir: vendorTargetDir,
      manifestPath: path.join(root, RG_MANIFEST_REL),
    });
  } catch (error) {
    provisionError = error;
  }

  const failure = packageLayoutFailure({ packageDir: vendorTargetDir, provisionError });
  if (failure) {
    fail(
      `${failure}. Reinstall with: npm install -g @mmmbuto/codex-vl@latest ` +
        "--allow-scripts=@mmmbuto/codex-vl --foreground-scripts",
    );
  }
  console.log(
    `[codex-vl] package layout complete: ` +
      PACKAGE_REQUIREMENTS.map(({ relative }) => relative).join(", "),
  );
}

if (require.main === module) {
  main();
}

module.exports = {
  provisionRipgrep,
  verifyPackageLayout,
  packageLayoutFailure,
  readRgManifest,
  findCodesign,
  codesignAdhoc,
  commandInPath,
};
