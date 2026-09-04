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
const codexPackageRoot = realpathSync(path.join(__dirname, ".."));
const BOOTSTRAP_STATE_SCHEMA_VERSION = 1;
const bootstrapModeOverride = (
  process.env.CODEX_VL_BOOTSTRAP || ""
).toLowerCase();
const skipNativeExec = process.env.CODEX_VL_SKIP_EXEC === "1";

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
    switch (arch) {
      case "x64":
        targetTriple = "x86_64-unknown-linux-musl";
        break;
      case "arm64":
        targetTriple = "aarch64-unknown-linux-musl";
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

// Fork-owned native package resolution (restored verbatim from 0.137.0):
// resolves both the upstream vendor/<triple>/bin and the fork CI
// vendor/<triple>/codex payload layouts AND returns the PATH shim dir
// (`pathDir`) consumed below — the 0.138 merge had replaced this block
// with upstream's simpler resolver, orphaning `pathDir`.
const codexBinaryName = process.platform === "win32" ? "codex.exe" : "codex";
const localVendorRoot = path.join(__dirname, "..", "vendor");
const packageBinaryPath = (vendorRoot) =>
  path.join(vendorRoot, targetTriple, "bin", codexBinaryName);
const legacyBinaryPath = (vendorRoot) =>
  path.join(vendorRoot, targetTriple, "codex", codexBinaryName);

function resolveNativePackage(vendorRoot) {
  const packageRoot = path.join(vendorRoot, targetTriple);
  const binaryPath = packageBinaryPath(vendorRoot);
  if (existsSync(binaryPath)) {
    return {
      binaryPath,
      pathDir: path.join(packageRoot, "codex-path"),
    };
  }

  const legacyPath = legacyBinaryPath(vendorRoot);
  if (existsSync(legacyPath)) {
    return {
      binaryPath: legacyPath,
      pathDir: path.join(packageRoot, "path"),
    };
  }

  return null;
}

let nativePackage;
let resolvedPlatformPackageRoot = null;
try {
  const packageJsonPath = require.resolve(`${platformPackage}/package.json`);
  resolvedPlatformPackageRoot = path.dirname(packageJsonPath);
  nativePackage = resolveNativePackage(
    path.join(resolvedPlatformPackageRoot, "vendor"),
  );
} catch {
  // Fall back to a repository-local vendor payload below. Keep package
  // resolution separate from binary resolution so diagnostics can distinguish
  // an omitted optional dependency from a postinstall that produced no binary.
}

nativePackage ??= resolveNativePackage(localVendorRoot);

if (!nativePackage) {
  const packageManager = detectPackageManager();
  throw new Error(nativePackageDiagnostic(packageManager));
}

const { binaryPath, pathDir } = nativePackage;

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

// codex-vl fork: pnpm links the package under the fork-owned scope, so the
// ownership probe must resolve that scope or pnpm detection is dead for fork
// users.
function isPnpmOwnedCodexInstall(nodeModulesDir) {
  if (!existsSync(path.join(nodeModulesDir, ".modules.yaml"))) {
    return false;
  }

  try {
    return (
      realpathSync(path.join(nodeModulesDir, "@mmmbuto", "codex-vl")) ===
      codexPackageRoot
    );
  } catch {
    return false;
  }
}

function isVitePlusOwnedCodexInstall(packagesDir) {
  if (path.basename(packagesDir) !== "packages") {
    return false;
  }

  try {
    const metadata = JSON.parse(
      readFileSync(path.join(packagesDir, "@mmmbuto", "codex-vl.json"), "utf8"),
    );
    if (metadata.name !== "@mmmbuto/codex-vl") {
      return false;
    }

    // Vite+ records the active global installation in packages/@mmmbuto/codex-vl.json.
    // Older installs have no ID or append a #-prefixed ID to the package name;
    // newer installs put the ID in a subdirectory of the package prefix.
    const installId = metadata.installId || "";
    const installDir = installId.startsWith("#")
      ? path.join(packagesDir, `@mmmbuto/codex-vl${installId}`)
      : path.join(packagesDir, "@mmmbuto/codex-vl", installId);
    for (const nodeModulesDir of [
      path.join(installDir, "lib", "node_modules"),
      path.join(installDir, "node_modules"),
    ]) {
      const packageRoot = path.join(nodeModulesDir, "@mmmbuto", "codex-vl");
      if (
        existsSync(packageRoot) &&
        realpathSync(packageRoot) === codexPackageRoot
      ) {
        return true;
      }
    }
  } catch {
    // Missing or unreadable ownership metadata must not prevent Codex starting.
  }
  return false;
}

/**
 * Use heuristics to detect the package manager that was used to install Codex
 * in order to give the user a hint about how to update it.
 */
function detectPackageManager() {
  // Package-manager ownership metadata can be several parents above the package.
  // Search ancestors of both the canonical package root and lexical entrypoint
  // because the package manager may link either path.
  const entrypointDir = path.dirname(path.resolve(process.argv[1]));
  for (const startDir of new Set([codexPackageRoot, entrypointDir])) {
    const filesystemRoot = path.parse(startDir).root;
    for (
      let currentDir = startDir;
      currentDir !== filesystemRoot;
      currentDir = path.dirname(currentDir)
    ) {
      if (isVitePlusOwnedCodexInstall(currentDir)) {
        return "vite-plus";
      }
      if (isPnpmOwnedCodexInstall(path.join(currentDir, "node_modules"))) {
        return "pnpm";
      }
    }

    if (isPnpmOwnedCodexInstall(path.join(filesystemRoot, "node_modules"))) {
      return "pnpm";
    }
  }

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
  return (process.env.PATH || "").split(path.delimiter).filter(Boolean);
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
if (existsSync(pathDir)) {
  additionalDirs.push(pathDir);
}
const updatedPath = getUpdatedPath(additionalDirs);

await maybeBootstrapInstallMode();

if (skipNativeExec) {
  process.exit(0);
}

const packageManager = detectPackageManager();
const packageManagerEnvVar =
  packageManager === "bun"
    ? "CODEX_MANAGED_BY_BUN"
    : packageManager === "pnpm"
      ? "CODEX_MANAGED_BY_PNPM"
      : packageManager === "vite-plus"
        ? "CODEX_MANAGED_BY_VITE_PLUS"
        : "CODEX_MANAGED_BY_NPM";
const env = {
  ...process.env,
  PATH: updatedPath,
  CODEX_MANAGED_PACKAGE_ROOT: codexPackageRoot,
};
delete env.CODEX_MANAGED_BY_NPM;
delete env.CODEX_MANAGED_BY_BUN;
delete env.CODEX_MANAGED_BY_PNPM;
delete env.CODEX_MANAGED_BY_VITE_PLUS;
env[packageManagerEnvVar] = "1";
if (platform === "android") {
  env.CODEX_SELF_EXE = binaryPath;
  env.LD_LIBRARY_PATH = sanitizeAndroidLdLibraryPath(path.dirname(binaryPath));
}

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  env,
});

child.on("error", (err) => {
  console.error(
    `Failed to execute Codex VL native binary at ${binaryPath}: ${err.message}`,
  );
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
