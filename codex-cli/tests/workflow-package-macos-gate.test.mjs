// The packaging gate is a shell step. A pipeline where `grep -q` exits at the
// first match — `printf ... | grep -q` — kills the writer with SIGPIPE (141),
// and `set -o pipefail` then fails the step on a correct payload. This runs the
// real step, extracted from the workflow, against a package built from the real
// script and manifest, so the gate is exercised the way CI exercises it.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import {
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

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
);
const workflowPath = path.join(
  repoRoot,
  ".github",
  "workflows",
  "package-macos-candidate.yml",
);
const STEP_NAME = "name: Verify source-build package payload";

const scratch = mkdtempSync(path.join(tmpdir(), "codex-vl-gate-test-"));
process.on("exit", () => rmSync(scratch, { recursive: true, force: true }));

// The step body, dedented from the workflow, so the test runs exactly what CI
// runs — including its `set -euo pipefail`.
function gateScript() {
  const lines = readFileSync(workflowPath, "utf8").split("\n");
  const stepAt = lines.findIndex((line) => line.includes(STEP_NAME));
  assert.notEqual(stepAt, -1, `the workflow must have a step named ${STEP_NAME}`);
  const runAt = lines.findIndex((line, index) => index > stepAt && line.trim() === "run: |");
  assert.notEqual(runAt, -1, "the gate step must have a run block");
  const indent = lines[runAt + 1].match(/^\s*/)[0];
  assert.ok(indent.length > 0, "the run block must be indented");
  const body = [];
  for (let index = runAt + 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.trim() !== "" && !line.startsWith(indent)) break;
    body.push(line.startsWith(indent) ? line.slice(indent.length) : "");
  }
  return body.join("\n");
}

function buildPayload({ mutatePostinstall } = {}) {
  const root = mkdtempSync(path.join(scratch, "payload-"));
  const packageDir = path.join(root, "package");
  const scriptsDir = path.join(packageDir, "scripts");
  mkdirSync(path.join(packageDir, "codex-rs"), { recursive: true });
  mkdirSync(scriptsDir, { recursive: true });
  mkdirSync(path.join(packageDir, "vendor", "aarch64-apple-darwin", "codex"), {
    recursive: true,
  });
  writeFileSync(path.join(packageDir, "codex-rs", "Cargo.toml"), "[workspace]\n");
  writeFileSync(
    path.join(packageDir, "vendor", "aarch64-apple-darwin", "codex", ".gitkeep"),
    "",
  );
  writeFileSync(
    path.join(scriptsDir, "rg-manifest"),
    readFileSync(path.join(repoRoot, "scripts", "codex_package", "rg")),
  );
  let postinstall = readFileSync(
    path.join(repoRoot, "codex-cli", "scripts", "postinstall_darwin_build.js"),
    "utf8",
  );
  if (mutatePostinstall) postinstall = mutatePostinstall(postinstall);
  writeFileSync(path.join(scriptsDir, "postinstall_darwin_build.js"), postinstall);
  const packOutput = path.join(root, "codex-darwin-arm64.tgz");
  execFileSync("tar", ["-czf", packOutput, "-C", root, "package"]);
  return packOutput;
}

function runGate(packOutput, runId) {
  const scriptPath = path.join(scratch, `gate-${runId}.sh`);
  writeFileSync(scriptPath, `${gateScript()}\n`);
  return spawnSync("bash", [scriptPath], {
    encoding: "utf8",
    env: {
      ...process.env,
      PACK_OUTPUT: packOutput,
      RUNNER_TEMP: mkdtempSync(path.join(scratch, "runner-")),
    },
  });
}

test("the packaging gate accepts a correct payload, ten times in a row", () => {
  const packOutput = buildPayload();
  for (let runId = 1; runId <= 10; runId += 1) {
    const result = runGate(packOutput, runId);
    assert.equal(
      result.status,
      0,
      `run ${runId} failed on a correct payload (rc=${result.status}); ` +
        `stdout=${JSON.stringify(result.stdout)} stderr=${JSON.stringify(result.stderr)} ` +
        "— a writer taking SIGPIPE under `set -o pipefail` looks exactly like this",
    );
  }
});

test("the packaging gate rejects a payload without the integrity wiring", () => {
  const packOutput = buildPayload({
    mutatePostinstall: (source) =>
      source.split("codex-path/rg").join("codex-path/removed"),
  });
  const result = runGate(packOutput, "mutant");
  assert.notEqual(result.status, 0, "the gate must reject a payload with no rg wiring");
  assert.match(result.stderr, /does not reference codex-path\/rg/);
});
