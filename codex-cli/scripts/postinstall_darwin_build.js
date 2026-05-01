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

if (os.platform() !== "darwin" || os.arch() !== "arm64") {
  console.log("[codex-vl] skipping macOS local build on this platform");
  process.exit(0);
}

if (!existsSync(manifest)) {
  fail("source payload is missing codex-rs/Cargo.toml");
}

const cargo = spawnSync("cargo", ["--version"], { stdio: "ignore" });
if (cargo.status !== 0) {
  fail("cargo not found; macOS installs build locally and require Rust/Cargo");
}

console.log("[codex-vl] building macOS native binaries locally");
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
  { cwd: root, stdio: "inherit" },
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
