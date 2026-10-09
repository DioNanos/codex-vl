// The macOS postinstall ships inside a package whose package.json declares no
// "type", so it is CommonJS - while codex-cli/package.json here is
// "type": "module". Copy the script next to a CommonJS package.json to load it
// as a library. The network and the platform are injected: these tests never
// build anything.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import test, { before } from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
);
const scriptSource = path.join(
  repoRoot,
  "codex-cli",
  "scripts",
  "postinstall_darwin_build.js",
);

const scratch = mkdtempSync(path.join(tmpdir(), "codex-vl-rg-test-"));
process.on("exit", () => rmSync(scratch, { recursive: true, force: true }));

const scriptCopy = path.join(scratch, "postinstall_darwin_build.js");
copyFileSync(scriptSource, scriptCopy);
writeFileSync(path.join(scratch, "package.json"), '{"type":"commonjs"}\n');

const RG_MEMBER = "ripgrep-15.2.0-aarch64-apple-darwin/rg";
const RG_URL =
  "https://example.invalid/ripgrep-15.2.0-aarch64-apple-darwin.tar.gz";
const quiet = { log: () => {}, warn: () => {} };

let postinstall;

before(() => {
  // Older revisions run the whole build at module scope - and exit on a
  // non-darwin host, which would silently kill this runner. Probe the exported
  // API in a child process so a regression is a readable failure instead.
  const probe = spawnSync(
    process.execPath,
    [
      "-e",
      `const m = require(${JSON.stringify(scriptCopy)}); process.stdout.write(Object.keys(m).join(","));`,
    ],
    { encoding: "utf8" },
  );
  for (const name of [
    "provisionRipgrep",
    "verifyPackageLayout",
    "packageLayoutFailure",
    "readRgManifest",
    "findCodesign",
    "codesignAdhoc",
  ]) {
    assert.match(
      probe.stdout || "",
      new RegExp(name),
      `postinstall_darwin_build.js must export ${name}, and must not run the ` +
        `build when required as a library; stdout=${JSON.stringify(probe.stdout)}`,
    );
  }
  postinstall = createRequire(import.meta.url)(scriptCopy);
});

function manifestEntry(overrides = {}) {
  return {
    size: 1764284,
    hash: "sha256",
    digest: "3".repeat(64),
    format: "tar.gz",
    path: RG_MEMBER,
    providers: [{ url: RG_URL }],
    ...overrides,
  };
}

function writeManifest(dir, platforms, name = "rg-manifest") {
  const manifestPath = path.join(dir, name);
  writeFileSync(
    manifestPath,
    `#!/usr/bin/env dotslash\n${JSON.stringify({ name: "rg", platforms }, null, 2)}\n`,
  );
  return manifestPath;
}

// A real tar.gz fixture: extraction runs for real, only the download is faked.
function ripgrepFixture({ digest, size } = {}) {
  const dir = mkdtempSync(path.join(scratch, "fixture-"));
  const payload = path.join(dir, "payload");
  mkdirSync(path.join(payload, path.dirname(RG_MEMBER)), { recursive: true });
  writeFileSync(
    path.join(payload, RG_MEMBER),
    "#!/bin/sh\necho 'ripgrep 15.2.0 (aarch64-apple-darwin)'\n",
  );
  const archive = path.join(dir, "ripgrep.tar.gz");
  execFileSync("tar", ["-czf", archive, "-C", payload, path.dirname(RG_MEMBER)]);
  const bytes = readFileSync(archive);
  return {
    dir,
    archive,
    manifestPath: writeManifest(dir, {
      "macos-aarch64": manifestEntry({
        size: size ?? bytes.length,
        digest: digest ?? createHash("sha256").update(bytes).digest("hex"),
      }),
    }),
  };
}

function emptyPackageDir() {
  return mkdtempSync(path.join(scratch, "package-"));
}

