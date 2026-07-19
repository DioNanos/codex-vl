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
  "aarch64-unknown-linux-musl": "@mmmbuto/codex-vl-linux-arm64",
  "aarch64-linux-android": "@mmmbuto/codex-vl-android-arm64",
  "aarch64-apple-darwin": "@mmmbuto/codex-vl-darwin-arm64",
};

const { platform, arch } = process;

let targetTriple = null;
switch (platform) {
  case "linux":
    if (arch === "x64") {
      targetTriple = "x86_64-unknown-linux-musl";
    } else if (arch === "arm64") {
      targetTriple = "aarch64-unknown-linux-musl";
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
// codex-vl fork: `codex-vl-exec` dispatches the single `codex` binary with the
// `exec` subcommand instead of shipping a standalone `codex-exec` binary. The
// `codex exec` subcommand uses the same ExecCli, so behavior is identical while
// the platform package drops ~220 MB (one fewer V8-linked binary).
const codexBinaryName = process.platform === "win32" ? "codex.exe" : "codex";
const localVendorRoot = path.join(__dirname, "..", "vendor");
const localBinaryPath = path.join(
  localVendorRoot,
  targetTriple,
  "codex",
  codexBinaryName,
);

let vendorRoot = null;
let resolvedPlatformPackageRoot = null;
try {
  const packageJsonPath = require.resolve(`${platformPackage}/package.json`);
  resolvedPlatformPackageRoot = path.dirname(packageJsonPath);
  vendorRoot = path.join(resolvedPlatformPackageRoot, "vendor");
} catch {
  // Fall back to a repository-local vendor payload below. Keep package
  // resolution separate from binary resolution so diagnostics can distinguish
  // an omitted optional dependency from a postinstall that produced no binary.
}

let archRoot = vendorRoot ? path.join(vendorRoot, targetTriple) : null;
let binaryPath = archRoot
  ? path.join(archRoot, "codex", codexBinaryName)
  : null;

if (!binaryPath || !existsSync(binaryPath)) {
  if (existsSync(localBinaryPath)) {
    vendorRoot = localVendorRoot;
    archRoot = path.join(vendorRoot, targetTriple);
    binaryPath = localBinaryPath;
  } else {
    throw new Error(nativePackageDiagnostic(detectPackageManager()));
  }
}

function detectPackageManager() {
  const userAgent = process.env.npm_config_user_agent || "";
  if (
    /\bbun\//.test(userAgent) ||
    (process.env.npm_execpath || "").includes("bun")
  ) {
    return "bun";
  }
  if (
    /\bpnpm\//.test(userAgent) ||
    (process.env.npm_execpath || "").includes("pnpm")
  ) {
    return "pnpm";
  }
  return "npm";
}

function packageManagerInstallCommand(packageManager) {
  return packageManager === "bun"
    ? "bun install -g @mmmbuto/codex-vl@latest"
    : packageManager === "pnpm"
      ? "pnpm add -g @mmmbuto/codex-vl@latest"
      : "npm install -g @mmmbuto/codex-vl@latest";
}

function nativePackageDiagnostic(packageManager) {
  const targetLabel =
    targetTriple === "aarch64-apple-darwin"
      ? "Darwin platform package"
      : "Platform package";
  const lines = resolvedPlatformPackageRoot
    ? [
        `${targetLabel} ${platformPackage} is installed at ${resolvedPlatformPackageRoot}, but its native binary is missing.`,
      ]
    : [`${targetLabel} ${platformPackage} is not installed.`];

  if (targetTriple === "aarch64-apple-darwin") {
    if (resolvedPlatformPackageRoot) {
      lines.push(
        "The macOS postinstall did not run or did not produce the native binary.",
      );
    } else {
      lines.push(
        "The npm optional dependency may have been omitted or its lifecycle install did not complete.",
      );
    }
    lines.push("Before reinstalling, verify: xcode-select -p; cargo --version");
    if (packageManager === "bun" || packageManager === "pnpm") {
      lines.push(
        `Reinstall Codex VL with lifecycle scripts enabled for ${packageManager}: ${packageManagerInstallCommand(packageManager)}`,
      );
    } else {
      lines.push(
        "Recover with:",
        "  npm uninstall -g @mmmbuto/codex-vl",
        "  npm install -g @mmmbuto/codex-vl@latest --allow-scripts=@mmmbuto/codex-vl --foreground-scripts",
      );
    }
    lines.push("The first macOS build can take 10-30 minutes.");
  } else {
    lines.push(
      `Reinstall Codex VL: ${packageManagerInstallCommand(packageManager)}`,
    );
  }

  return lines.join("\n");
}

function getUpdatedPath(newDirs) {
  const existingPath = process.env.PATH || "";
  return [
    ...newDirs,
    ...existingPath.split(path.delimiter).filter(Boolean),
  ].join(path.delimiter);
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
const child = spawn(resolvedBinaryPath, ["exec", ...process.argv.slice(2)], {
  stdio: "inherit",
  env,
});

child.on("error", (err) => {
  console.error(
    `Failed to execute Codex VL native binary at ${resolvedBinaryPath}: ${err.message}`,
  );
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
