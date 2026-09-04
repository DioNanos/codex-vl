import { promises as fs } from "node:fs";
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

function isTermux(env, platform) {
  return Boolean(env.TERMUX_VERSION)
    || env.PREFIX === DEFAULT_TERMUX_PREFIX
    || platform === "android";
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
} = {}) {
  if (!isTermux(env, platform)) {
    return;
  }

  const prefix = env.PREFIX || DEFAULT_TERMUX_PREFIX;
  for (const launcher of LAUNCHERS) {
    await fixLauncherShebang(path.join(binDir, launcher), prefix, warn);
  }
}

const scriptPath = fileURLToPath(import.meta.url);
if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  await runPostinstall();
}