// A layout the daemon accepts: the regular files it validates, the programs
// executable. The packaged voice runtime is part of that layout: without it the
// CLI ships without /voice and the install must fail instead of looking healthy.
function completePackageDir() {
  const packageDir = emptyPackageDir();
  mkdirSync(path.join(packageDir, "bin"), { recursive: true });
  mkdirSync(path.join(packageDir, "codex-path"), { recursive: true });
  mkdirSync(path.join(packageDir, "codex-resources", "voice", "bin"), {
    recursive: true,
  });
  mkdirSync(path.join(packageDir, "codex-resources", "voice", "lib"), {
    recursive: true,
  });
  writeFileSync(path.join(packageDir, "codex-package.json"), "{}");
  for (const relative of [
    "bin/codex",
    "bin/codex-code-mode-host",
    "codex-path/rg",
    "codex-resources/voice/bin/codex-voice-host",
  ]) {
    writeFileSync(path.join(packageDir, relative), "#!/bin/sh\n");
    chmodSync(path.join(packageDir, relative), 0o755);
  }
  writeFileSync(
    path.join(packageDir, "codex-resources", "voice", "lib", "libgstreamer-1.0.0.dylib"),
    "fixture",
  );
  return packageDir;
}

// --- pinned download --------------------------------------------------------

test("A: from the DotSlash manifest, rg lands in codex-path/ executable", () => {
  const fixture = ripgrepFixture();
  const packageDir = emptyPackageDir();
  const requested = [];
  const dest = postinstall.provisionRipgrep({
    packageDir,
    manifestPath: fixture.manifestPath,
    download: (url, to) => {
      requested.push(url);
      copyFileSync(fixture.archive, to);
    },
    ...quiet,
  });
  assert.deepEqual(requested, [RG_URL]);
  assert.equal(dest, path.join(packageDir, "codex-path", "rg"));
  assert.match(readFileSync(dest, "utf8"), /ripgrep 15\.2\.0/);
  assert.equal(statSync(dest).mode & 0o111, 0o111, "rg must be executable");
});

test("A: a wrong sha256 is an error and leaves no rg behind", () => {
  const fixture = ripgrepFixture({ digest: "0".repeat(64) });
  const packageDir = emptyPackageDir();
  assert.throws(
    () =>
      postinstall.provisionRipgrep({
        packageDir,
        manifestPath: fixture.manifestPath,
        download: (_url, to) => copyFileSync(fixture.archive, to),
        ...quiet,
      }),
    /checksum mismatch/,
  );
  assert.equal(existsSync(path.join(packageDir, "codex-path", "rg")), false);
});

test("A: a size mismatch is an error", () => {
  const fixture = ripgrepFixture({ size: 12 });
  const packageDir = emptyPackageDir();
  assert.throws(
    () =>
      postinstall.provisionRipgrep({
        packageDir,
        manifestPath: fixture.manifestPath,
        download: (_url, to) => copyFileSync(fixture.archive, to),
        ...quiet,
      }),
    /size mismatch/,
  );
});

// --- R1: integrity metadata is mandatory, never silently skipped -------------

for (const [label, entry] of [
  ["without size", manifestEntry({ size: undefined })],
  ["without digest", manifestEntry({ digest: undefined })],
  ["with size 0", manifestEntry({ size: 0 })],
  ["with a non-numeric size", manifestEntry({ size: "1764284" })],
  ["with a digest that is not a sha256", manifestEntry({ digest: "abc" })],
]) {
  test(`R1: a manifest entry ${label} is an error, and no fallback hides it`, () => {
    const packageDir = emptyPackageDir();
    const manifestPath = writeManifest(packageDir, { "macos-aarch64": entry });
    let downloaded = false;
    assert.throws(
      () =>
        postinstall.provisionRipgrep({
          packageDir,
          manifestPath,
          download: () => {
            downloaded = true;
          },
          findSystemRg: () => "/usr/bin/rg",
          ...quiet,
        }),
      /ripgrep manifest/,
    );
    assert.equal(downloaded, false, "an invalid entry must not reach the network");
  });
}

