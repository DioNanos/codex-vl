#!/usr/bin/env node
const { spawnSync } = require("node:child_process");
const { chmodSync, copyFileSync, existsSync, mkdirSync } = require("node:fs");
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
  console.error("[codex-vl]   npm install -g @mmmbuto/codex-vl");
  process.exit(1);
}

console.log("[codex-vl] preflight passed");
console.log("[codex-vl] compiling codex-vl natively (10-30 min on first install)");
console.log("");

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
    "-p",
    "codex-exec",
  ],
  {
    cwd: root,
    env: appendRustflags(process.env, "-C target-cpu=native"),
    stdio: "inherit",
  },
);

if (build.status !== 0) {
  process.exit(build.status || 1);
}

mkdirSync(vendorCodexDir, { recursive: true });
for (const binary of ["codex", "codex-exec"]) {
  const src = path.join(releaseDir, binary);
  const dest = path.join(vendorCodexDir, binary);
  if (!existsSync(src)) {
    fail(`expected build output missing: ${src}`);
  }
  copyFileSync(src, dest);
  chmodSync(dest, 0o755);
}

console.log("[codex-vl] installed local macOS binaries");
