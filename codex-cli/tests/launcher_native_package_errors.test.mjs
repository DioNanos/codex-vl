import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const sourceRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const launchers = ["codex.js", "codex-exec.js"];
const platformPackage = "@mmmbuto/codex-vl-darwin-arm64";
const targetTriple = "aarch64-apple-darwin";
const postinstallSource = readFileSync(
  path.join(sourceRoot, "scripts", "postinstall_darwin_build.js"),
  "utf8",
);
const termuxPostinstallSource = readFileSync(
  path.join(sourceRoot, "scripts", "postinstall_termux_launcher.js"),
  "utf8",
);

test("Termux postinstall rewrites both npm launcher shebangs", () => {
  assert.match(termuxPostinstallSource, /Termux has no \/usr\/bin\/env/);
  assert.match(termuxPostinstallSource, /process\.platform !== "android"/);
  assert.match(termuxPostinstallSource, /bin\/codex\.js/);
  assert.match(termuxPostinstallSource, /bin\/codex-exec\.js/);
  assert.deepEqual(
    JSON.parse(readFileSync(path.join(sourceRoot, "package.json"), "utf8"))
      .files,
    [
      "bin/codex.js",
      "bin/codex-exec.js",
      "scripts/postinstall_termux_launcher.js",
    ],
  );
});

function createFixture(t, { platformPackageInstalled = false, binaryMode }) {
  const root = mkdtempSync(path.join(tmpdir(), "codex-vl-launcher-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));

  const coordinatorRoot = path.join(root, "coordinator");
  const binRoot = path.join(coordinatorRoot, "bin");
  mkdirSync(binRoot, { recursive: true });
  writeFileSync(
    path.join(coordinatorRoot, "package.json"),
    JSON.stringify({ name: "@mmmbuto/codex-vl", type: "module" }),
  );

  const forcePlatformPath = path.join(root, "force-platform.cjs");
  writeFileSync(
    forcePlatformPath,
    [
      'Object.defineProperty(process, "platform", { value: "darwin" });',
      'Object.defineProperty(process, "arch", { value: "arm64" });',
      "",
    ].join("\n"),
  );

  const platformRoot = path.join(
    coordinatorRoot,
    "node_modules",
    "@mmmbuto",
    "codex-vl-darwin-arm64",
  );
  if (platformPackageInstalled) {
    mkdirSync(platformRoot, { recursive: true });
    writeFileSync(
      path.join(platformRoot, "package.json"),
      JSON.stringify({ name: platformPackage, version: "0.0.0-test" }),
    );
  }

  if (binaryMode) {
    const binaryPath = path.join(
      platformRoot,
      "vendor",
      targetTriple,
      "codex",
      "codex",
    );
    mkdirSync(path.dirname(binaryPath), { recursive: true });
    writeFileSync(binaryPath, "#!/bin/sh\nexit 0\n");
    chmodSync(binaryPath, binaryMode === "executable" ? 0o755 : 0o644);
  }

  return { root, coordinatorRoot, binRoot, forcePlatformPath };
}

function runLauncher(fixture, launcherName) {
  const launcherPath = path.join(fixture.binRoot, launcherName);
  copyFileSync(path.join(sourceRoot, "bin", launcherName), launcherPath);

  return spawnSync(
    process.execPath,
    ["--require", fixture.forcePlatformPath, launcherPath, "--version"],
    {
      encoding: "utf8",
      env: {
        ...process.env,
        CODEX_HOME: path.join(fixture.root, "codex-home"),
        CODEX_VL_BOOTSTRAP: "skip",
        npm_config_user_agent: "npm/12.0.1 node/v26.4.0 darwin arm64",
      },
    },
  );
}

test("Darwin postinstall prints the complete npm 12 recovery command", () => {
  assert.match(
    postinstallSource,
    /npm install -g @mmmbuto\/codex-vl@latest --allow-scripts=@mmmbuto\/codex-vl --foreground-scripts/,
  );
  assert.doesNotMatch(
    postinstallSource,
    /npm install -g --allow-scripts=@mmmbuto\/codex-vl/,
  );
});

for (const launcherName of launchers) {
  test(`${launcherName}: reports an omitted Darwin platform package`, (t) => {
    const fixture = createFixture(t, {});
    const result = runLauncher(fixture, launcherName);

    assert.notEqual(result.status, 0);
    assert.match(
      result.stderr,
      /Darwin platform package @mmmbuto\/codex-vl-darwin-arm64 is not installed\./,
    );
    assert.match(
      result.stderr,
      /npm install -g @mmmbuto\/codex-vl@latest --allow-scripts=@mmmbuto\/codex-vl --foreground-scripts/,
    );
  });

  test(`${launcherName}: reports a Darwin postinstall with no binary`, (t) => {
    const fixture = createFixture(t, { platformPackageInstalled: true });
    const result = runLauncher(fixture, launcherName);

    assert.notEqual(result.status, 0);
    assert.match(
      result.stderr,
      /is installed at .*but its native binary is missing\./,
    );
    assert.match(
      result.stderr,
      /The macOS postinstall did not run or did not produce the native binary\./,
    );
  });

  test(`${launcherName}: launches a produced Darwin binary`, (t) => {
    const fixture = createFixture(t, {
      platformPackageInstalled: true,
      binaryMode: "executable",
    });
    const result = runLauncher(fixture, launcherName);

    assert.equal(result.status, 0, result.stderr);
  });

  test(`${launcherName}: names a Darwin binary that cannot execute`, (t) => {
    const fixture = createFixture(t, {
      platformPackageInstalled: true,
      binaryMode: "not-executable",
    });
    const result = runLauncher(fixture, launcherName);

    assert.notEqual(result.status, 0);
    assert.match(
      result.stderr,
      /Failed to execute Codex VL native binary at .*codex: (?:spawn .* )?EACCES/,
    );
  });
}