test("R1: without a provider URL the entry is an error", () => {
  const packageDir = emptyPackageDir();
  const manifestPath = writeManifest(packageDir, {
    "macos-aarch64": manifestEntry({ providers: [] }),
  });
  assert.throws(
    () =>
      postinstall.provisionRipgrep({
        packageDir,
        manifestPath,
        findSystemRg: () => "/usr/bin/rg",
        ...quiet,
      }),
    /ripgrep manifest/,
  );
});

// --- R3: the fallback is for a failed download, not for bad metadata --------

test("R3: a network failure falls back to the system rg, with a warning", () => {
  const fixture = ripgrepFixture();
  const packageDir = emptyPackageDir();
  const systemRg = path.join(packageDir, "system-rg");
  writeFileSync(systemRg, "#!/bin/sh\necho 'ripgrep 14.0.0'\n");
  const warnings = [];
  const dest = postinstall.provisionRipgrep({
    packageDir,
    manifestPath: fixture.manifestPath,
    download: () => {
      throw new Error("curl: (6) Could not resolve host");
    },
    findSystemRg: () => systemRg,
    log: () => {},
    warn: (message) => warnings.push(message),
  });
  assert.equal(readFileSync(dest, "utf8"), readFileSync(systemRg, "utf8"));
  assert.equal(warnings.length, 1, `unexpected warnings: ${JSON.stringify(warnings)}`);
  assert.match(warnings[0], /download failed/);
  assert.match(warnings[0], /system ripgrep/);
});

test("R3: a network failure without a system rg is a clear error", () => {
  const fixture = ripgrepFixture();
  const packageDir = emptyPackageDir();
  assert.throws(
    () =>
      postinstall.provisionRipgrep({
        packageDir,
        manifestPath: fixture.manifestPath,
        download: () => {
          throw new Error("curl: (6) Could not resolve host");
        },
        findSystemRg: () => null,
        ...quiet,
      }),
    /no ripgrep available/,
  );
});

test("R3: a missing manifest is an error, never a fallback", () => {
  const packageDir = emptyPackageDir();
  let fellBack = false;
  assert.throws(
    () =>
      postinstall.provisionRipgrep({
        packageDir,
        manifestPath: path.join(packageDir, "no-such-manifest"),
        findSystemRg: () => {
          fellBack = true;
          return "/usr/bin/rg";
        },
        ...quiet,
      }),
    /ripgrep manifest/,
  );
  assert.equal(fellBack, false);
});

test("R3: broken JSON is an error, never a fallback", () => {
  const packageDir = emptyPackageDir();
  const manifestPath = path.join(packageDir, "rg-manifest");
  writeFileSync(manifestPath, "#!/usr/bin/env dotslash\n{ not json\n");
  assert.throws(
    () =>
      postinstall.provisionRipgrep({
        packageDir,
        manifestPath,
        findSystemRg: () => "/usr/bin/rg",
        ...quiet,
      }),
    /ripgrep manifest/,
  );
});

test("R3: a manifest without the macos-aarch64 entry is an error, and never downloads", () => {
  const packageDir = emptyPackageDir();
  const manifestPath = writeManifest(packageDir, {});
  let downloaded = false;
  assert.throws(
    () =>
      postinstall.provisionRipgrep({
        packageDir,
        manifestPath,
        download: () => {
          downloaded = true;
        },
        findSystemRg: () => "/usr/bin/rg",
        ...quiet,
      }),
    /ripgrep manifest/,
  );
  assert.equal(downloaded, false);
});

// --- R2: the guard is the daemon's predicate, not just exists ---------------

