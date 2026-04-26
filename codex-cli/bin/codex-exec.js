#!/usr/bin/env node
// Unified entry point for the Codex Exec CLI.

import { spawn } from "node:child_process";
import { existsSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const require = createRequire(import.meta.url);

const PLATFORM_PACKAGE_BY_TARGET = {
  "x86_64-unknown-linux-musl": "@mmmbuto/codex-vl-linux-x64",
  "aarch64-linux-android": "@mmmbuto/codex-vl-android-arm64",
  "aarch64-apple-darwin": "@mmmbuto/codex-vl-darwin-arm64",
};

const { platform, arch } = process;

let targetTriple = null;
switch (platform) {
  case "linux":
    if (arch === "x64") {
      targetTriple = "x86_64-unknown-linux-musl";
    }
    break;
  case "android":
    if (arch === "arm64") {
      targetTriple = "aarch64-linux-android";
    }
    break;
  case "darwin":
    if (arch === "arm64") {
      targetTriple = "aarch64-apple-darwin";
    }
    break;
  default:
    break;
}

if (!targetTriple) {
  throw new Error(`Unsupported platform: ${platform} (${arch})`);
}

const platformPackage = PLATFORM_PACKAGE_BY_TARGET[targetTriple];
const codexExecBinaryName =
  process.platform === "win32" ? "codex-exec.exe" : "codex-exec";
const localVendorRoot = path.join(__dirname, "..", "vendor");
const localBinaryPath = path.join(
  localVendorRoot,
  targetTriple,
  "codex",
  codexExecBinaryName,
);

let vendorRoot;
try {
  const packageJsonPath = require.resolve(`${platformPackage}/package.json`);
  vendorRoot = path.join(path.dirname(packageJsonPath), "vendor");
} catch {
  if (existsSync(localBinaryPath)) {
    vendorRoot = localVendorRoot;
  } else {
    throw new Error(
      `Missing optional dependency ${platformPackage}. Reinstall Codex VL: npm install -g @mmmbuto/codex-vl@latest`,
    );
  }
}

const archRoot = path.join(vendorRoot, targetTriple);
const binaryPath = path.join(archRoot, "codex", codexExecBinaryName);

function getUpdatedPath(newDirs) {
  const existingPath = process.env.PATH || "";
  return [...newDirs, ...existingPath.split(path.delimiter).filter(Boolean)].join(
    path.delimiter,
  );
}

function sanitizeAndroidLdLibraryPath(binDir) {
  const termuxPrefix = process.env.PREFIX || "/data/data/com.termux/files/usr";
  const blocked = new Set([
    `${termuxPrefix}/lib`,
    `${termuxPrefix}/libexec`,
    "/data/data/com.termux/files/usr/lib",
    "/data/data/com.termux/files/usr/libexec",
  ]);

  const extraPaths = (process.env.LD_LIBRARY_PATH || "")
    .split(":")
    .filter((entry) => entry && !blocked.has(entry));

  return [binDir, ...extraPaths].join(":");
}

function safeRealpath(targetPath) {
  try {
    return realpathSync(targetPath);
  } catch {
    return null;
  }
}

const additionalDirs = [];
const pathDir = path.join(archRoot, "path");
if (existsSync(pathDir)) {
  additionalDirs.push(pathDir);
}

const env = {
  ...process.env,
  PATH: getUpdatedPath(additionalDirs),
  CODEX_MANAGED_BY_NPM: "1",
};

if (platform === "android") {
  env.CODEX_SELF_EXE = binaryPath;
  env.LD_LIBRARY_PATH = sanitizeAndroidLdLibraryPath(path.dirname(binaryPath));
}

const resolvedBinaryPath = safeRealpath(binaryPath) ?? binaryPath;
const child = spawn(resolvedBinaryPath, process.argv.slice(2), {
  stdio: "inherit",
  env,
});

child.on("error", (err) => {
  console.error(err);
  process.exit(1);
});

const forwardSignal = (signal) => {
  if (child.killed) {
    return;
  }
  try {
    child.kill(signal);
  } catch {
    /* ignore */
  }
};

["SIGINT", "SIGTERM", "SIGHUP"].forEach((sig) => {
  process.on(sig, () => forwardSignal(sig));
});

const childResult = await new Promise((resolve) => {
  child.on("exit", (code, signal) => {
    if (signal) {
      resolve({ type: "signal", signal });
    } else {
      resolve({ type: "code", exitCode: code ?? 1 });
    }
  });
});

if (childResult.type === "signal") {
  process.kill(process.pid, childResult.signal);
} else {
  process.exit(childResult.exitCode);
}
