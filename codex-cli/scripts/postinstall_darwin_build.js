#!/usr/bin/env node
const { spawnSync } = require("node:child_process");
const {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
} = require("node:fs");
const { createHash } = require("node:crypto");
const os = require("node:os");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const target = "aarch64-apple-darwin";
const manifest = path.join(root, "codex-rs", "Cargo.toml");
const releaseDir = path.join(root, "codex-rs", "target", target, "release");
const vendorCodexDir = path.join(root, "vendor", target, "codex");

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

console.log("[codex-vl] installed local macOS binaries");