test("R2: the guard lists exactly what the daemon refuses", () => {
  const packageDir = emptyPackageDir();
  mkdirSync(path.join(packageDir, "bin"), { recursive: true });
  writeFileSync(path.join(packageDir, "bin", "codex"), "#!/bin/sh\n");
  chmodSync(path.join(packageDir, "bin", "codex"), 0o755);
  writeFileSync(path.join(packageDir, "codex-package.json"), "{}");
  assert.deepEqual(postinstall.verifyPackageLayout({ packageDir }), [
    "bin/codex-code-mode-host",
    "codex-path/rg",
    "codex-resources/voice/bin/codex-voice-host",
    "codex-resources/voice/lib/libgstreamer-1.0.0.dylib",
  ]);

  assert.deepEqual(postinstall.verifyPackageLayout({ packageDir: completePackageDir() }), []);
});

test("R2: a directory where a file is expected is not a package", () => {
  const packageDir = completePackageDir();
  rmSync(path.join(packageDir, "bin", "codex"), { force: true });
  mkdirSync(path.join(packageDir, "bin", "codex"), { recursive: true });
  assert.deepEqual(postinstall.verifyPackageLayout({ packageDir }), ["bin/codex"]);
});

test("R2: a program without the executable bit is refused, like the daemon", () => {
  const packageDir = completePackageDir();
  chmodSync(path.join(packageDir, "codex-path", "rg"), 0o644);
  assert.deepEqual(postinstall.verifyPackageLayout({ packageDir }), [
    "codex-path/rg",
  ]);

  // codex-package.json is a manifest, not a program: no executable bit needed.
  const onlyManifest = completePackageDir();
  chmodSync(path.join(onlyManifest, "codex-package.json"), 0o644);
  assert.deepEqual(postinstall.verifyPackageLayout({ packageDir: onlyManifest }), []);
});

test("R1: a provisioning error fails the package even when every path exists", () => {
  const packageDir = completePackageDir();
  assert.equal(
    postinstall.packageLayoutFailure({ packageDir, provisionError: null }),
    null,
  );
  const failure = postinstall.packageLayoutFailure({
    packageDir,
    provisionError: new Error("ripgrep checksum mismatch: expected 00, got ff"),
  });
  assert.notEqual(failure, null, "a checksum mismatch must never end in success");
  assert.match(failure, /checksum mismatch/);

  const incomplete = postinstall.packageLayoutFailure({
    packageDir: emptyPackageDir(),
    provisionError: null,
  });
  assert.match(incomplete, /incomplete macOS package layout/);
});

// --- ad-hoc signing ---------------------------------------------------------

test("codesign detection does not ask it for --version", () => {
  const bindir = mkdtempSync(path.join(scratch, "bin-"));
  const fake = path.join(bindir, "codesign");
  writeFileSync(
    fake,
    '#!/bin/sh\nif [ "$1" = "--version" ]; then exit 2; fi\nexit 0\n',
  );
  chmodSync(fake, 0o755);
  const env = { ...process.env, PATH: `${bindir}:${process.env.PATH ?? ""}` };
  assert.equal(postinstall.findCodesign({ env }), fake);
  assert.equal(
    postinstall.findCodesign({ env: { ...process.env, PATH: "/nonexistent" } }),
    null,
  );
});

test("the ad-hoc signature is codesign -s - -f, then verified with -v", () => {
  const calls = [];
  const warnings = [];
  const signed = postinstall.codesignAdhoc("/tmp/rg-target", {
    platform: "darwin",
    find: () => "/usr/bin/codesign",
    run: (command, args) => {
      calls.push([command, ...args]);
      return { status: 0 };
    },
    log: () => {},
    warn: (message) => warnings.push(message),
  });
  assert.equal(signed, true);
  assert.deepEqual(calls, [
    ["/usr/bin/codesign", "-s", "-", "-f", "/tmp/rg-target"],
    ["/usr/bin/codesign", "-v", "/tmp/rg-target"],
  ]);
  assert.deepEqual(warnings, []);
});

