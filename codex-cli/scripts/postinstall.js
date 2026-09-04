import { existsSync as defaultExistsSync, promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DEFAULT_TERMUX_PREFIX = "/data/data/com.termux/files/usr";
const DEFAULT_BIN_DIR = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "bin",
);
const LAUNCHERS = ["codex.js", "codex-exec.js"];
const DEFAULT_SHEBANG = "#!/usr/bin/env node";
const TERMUX_ROOT = "/data/data/com.termux";

function isTermux(env, platform) {
  return Boolean(env.TERMUX_VERSION)
    || env.PREFIX === DEFAULT_TERMUX_PREFIX
    || platform === "android";
}

function validatedTermuxPrefix(rawPrefix, warn) {
  if (!rawPrefix) {
    return DEFAULT_TERMUX_PREFIX;
  }

  const normalized = typeof rawPrefix === "string"
    ? path.posix.normalize(rawPrefix)
    : "";
  const hasParentSegment = typeof rawPrefix === "string"
    && rawPrefix.split("/").includes("..");
  const isAllowed = typeof rawPrefix === "string"
    && path.posix.isAbsolute(rawPrefix)
    && !hasParentSegment
    && (normalized === TERMUX_ROOT || normalized.startsWith(`${TERMUX_ROOT}/`));

  if (!isAllowed) {
    warn("Warning: invalid Termux PREFIX; using the default Termux prefix.");
    return DEFAULT_TERMUX_PREFIX;
  }

  return normalized;
}

async function fixLauncherShebang(filePath, prefix, warn) {
  try {
    const contents = await fs.readFile(filePath, "utf8");
    const newlineIndex = contents.indexOf("\n");
    const firstLineEnd = newlineIndex === -1 ? contents.length : newlineIndex;
    const hasCarriageReturn = newlineIndex > 0 && contents[newlineIndex - 1] === "\r";
    const firstLine = contents.slice(0, hasCarriageReturn ? firstLineEnd - 1 : firstLineEnd);

    if (firstLine !== DEFAULT_SHEBANG) {
      return;
    }

    const replacement = `#!${prefix}/bin/env node`;
    const lineEnding = newlineIndex === -1 ? "" : hasCarriageReturn ? "\r\n" : "\n";
    const rest = newlineIndex === -1 ? "" : contents.slice(newlineIndex + 1);
    await fs.writeFile(filePath, `${replacement}${lineEnding}${rest}`);
  } catch (error) {
    warn(`Warning: unable to fix shebang in ${filePath}: ${error.message}`);
  }
}

export async function runPostinstall({
  binDir = DEFAULT_BIN_DIR,
  env = process.env,
  platform = process.platform,
  warn = console.warn,
  existsSync = defaultExistsSync,
} = {}) {
  if (!isTermux(env, platform)) {
    return;
  }

  const prefix = validatedTermuxPrefix(env.PREFIX, warn);
  const envPath = path.posix.join(prefix, "bin", "env");
  try {
    if (!existsSync(envPath)) {
      warn(`Warning: ${envPath} does not exist; launcher shebangs were left unchanged.`);
      return;
    }
  } catch (error) {
    warn(`Warning: unable to check ${envPath}; launcher shebangs were left unchanged: ${error.message}`);
    return;
  }

  for (const launcher of LAUNCHERS) {
    await fixLauncherShebang(path.join(binDir, launcher), prefix, warn);
  }
}

const scriptPath = fileURLToPath(import.meta.url);
if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  await runPostinstall();
}
