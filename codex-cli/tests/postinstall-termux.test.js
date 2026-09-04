import assert from "node:assert/strict";
import {
  copyFileSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { runPostinstall } from "../scripts/postinstall.js";

const sourceRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const launcherNames = ["codex.js", "codex-exec.js"];

function createFixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), "codex-vl-termux-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const binDir = path.join(root, "bin");
  mkdirSync(binDir);
  for (const name of launcherNames) {
    copyFileSync(
      path.join(sourceRoot, "bin", name),
      path.join(binDir, name),
    );
  }
  return { binDir, root };
}

test("Termux postinstall rewrites both launcher shebangs and is idempotent", async (t) => {
  const fixture = createFixture(t);
  const prefix = path.join(fixture.root, "termux-prefix");
  const originals = new Map(
    launcherNames.map((name) => [
      name,
      readFileSync(path.join(fixture.binDir, name), "utf8"),
    ]),
  );

  await runPostinstall({
    binDir: fixture.binDir,
    env: { PREFIX: prefix, TERMUX_VERSION: "test" },
    platform: "linux",
  });

  const expectedShebang = `#!${prefix}/bin/env node`;
  const rewritten = new Map(
    launcherNames.map((name) => [
      name,
      readFileSync(path.join(fixture.binDir, name), "utf8"),
    ]),
  );
  for (const [name, contents] of rewritten) {
    assert.equal(contents.split("\n", 1)[0], expectedShebang);
    assert.equal(
      contents.slice(contents.indexOf("\n")),
      originals.get(name).slice(originals.get(name).indexOf("\n")),
    );
  }

  await runPostinstall({
    binDir: fixture.binDir,
    env: { PREFIX: prefix, TERMUX_VERSION: "test" },
    platform: "linux",
  });
  for (const name of launcherNames) {
    assert.equal(
      readFileSync(path.join(fixture.binDir, name), "utf8"),
      rewritten.get(name),
    );
  }
});

test("non-Termux postinstall leaves launcher files unchanged", async (t) => {
  const fixture = createFixture(t);
  const warnings = [];
  const before = new Map(
    launcherNames.map((name) => [
      name,
      readFileSync(path.join(fixture.binDir, name), "utf8"),
    ]),
  );

  await runPostinstall({
    binDir: fixture.binDir,
    env: { PREFIX: "/usr" },
    platform: "linux",
    warn: (message) => warnings.push(message),
  });

  assert.deepEqual(warnings, []);

  for (const name of launcherNames) {
    assert.equal(
      readFileSync(path.join(fixture.binDir, name), "utf8"),
      before.get(name),
    );
  }
});