test("macOS without codesign: an explicit warning, no signature", () => {
  const warnings = [];
  const signed = postinstall.codesignAdhoc("/tmp/rg-target", {
    platform: "darwin",
    find: () => null,
    run: () => ({ status: 0 }),
    log: () => {},
    warn: (message) => warnings.push(message),
  });
  assert.equal(signed, false);
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /codesign not found/i);
});

test("a failed or unverified signature warns, and never stays silent", () => {
  const failed = [];
  assert.equal(
    postinstall.codesignAdhoc("/tmp/rg-target", {
      platform: "darwin",
      find: () => "/usr/bin/codesign",
      run: () => ({ status: 1 }),
      log: () => {},
      warn: (message) => failed.push(message),
    }),
    false,
  );
  assert.match(failed.join(" "), /codesign failed/i);

  const unverified = [];
  let call = 0;
  assert.equal(
    postinstall.codesignAdhoc("/tmp/rg-target", {
      platform: "darwin",
      find: () => "/usr/bin/codesign",
      run: () => ({ status: call++ === 0 ? 0 : 1 }),
      log: () => {},
      warn: (message) => unverified.push(message),
    }),
    false,
  );
  assert.match(unverified.join(" "), /verification failed/i);
});

test("outside macOS the signature is a silent no-op", () => {
  const warnings = [];
  assert.equal(
    postinstall.codesignAdhoc("/tmp/rg-target", {
      platform: "linux",
      find: () => "/usr/bin/codesign",
      run: () => ({ status: 0 }),
      log: () => {},
      warn: (message) => warnings.push(message),
    }),
    false,
  );
  assert.deepEqual(warnings, []);
});

// --- R5: the download must not leave the install hanging ---------------------
// A slow network must not stall the install: curl runs with a bounded connect
// time, a bounded total time and retries, and once those are exhausted the
// declared fallback takes over.
test("R5: curl runs with retries and timeouts, and a timeout ends in the fallback", () => {
  const bindir = mkdtempSync(path.join(scratch, "curl-"));
  const argvFile = path.join(bindir, "argv.txt");
  const fakeCurl = path.join(bindir, "curl");
  writeFileSync(
    fakeCurl,
    `#!/bin/sh\nprintf '%s\\n' "$@" > ${JSON.stringify(argvFile)}\n` +
      'echo "curl: (28) Operation timed out after 300001 milliseconds" >&2\n' +
      "exit 28\n",
  );
  chmodSync(fakeCurl, 0o755);

  const fixture = ripgrepFixture();
  const packageDir = emptyPackageDir();
  const systemRg = path.join(packageDir, "system-rg");
  writeFileSync(systemRg, "#!/bin/sh\necho 'ripgrep 14.0.0'\n");
  const warnings = [];
  const previousPath = process.env.PATH;
  process.env.PATH = `${bindir}:${previousPath ?? ""}`;
  try {
    const dest = postinstall.provisionRipgrep({
      packageDir,
      manifestPath: fixture.manifestPath,
      findSystemRg: () => systemRg,
      log: () => {},
      warn: (message) => warnings.push(message),
    });
    assert.equal(readFileSync(dest, "utf8"), readFileSync(systemRg, "utf8"));
    assert.equal(
      warnings.length,
      1,
      `unexpected warnings: ${JSON.stringify(warnings)}`,
    );
    assert.match(warnings[0], /download failed/);
    assert.match(warnings[0], /system ripgrep/);
  } finally {
    process.env.PATH = previousPath;
  }

  const argv = readFileSync(argvFile, "utf8").split("\n");
  for (const [flag, value] of [
    ["--retry", "3"],
    ["--connect-timeout", "30"],
    ["--max-time", "300"],
  ]) {
    const at = argv.indexOf(flag);
    assert.notEqual(at, -1, `curl must be called with ${flag}`);
    assert.equal(argv[at + 1], value, `${flag} must be ${value}`);
  }
  assert.equal(argv.includes(RG_URL), true, "curl must be given the pinned URL");
});
