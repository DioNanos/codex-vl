import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// Termux has no /usr/bin/env. npm creates a global bin link to these files,
// but the kernel processes their shebang before Node can run the launcher.
// Rewrite only the installed copy to the Node interpreter active for npm.
if (process.platform !== "android") {
  process.exit(0);
}

if (!path.isAbsolute(process.execPath)) {
  throw new Error(`Node executable path is not absolute: ${process.execPath}`);
}

const packageRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const launcherPaths = ["bin/codex.js", "bin/codex-exec.js"];

for (const relativePath of launcherPaths) {
  const launcherPath = path.join(packageRoot, relativePath);
  if (!existsSync(launcherPath)) {
    continue;
  }

  const source = readFileSync(launcherPath, "utf8");
  const firstLineEnd = source.indexOf("\n");
  const firstLine = firstLineEnd === -1 ? source : source.slice(0, firstLineEnd);
  if (!firstLine.startsWith("#!")) {
    throw new Error(`Launcher has no shebang: ${launcherPath}`);
  }

  const replacement = `#!${process.execPath}`;
  if (firstLine !== replacement) {
    writeFileSync(
      launcherPath,
      `${replacement}${source.slice(firstLine.length)}`,
      "utf8",
    );
  }
}
