#!/usr/bin/env node
// Unified entry point for the Codex CLI.

import { spawn } from "node:child_process";
import {
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "path";
import { fileURLToPath } from "url";

// __dirname equivalent in ESM
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const require = createRequire(import.meta.url);
const scriptRealPath = safeRealpath(__filename) ?? __filename;
const BOOTSTRAP_STATE_SCHEMA_VERSION = 1;
const bootstrapModeOverride = (process.env.CODEX_VL_BOOTSTRAP || "").toLowerCase();
const skipNativeExec = process.env.CODEX_VL_SKIP_EXEC === "1";

const PLATFORM_PACKAGE_BY_TARGET = {
  "x86_64-unknown-linux-musl": "@mmmbuto/codex-vl-linux-x64",
  "aarch64-linux-android": "@mmmbuto/codex-vl-android-arm64",
  "aarch64-apple-darwin": "@mmmbuto/codex-vl-darwin-arm64",
};

const { platform, arch } = process;

let targetTriple = null;
switch (platform) {
  case "linux":
    switch (arch) {
      case "x64":
        targetTriple = "x86_64-unknown-linux-musl";
        break;
      default:
        break;
    }
    break;
  case "android":
    switch (arch) {
      case "arm64":
        targetTriple = "aarch64-linux-android";
        break;
      default:
        break;
    }
    break;
  case "darwin":
    switch (arch) {
      case "arm64":
        targetTriple = "aarch64-apple-darwin";
        break;
      default:
        break;
    }
    break;
  default:
    break;
}

if (!targetTriple) {
  throw new Error(`Unsupported platform: ${platform} (${arch})`);
}

const platformPackage = PLATFORM_PACKAGE_BY_TARGET[targetTriple];
if (!platformPackage) {
  throw new Error(`Unsupported target triple: ${targetTriple}`);
}

const codexBinaryName = process.platform === "win32" ? "codex.exe" : "codex";
const localVendorRoot = path.join(__dirname, "..", "vendor");
const localBinaryPath = path.join(
  localVendorRoot,
  targetTriple,
  "codex",
  codexBinaryName,
);

let vendorRoot;
try {
  const packageJsonPath = require.resolve(`${platformPackage}/package.json`);
  vendorRoot = path.join(path.dirname(packageJsonPath), "vendor");
} catch {
  if (existsSync(localBinaryPath)) {
    vendorRoot = localVendorRoot;
  } else {
    const packageManager = detectPackageManager();
    const updateCommand =
      packageManager === "bun"
        ? "bun install -g @mmmbuto/codex-vl@latest"
        : "npm install -g @mmmbuto/codex-vl@latest";
    throw new Error(
      `Missing optional dependency ${platformPackage}. Reinstall Codex VL: ${updateCommand}`,
    );
  }
}

if (!vendorRoot) {
  const packageManager = detectPackageManager();
  const updateCommand =
    packageManager === "bun"
      ? "bun install -g @mmmbuto/codex-vl@latest"
      : "npm install -g @mmmbuto/codex-vl@latest";
  throw new Error(
    `Missing optional dependency ${platformPackage}. Reinstall Codex VL: ${updateCommand}`,
  );
}

const archRoot = path.join(vendorRoot, targetTriple);
const binaryPath = path.join(archRoot, "codex", codexBinaryName);

// Use an asynchronous spawn instead of spawnSync so that Node is able to
// respond to signals (e.g. Ctrl-C / SIGINT) while the native binary is
// executing. This allows us to forward those signals to the child process
// and guarantees that when either the child terminates or the parent
// receives a fatal signal, both processes exit in a predictable manner.

function getUpdatedPath(newDirs) {
  const pathSep = process.platform === "win32" ? ";" : ":";
  const existingPath = process.env.PATH || "";
  const updatedPath = [
    ...newDirs,
    ...existingPath.split(pathSep).filter(Boolean),
  ].join(pathSep);
  return updatedPath;
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

/**
 * Use heuristics to detect the package manager that was used to install Codex
 * in order to give the user a hint about how to update it.
 */
function detectPackageManager() {
  const userAgent = process.env.npm_config_user_agent || "";
  if (/\bbun\//.test(userAgent)) {
    return "bun";
  }

  const execPath = process.env.npm_execpath || "";
  if (execPath.includes("bun")) {
    return "bun";
  }

  if (
    __dirname.includes(".bun/install/global") ||
    __dirname.includes(".bun\\install\\global")
  ) {
    return "bun";
  }

  return userAgent ? "npm" : null;
}

function safeRealpath(targetPath) {
  try {
    return realpathSync(targetPath);
  } catch {
    return null;
  }
}

function resolveCodexHome() {
  const codexHome = process.env.CODEX_HOME;
  if (codexHome && codexHome.trim().length > 0) {
    return path.resolve(codexHome);
  }

  return path.join(os.homedir(), ".codex");
}

function installModeStatePath() {
  return path.join(resolveCodexHome(), "codex-vl", "install-mode.json");
}

function readInstallModeState() {
  const statePath = installModeStatePath();
  if (!existsSync(statePath)) {
    return null;
  }

  try {
    const parsed = JSON.parse(readFileSync(statePath, "utf8"));
    if (parsed?.schemaVersion !== BOOTSTRAP_STATE_SCHEMA_VERSION) {
      return null;
    }
    if (parsed?.mode !== "side_by_side" && parsed?.mode !== "main") {
      return null;
    }
    return parsed;
  } catch {
    return null;
  }
}

function writeInstallModeState(state) {
  const statePath = installModeStatePath();
  mkdirSync(path.dirname(statePath), { recursive: true });
  writeFileSync(
    statePath,
    JSON.stringify(
      {
        schemaVersion: BOOTSTRAP_STATE_SCHEMA_VERSION,
        configured: true,
        mode: state.mode,
        aliasPath: state.aliasPath ?? null,
      },
      null,
      2,
    ) + "\n",
    "utf8",
  );
}

function pathEntries() {
  return (process.env.PATH || "")
    .split(path.delimiter)
    .filter(Boolean);
}

function isPathEntryAccessible(dirPath) {
  try {
    return lstatSync(dirPath).isDirectory();
  } catch {
    return false;
  }
}

function findCommandOnPath(commandName) {
  const entries = pathEntries();
  for (let index = 0; index < entries.length; index += 1) {
    const dirPath = entries[index];
    if (!isPathEntryAccessible(dirPath)) {
      continue;
    }

    const candidate = path.join(dirPath, commandName);
    if (existsSync(candidate)) {
      return { path: candidate, dir: dirPath, index };
    }
  }

  return null;
}

function findMatchingCommandOnPath(commandName) {
  const candidate = findCommandOnPath(commandName);
  if (!candidate) {
    return null;
  }

  return safeRealpath(candidate.path) === scriptRealPath ? candidate : null;
}

async function maybeBootstrapInstallMode() {
  if (bootstrapModeOverride === "skip") {
    return;
  }

  const savedState = readInstallModeState();
  const existingCodex = findCommandOnPath("codex");
  const existingCodexMatchesThisInstall =
    existingCodex && safeRealpath(existingCodex.path) === scriptRealPath;

  if (existingCodexMatchesThisInstall && bootstrapModeOverride !== "force") {
    writeInstallModeState({
      mode: "main",
      aliasPath: existingCodex.path,
    });
    return;
  }

  if (savedState && bootstrapModeOverride !== "force") {
    return;
  }

  writeInstallModeState({
    mode: "side_by_side",
    aliasPath: null,
  });
}

const additionalDirs = [];
const pathDir = path.join(archRoot, "path");
if (existsSync(pathDir)) {
  additionalDirs.push(pathDir);
}
const updatedPath = getUpdatedPath(additionalDirs);

await maybeBootstrapInstallMode();

if (skipNativeExec) {
  process.exit(0);
}

const env = { ...process.env, PATH: updatedPath };
if (platform === "android") {
  env.CODEX_SELF_EXE = binaryPath;
  env.LD_LIBRARY_PATH = sanitizeAndroidLdLibraryPath(path.dirname(binaryPath));
}
const packageManagerEnvVar =
  detectPackageManager() === "bun"
    ? "CODEX_MANAGED_BY_BUN"
    : "CODEX_MANAGED_BY_NPM";
env[packageManagerEnvVar] = "1";

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  env,
});

child.on("error", (err) => {
  // Typically triggered when the binary is missing or not executable.
  // Re-throwing here will terminate the parent with a non-zero exit code
  // while still printing a helpful stack trace.
  // eslint-disable-next-line no-console
  console.error(err);
  process.exit(1);
});

// Forward common termination signals to the child so that it shuts down
// gracefully. In the handler we temporarily disable the default behavior of
// exiting immediately; once the child has been signaled we simply wait for
// its exit event which will in turn terminate the parent (see below).
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

// When the child exits, mirror its termination reason in the parent so that
// shell scripts and other tooling observe the correct exit status.
// Wrap the lifetime of the child process in a Promise so that we can await
// its termination in a structured way. The Promise resolves with an object
// describing how the child exited: either via exit code or due to a signal.
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
  // Re-emit the same signal so that the parent terminates with the expected
  // semantics (this also sets the correct exit code of 128 + n).
  process.kill(process.pid, childResult.signal);
} else {
  process.exit(childResult.exitCode);
}
