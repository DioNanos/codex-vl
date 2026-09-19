import assert from "node:assert/strict";
import test from "node:test";
import { buildChildStdio } from "../bin/identity_fds.js";

const INHERIT = "inherit";
const IGNORE = "ignore";

test("absent declaration keeps the base 0-2 stdio", () => {
  assert.deepEqual(buildChildStdio(undefined), [INHERIT, INHERIT, INHERIT]);
  assert.deepEqual(buildChildStdio(""), [INHERIT, INHERIT, INHERIT]);
});

test("3:4 extends stdio with integer descriptors 3 and 4", () => {
  assert.deepEqual(buildChildStdio("3:4"), [INHERIT, INHERIT, INHERIT, 3, 4]);
});

test("5:7 fills intermediate descriptors with ignore", () => {
  assert.deepEqual(buildChildStdio("5:7"), [
    INHERIT,
    INHERIT,
    INHERIT,
    IGNORE,
    IGNORE,
    5,
    IGNORE,
    7,
  ]);
});

test("32:31 is the inclusive upper bound", () => {
  const expected = [
    INHERIT,
    INHERIT,
    INHERIT,
    ...new Array(28).fill(IGNORE),
    31,
    32,
  ];
  assert.deepEqual(buildChildStdio("32:31"), expected);
});

test("invalid declarations never invent descriptors", () => {
  for (const invalid of [
    "x:y",
    "3",
    "1:2",
    "0:3",
    "3:0",
    ":",
    "3:",
    "3:4:5",
    "3.5:4",
    " 3:4",
    "3:4 ",
    "3:100000",
    "40:41",
    "33:32",
    "3:3",
    "",
    null,
  ]) {
    assert.deepEqual(
      buildChildStdio(invalid),
      [INHERIT, INHERIT, INHERIT],
      `expected base stdio for ${JSON.stringify(invalid)}`,
    );
  }
});
