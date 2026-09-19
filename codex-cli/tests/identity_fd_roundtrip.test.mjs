// End-to-end proof that the identity descriptors declared via
// NEXUSCREW_IDENTITY_FD actually reach a spawned child when passed as
// integers, and that the pre-D224 "inherit" array does NOT.
//
// Harness note: buildChildStdio("3:4") returns literal descriptors 3 and 4,
// which name this TEST process' descriptors, not the harness pipes. We call
// the real buildChildStdio and then substitute the declared positions with
// the actual parent-side fifo fds, preserving the integer-at-declared-
// position semantics that the wrapper ships.
//
// The negative control uses the pre-D224 array
// ["inherit","inherit","inherit","inherit","inherit"]: per Node docs
// "inherit" in additional positions is equivalent to "ignore", so the child
// read of fd 3 must fail (EINVAL/EBADF) and no echo may be produced. A
// control that passes with the old array would make the test meaningless.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { buildChildStdio } from "../bin/identity_fds.js";

const O_NONBLOCK = fs.constants.O_NONBLOCK;

const CHILD_SCRIPT = `
const fs = require("node:fs");
const buf = Buffer.alloc(64);
try {
  const n = fs.readSync(3, buf, 0, buf.length);
  fs.writeSync(4, "ECHO:" + buf.toString("utf8", 0, n));
} catch (err) {
  process.stderr.write("READ-FAILED:" + err.code);
  process.exit(1);
}
`;

function makeFifo(dir, name) {
  const fifo = path.join(dir, name);
  fs.closeSync(
    fs.openSync(fifo, fs.constants.O_CREAT | fs.constants.O_EXCL, 0o600),
  );
  return fifo;
}

function roundtrip(stdio) {
  const marker = `MARKER-${Math.random().toString(36).slice(2)}`;
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "identity-fd-"));
  const fifoIn = makeFifo(dir, "identity-in.fifo");
  const fifoOut = makeFifo(dir, "identity-out.fifo");

  // FIFO open protocol: reader side opens O_NONBLOCK first so the writer
  // side never blocks; the reader fd of the IN fifo is the one the child
  // inherits, and the parent writes the marker through a transient writer.
  const outReader = fs.openSync(fifoOut, fs.constants.O_RDONLY | O_NONBLOCK);
  const inReader = fs.openSync(fifoIn, fs.constants.O_RDONLY | O_NONBLOCK);
  const inWriter = fs.openSync(fifoIn, fs.constants.O_WRONLY);

  try {
    fs.writeSync(inWriter, marker);
    fs.closeSync(inWriter);

    const result = spawnSync(process.execPath, ["-e", CHILD_SCRIPT], {
      stdio,
      encoding: "utf8",
    });

    let echoed = "";
    try {
      const buf = Buffer.alloc(128);
      const n = fs.readSync(outReader, buf, 0, buf.length);
      echoed = buf.toString("utf8", 0, n);
    } catch {
      echoed = "";
    }

    return {
      marker,
      echoed,
      stderr: result.stderr ?? "",
      status: result.status,
    };
  } finally {
    for (const fd of [outReader, inReader]) {
      try {
        fs.closeSync(fd);
      } catch {
        // already closed
      }
    }
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

test("integer descriptors forward the identity channel to the child", () => {
  const built = buildChildStdio("3:4");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "identity-fd-fix"));
  const fifoIn = makeFifo(dir, "identity-in.fifo");
  const fifoOut = makeFifo(dir, "identity-out.fifo");

  const outReader = fs.openSync(fifoOut, fs.constants.O_RDONLY | O_NONBLOCK);
  const inReader = fs.openSync(fifoIn, fs.constants.O_RDONLY | O_NONBLOCK);
  const inWriter = fs.openSync(fifoIn, fs.constants.O_WRONLY);
  const marker = `MARKER-${Math.random().toString(36).slice(2)}`;

  try {
    fs.writeSync(inWriter, marker);
    fs.closeSync(inWriter);

    // Real buildChildStdio output, declared positions 3 and 4 replaced with
    // the actual parent-side fds (this harness cannot reserve descriptors
    // 3 and 4 for the pipes).
    const stdio = [...built];
    assert.equal(stdio[3], 3);
    assert.equal(stdio[4], 4);
    stdio[2] = "pipe"; // capture child stderr in this harness
    stdio[3] = inReader;
    stdio[4] = fs.openSync(fifoOut, fs.constants.O_WRONLY);

    const result = spawnSync(process.execPath, ["-e", CHILD_SCRIPT], {
      stdio,
      encoding: "utf8",
    });

    const stderrText = result.stderr ?? "";
    assert.equal(result.status, 0, `child failed: ${stderrText}`);
    assert.equal(stderrText, "");

    const buf = Buffer.alloc(128);
    const n = fs.readSync(outReader, buf, 0, buf.length);
    assert.equal(buf.toString("utf8", 0, n), `ECHO:${marker}`);
  } finally {
    fs.closeSync(outReader);
    fs.closeSync(inReader);
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('pre-D224 "inherit" array does NOT forward the identity channel', () => {
  // Same shape the pre-D224 wrapper shipped; stderr piped only to capture
  // the child failure without changing descriptor-3 semantics.
  const oldStdio = ["inherit", "inherit", "pipe", "inherit", "inherit"];
  const { echoed, stderr, status } = roundtrip(oldStdio);

  assert.notEqual(status, 0, "child must fail when fd 3 is not forwarded");
  assert.match(stderr, /READ-FAILED:(EINVAL|EBADF)/);
  assert.equal(echoed, "", "no echo may be produced without forwarded fds");
});
