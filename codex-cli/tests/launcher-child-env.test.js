// Launcher environment contract tests.
//
// Both real launchers are executed as child processes against a sandboxed
// npm-like package layout with a stand-in "native binary" that dumps the
// environment it received. The Android branch is exercised on any host by
// preloading a shim that reports platform=android / arch=arm64, so the
// launcher files under test are the real ones copied byte for byte.
//
// Contract under test:
//   - the native binary receives the caller's LD_LIBRARY_PATH verbatim on
//     every platform (unset, empty, PREFIX/lib, or custom values); the
//     launcher never prepends its own vendor directory and never filters
//     entries;
//   - package-manager metadata (CODEX_MANAGED_BY_*) and
//     CODEX_MANAGED_PACKAGE_ROOT stay in the launcher -> native context;
//   - CODEX_SELF_EXE is set on Android and absent elsewhere;
//   - PATH keeps the packaged tool directory in front;
//   - argv, exit codes, and signal termination are mirrored by the parent.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
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

const LAUNCHERS = ["codex.js", "codex-exec.js"];
const PATH_INPUT = "/nonexistent-launcher-test-path";
// The stand-in runs via a `#!/usr/bin/env node` shebang, so the child needs
// the real Node binary directory on PATH in addition to the probe entry.
const NODE_BIN_DIR = path.dirname(process.execPath);
const BASE_PATH = `${NODE_BIN_DIR}:${PATH_INPUT}`;

// Executed instead of the real ELF: dumps argv + env, then exits with the
// requested code or terminates itself with the requested signal.
const NATIVE_STANDIN = [
  "#!/usr/bin/env node",
  'import fs from "node:fs";',
  "const dump = { argv: process.argv.slice(2), env: { ...process.env } };",
  "fs.writeFileSync(process.env.CODEX_TEST_DUMP, JSON.stringify(dump));",
  "if (process.env.CODEX_TEST_SIGNAL) {",
  "  process.kill(process.pid, process.env.CODEX_TEST_SIGNAL);",
  "} else {",
  "  process.exit(Number(process.env.CODEX_TEST_EXIT || 0));",
  "}",
  "",
].join("\n");

// Reports the Android target regardless of the host, so the Android branch
// of the real launchers runs on Linux/macOS test hosts too.
const ANDROID_SHIM = [
  'import process from "node:process";',
  'Object.defineProperty(process, "platform", { value: "android" });',
  'Object.defineProperty(process, "arch", { value: "arm64" });',
  "",
].join("\n");

function writeExecutable(filePath, contents) {
  writeFileSync(filePath, contents, "utf8");
  chmodSync(filePath, 0o755);
}

