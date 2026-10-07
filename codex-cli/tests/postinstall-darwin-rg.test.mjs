// The macOS postinstall ships inside a package whose package.json declares no
// "type", so it is CommonJS — while codex-cli/package.json here is "type":
// "module". Copy the script next to a CommonJS package.json to load it as a
// library. Network and platform are injected: these tests never build anything.
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
const RG_URL = "https://example.invalid/ripgrep-15.2.0-aarch64-apple-darwin.tar.gz";

let postinstall;

before(() => {
  // Older revisions run the whole build at module scope — and exit on a
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
  assert.match(
    probe.stdout || "",
    /provisionRipgrep/,
    "postinstall_darwin_build.js must export provisionRipgrep (and not run the " +
      `build when required as a library); stdout=${JSON.stringify(probe.stdout)}`,
  );
  for (const name of ["verifyPackageLayout", "findCodesign", "codesignAdhoc"]) {
    assert.match(
      probe.stdout || "",
      new RegExp(name),
      `postinstall_darwin_build.js must export ${name}`,
    );
  }
  postinstall = createRequire(import.meta.url)(scriptCopy);
});

// A real tar.gz fixture: extraction runs for real, only the download is faked.
function ripgrepFixture({ digest } = {}) {
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
  const manifestPath = path.join(dir, "rg-manifest");
  writeFileSync(
    manifestPath,
    [
      "#!/usr/bin/env dotslash",
      JSON.stringify(
        {
          name: "rg",
          platforms: {
            "macos-aarch64": {
              size: bytes.length,
              hash: "sha256",
              digest:
                digest ?? createHash("sha256").update(bytes).digest("hex"),
              format: "tar.gz",
              path: RG_MEMBER,
              providers: [{ url: RG_URL }],
            },
          },
        },
        null,
        2,
      ),
      "",
    ].join("\n"),
  );
  return { dir, archive, manifestPath };
}

function emptyPackageDir() {
  const packageDir = mkdtempSync(path.join(scratch, "package-"));
  return packageDir;
}

const quiet = { log: () => {}, warn: () => {} };

test("A: dal manifest DotSlash, rg finisce in codex-path/ eseguibile", () => {
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
  assert.equal(statSync(dest).mode & 0o111, 0o111, "rg deve essere eseguibile");
});

test("A: sha256 sbagliato -> errore e nessun rg lasciato in codex-path/", () => {
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

test("guardia: elenca esattamente i requisiti che il daemon pretende", () => {
  const packageDir = emptyPackageDir();
  mkdirSync(path.join(packageDir, "bin"), { recursive: true });
  writeFileSync(path.join(packageDir, "bin", "codex"), "");
  writeFileSync(path.join(packageDir, "codex-package.json"), "{}");
  assert.deepEqual(postinstall.verifyPackageLayout({ packageDir }), [
    "bin/codex-code-mode-host",
    "codex-path/rg",
  ]);

  writeFileSync(path.join(packageDir, "bin", "codex-code-mode-host"), "");
  mkdirSync(path.join(packageDir, "codex-path"), { recursive: true });
  writeFileSync(path.join(packageDir, "codex-path", "rg"), "");
  assert.deepEqual(postinstall.verifyPackageLayout({ packageDir }), []);
});

test("B: manifest assente -> rg di sistema, con avviso (versione non pinnata)", () => {
  const packageDir = emptyPackageDir();
  const systemRg = path.join(packageDir, "system-rg");
  writeFileSync(systemRg, "#!/bin/sh\necho 'ripgrep 14.0.0'\n");
  const warnings = [];
  const dest = postinstall.provisionRipgrep({
    packageDir,
    manifestPath: path.join(packageDir, "no-such-manifest"),
    findSystemRg: () => systemRg,
    log: () => {},
    warn: (message) => warnings.push(message),
  });
  assert.equal(readFileSync(dest, "utf8"), readFileSync(systemRg, "utf8"));
  // Due avvisi distinti: il manifest non si legge, e la versione che si usa
  // non è pinnata dal pacchetto.
  assert.equal(warnings.length, 2, `avvisi inattesi: ${JSON.stringify(warnings)}`);
  assert.match(warnings[0], /manifest unavailable/);
  assert.match(warnings[1], /system ripgrep/);
});

test("B: manifest assente e nessun rg di sistema -> errore chiaro", () => {
  const packageDir = emptyPackageDir();
  assert.throws(
    () =>
      postinstall.provisionRipgrep({
        packageDir,
        manifestPath: path.join(packageDir, "no-such-manifest"),
        findSystemRg: () => null,
        ...quiet,
      }),
    /no ripgrep available/,
  );
});

test("A: il manifest senza la voce macos-aarch64 non passa per il download", () => {
  const packageDir = emptyPackageDir();
  const manifestPath = path.join(packageDir, "rg-manifest");
  writeFileSync(
    manifestPath,
    `#!/usr/bin/env dotslash\n${JSON.stringify({ name: "rg", platforms: {} })}\n`,
  );
  const warnings = [];
  assert.throws(
    () =>
      postinstall.provisionRipgrep({
        packageDir,
        manifestPath,
        findSystemRg: () => null,
        log: () => {},
        warn: (message) => warnings.push(message),
      }),
    /no ripgrep available/,
  );
  assert.equal(warnings.some((w) => /macos-aarch64/.test(w)), true);
});

// --- firma ad hoc -----------------------------------------------------------
// `codesign` non ha `--version`: su macOS risponde 2. Chiedere la versione per
// decidere se il tool c'è lo dichiara assente e la firma non viene mai tentata.
test("il rilevamento di codesign non gli chiede --version", () => {
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

test("firma ad hoc: codesign -s - -f, poi verifica con -v", () => {
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

test("macOS senza codesign: avviso esplicito, nessuna firma", () => {
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

test("firma fallita o non verificata: avviso, mai silenzio", () => {
  const fallita = [];
  assert.equal(
    postinstall.codesignAdhoc("/tmp/rg-target", {
      platform: "darwin",
      find: () => "/usr/bin/codesign",
      run: () => ({ status: 1 }),
      log: () => {},
      warn: (message) => fallita.push(message),
    }),
    false,
  );
  assert.match(fallita.join(" "), /codesign failed/i);

  const nonVerificata = [];
  let call = 0;
  assert.equal(
    postinstall.codesignAdhoc("/tmp/rg-target", {
      platform: "darwin",
      find: () => "/usr/bin/codesign",
      run: () => ({ status: call++ === 0 ? 0 : 1 }),
      log: () => {},
      warn: (message) => nonVerificata.push(message),
    }),
    false,
  );
  assert.match(nonVerificata.join(" "), /verification failed/i);
});

test("fuori da macOS la firma è un no-op silenzioso", () => {
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