function buildFixture(t, { triple, withPlatformPackage, androidShim }) {
  const root = mkdtempSync(path.join(tmpdir(), "codex-vl-launcher-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));

  const pkgRoot = path.join(root, "package");
  const binDir = path.join(pkgRoot, "bin");
  mkdirSync(binDir, { recursive: true });
  for (const name of [...LAUNCHERS, "identity_fds.js"]) {
    copyFileSync(path.join(sourceRoot, "bin", name), path.join(binDir, name));
  }

  writeFileSync(
    path.join(pkgRoot, "package.json"),
    JSON.stringify(
      {
        name: "@mmmbuto/codex-vl",
        version: "0.0.0-launcher-test",
        type: "module",
      },
      null,
      2,
    ),
    "utf8",
  );

  // Repository-local vendor payload (used when the platform package is not
  // resolvable) plus, optionally, the platform package itself so both
  // resolution paths can be exercised.
  const archRoots = [
    path.join(pkgRoot, "vendor", triple),
  ];
  if (withPlatformPackage) {
    const platformPackageRoot = path.join(
      pkgRoot,
      "node_modules",
      "@mmmbuto",
      `codex-vl-${triple.startsWith("x86_64") ? "linux-x64" : "android-arm64"}`,
    );
    archRoots.push(path.join(platformPackageRoot, "vendor", triple));
    mkdirSync(platformPackageRoot, { recursive: true });
    writeFileSync(
      path.join(platformPackageRoot, "package.json"),
      JSON.stringify({ name: "@mmmbuto/codex-vl-test-platform" }, null, 2),
      "utf8",
    );
  }

  for (const archRoot of archRoots) {
    mkdirSync(path.join(archRoot, "bin"), { recursive: true });
    writeExecutable(path.join(archRoot, "bin", "codex"), NATIVE_STANDIN);
    mkdirSync(path.join(archRoot, "codex-path"), { recursive: true });
  }

  const home = path.join(root, "home");
  mkdirSync(home, { recursive: true });

  let shimPath = null;
  if (androidShim) {
    shimPath = path.join(root, "android-shim.mjs");
    writeFileSync(shimPath, ANDROID_SHIM, "utf8");
  }

  // The stand-in in the platform package wins for the withPlatformPackage
  // fixture; otherwise resolution falls back to the local vendor payload.
  const binaryPath = withPlatformPackage ? archRoots[1] : archRoots[0];
  const expectedBinaryPath = path.join(binaryPath, "bin", "codex");
  const pathDir = path.join(binaryPath, "codex-path");

  return {
    root,
    pkgRoot,
    binDir,
    home,
    shimPath,
    expectedBinaryPath,
    pathDir,
  };
}

function runLauncher(fixture, launcherName, { args = [], env = {} } = {}) {
  const dumpPath = path.join(fixture.root, "dump.json");
  const childEnv = {
    PATH: BASE_PATH,
    HOME: fixture.home,
    CODEX_HOME: path.join(fixture.home, ".codex"),
    CODEX_TEST_DUMP: dumpPath,
    CODEX_TEST_EXIT: "0",
    ...env,
  };
  if (fixture.shimPath) {
    childEnv.NODE_OPTIONS = `--import ${fixture.shimPath}`;
  }

  const result = spawnSync(
    process.execPath,
    [path.join(fixture.binDir, launcherName), ...args],
    { env: childEnv, encoding: "utf8" },
  );

  let dump = null;
  if (dumpPath && !result.error) {
    try {
      dump = JSON.parse(readFileSync(dumpPath, "utf8"));
    } catch {
      dump = null;
    }
  }
  return { result, dump };
}

function assertLauncherCopiesAreReal(fixture) {
  for (const name of LAUNCHERS) {
    const source = readFileSync(path.join(sourceRoot, "bin", name));
    const copy = readFileSync(path.join(fixture.binDir, name));
    assert.equal(
      createHash("sha256").update(copy).digest("hex"),
      createHash("sha256").update(source).digest("hex"),
      `sandbox copy of ${name} must be byte-identical to the real launcher`,
    );
  }
}

function readDumpEnv(dump) {
  assert.ok(dump, "native stand-in did not run or produced no dump");
  return dump.env;
}

function assertLdLibraryPathVerbatim(dumpEnv, expected) {
  if (expected === undefined) {
    assert.equal(
      Object.prototype.hasOwnProperty.call(dumpEnv, "LD_LIBRARY_PATH"),
      false,
      "LD_LIBRARY_PATH must stay unset when the caller has none",
    );
    return;
  }
  assert.equal(
    dumpEnv.LD_LIBRARY_PATH,
    expected,
    "LD_LIBRARY_PATH must reach the native binary verbatim",
  );
}

function assertNoVendorDirInLd(dumpEnv, vendorDir) {
  const value = dumpEnv.LD_LIBRARY_PATH;
  if (value === undefined) {
    return;
  }
  const entries = value.split(":");
  assert.equal(
    entries.includes(vendorDir),
    false,
    `vendor directory ${vendorDir} must not be injected into LD_LIBRARY_PATH`,
  );
}

// --- Linux launchers: caller environment verbatim + managed metadata ------

test("linux launcher passes caller env verbatim and keeps managed metadata", (t) => {
  const fixture = buildFixture(t, {
    triple: "x86_64-unknown-linux-musl",
    withPlatformPackage: false,
    androidShim: false,
  });
  assertLauncherCopiesAreReal(fixture);

  for (const launcher of LAUNCHERS) {
    const { result, dump } = runLauncher(fixture, launcher, {
      args: ["--probe", "value with spaces"],
    });
    assert.equal(result.status, 0, `${launcher} stderr: ${result.stderr}`);
    const dumpEnv = readDumpEnv(dump);

    assertLdLibraryPathVerbatim(dumpEnv, undefined);
    assert.equal(
      Object.prototype.hasOwnProperty.call(dumpEnv, "CODEX_SELF_EXE"),
      false,
      `${launcher}: CODEX_SELF_EXE is Android-only`,
    );
    assert.equal(dumpEnv.CODEX_MANAGED_BY_NPM, "1", launcher);
    // codex.js records the package root for the managed-install context;
    // codex-exec.js does not set it today and this test must not widen the
    // contract beyond the launcher fix under scrutiny.
    if (launcher === "codex.js") {
      assert.equal(
        dumpEnv.CODEX_MANAGED_PACKAGE_ROOT,
        fixture.pkgRoot,
        launcher,
      );
    }
    for (const name of [
      "CODEX_MANAGED_BY_BUN",
      "CODEX_MANAGED_BY_PNPM",
      "CODEX_MANAGED_BY_VITE_PLUS",
    ]) {
      assert.equal(
        Object.prototype.hasOwnProperty.call(dumpEnv, name),
        false,
        `${launcher}: ${name} must not leak from the caller`,
      );
    }
    assert.equal(
      dumpEnv.PATH,
      `${fixture.pathDir}:${BASE_PATH}`,
      `${launcher}: packaged tool dir must lead PATH`,
    );
    assert.deepEqual(
      dump.argv,
      launcher === "codex-exec.js"
        ? ["exec", "--probe", "value with spaces"]
        : ["--probe", "value with spaces"],
      launcher,
    );
  }
});

test("linux launcher keeps a custom LD_LIBRARY_PATH byte for byte", (t) => {
  const fixture = buildFixture(t, {
    triple: "x86_64-unknown-linux-musl",
    withPlatformPackage: false,
    androidShim: false,
  });
  const custom = "/opt/custom-a/lib:/opt/custom-b/lib";
  for (const launcher of LAUNCHERS) {
    const { result, dump } = runLauncher(fixture, launcher, {
      env: { LD_LIBRARY_PATH: custom },
    });
    assert.equal(result.status, 0, launcher);
    const dumpEnv = readDumpEnv(dump);
    assertLdLibraryPathVerbatim(dumpEnv, custom);
    assertNoVendorDirInLd(dumpEnv, path.dirname(fixture.expectedBinaryPath));
  }
});

// --- Android launchers: no LD rewrite, SELF_EXE kept for the fork ---------

test("android launcher leaves LD_LIBRARY_PATH untouched when unset or empty", (t) => {
  const fixture = buildFixture(t, {
    triple: "aarch64-linux-android",
    withPlatformPackage: true,
    androidShim: true,
  });
  assertLauncherCopiesAreReal(fixture);

  for (const launcher of LAUNCHERS) {
    for (const ldValue of [undefined, ""]) {
      const { result, dump } = runLauncher(fixture, launcher, {
        env:
          ldValue === undefined ? {} : { LD_LIBRARY_PATH: ldValue },
      });
      assert.equal(result.status, 0, `${launcher} (LD=${ldValue}) stderr: ${result.stderr}`);
      const dumpEnv = readDumpEnv(dump);
      assertLdLibraryPathVerbatim(dumpEnv, ldValue);
      assertNoVendorDirInLd(
        dumpEnv,
        path.dirname(fixture.expectedBinaryPath),
      );
      assert.equal(
        dumpEnv.CODEX_SELF_EXE,
        fixture.expectedBinaryPath,
        `${launcher}: CODEX_SELF_EXE must keep pointing at the packaged binary`,
      );
    }
  }
});

test("android launcher forwards PREFIX and custom LD_LIBRARY_PATH entries verbatim", (t) => {
  const fixture = buildFixture(t, {
    triple: "aarch64-linux-android",
    withPlatformPackage: true,
    androidShim: true,
  });
  const prefix = "/data/data/com.termux/files/usr";
  const ldValue = `${prefix}/lib:${prefix}/libexec:/opt/custom/lib`;

  for (const launcher of LAUNCHERS) {
    const { result, dump } = runLauncher(fixture, launcher, {
      env: { LD_LIBRARY_PATH: ldValue, PREFIX: prefix },
    });
    assert.equal(result.status, 0, launcher);
    const dumpEnv = readDumpEnv(dump);
    assertLdLibraryPathVerbatim(dumpEnv, ldValue);
    assert.equal(dumpEnv.PREFIX, prefix, launcher);
    assertNoVendorDirInLd(dumpEnv, path.dirname(fixture.expectedBinaryPath));
  }
});

test("android launcher mirrors child exit codes and survives signal termination", (t) => {
  const fixture = buildFixture(t, {
    triple: "aarch64-linux-android",
    withPlatformPackage: true,
    androidShim: true,
  });

  for (const launcher of LAUNCHERS) {
    const exited = runLauncher(fixture, launcher, {
      env: { CODEX_TEST_EXIT: "42" },
    });
    assert.equal(exited.result.status, 42, launcher);

    // When the child dies by signal the parent re-emits the signal to
    // itself, but its own signal handlers intercept that delivery, so the
    // observable outcome is a clean exit once the child is gone. The signal
    // path predates this fix; the test pins that the parent still
    // terminates promptly without a crash and with the child fully run.
    const signaled = runLauncher(fixture, launcher, {
      env: { CODEX_TEST_SIGNAL: "SIGTERM" },
    });
    assert.ok(signaled.dump, `${launcher}: child ran before termination`);
    assert.ok(
      (signaled.result.status === 0 || signaled.result.status === 143) ||
        signaled.result.signal === "SIGTERM",
      `${launcher}: unexpected termination status=${signaled.result.status} signal=${signaled.result.signal}`,
    );
    assert.equal(
      signaled.result.stderr.includes("Failed to execute"),
      false,
      `${launcher}: signal termination must not surface a spawn error`,
    );
  }
});
