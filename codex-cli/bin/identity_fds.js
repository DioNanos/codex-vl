// Identity channel descriptor forwarding for the npm wrappers.
//
// NexusCrew launches cells with a bidirectional pipe pair on file
// descriptors 3 and 4 and publishes the pair via NEXUSCREW_IDENTITY_FD
// ("a:b"). Node's `stdio: "inherit"` only forwards descriptors 0-2: in any
// additional position it is equivalent to "ignore", so a descriptor passed
// that way never reaches the child and the native binary fails closed.
// Measured behavior (D-224): stdio[3] = "inherit" makes the child fail with
// EINVAL, while stdio[3] = 3 forwards the real parent descriptor and the
// child reads the channel marker. This helper converts the declared
// descriptors into a `stdio` array holding the descriptor NUMBERS at their
// declared positions, which Node forwards verbatim to the child.
// Declarations are bounded: both descriptors must be distinct and no larger
// than 32, and a declaration that violates the bound is treated as absent so
// the daemon fails closed with the reason it owns.

import assert from "node:assert/strict";

const BASE_STDIO = ["inherit", "inherit", "inherit"];

// Both descriptors are forwarded to a single child, so 32 is a generous
// ceiling: anything above it (or a repeated descriptor) cannot be a genuine
// NexusCrew declaration and must not silently build a huge stdio array.
const MAX_IDENTITY_FD = 32;

function parseIdentityFdPair(rawValue) {
  if (typeof rawValue !== "string") {
    return null;
  }

  const match = rawValue.match(/^(\d+):(\d+)$/);
  if (!match) {
    return null;
  }

  const first = Number(match[1]);
  const second = Number(match[2]);
  if (
    first < 3 ||
    second < 3 ||
    first > MAX_IDENTITY_FD ||
    second > MAX_IDENTITY_FD
  ) {
    return null;
  }
  if (first === second) {
    return null;
  }

  return [first, second];
}

/**
 * Build the `stdio` option for spawning the native binary.
 *
 * Returns the base three-element inherit array when the declaration is
 * absent or invalid: the variable must stay untouched so the daemon can
 * surface the right fail-closed reason. On a valid declaration the array is
 * extended with "ignore" placeholders and the descriptor NUMBER at each
 * declared fd: only integers are actually inherited by the child, while
 * "inherit" in additional positions is silently demoted to "ignore".
 */
export function buildChildStdio(identityFdValue) {
  const pair = parseIdentityFdPair(identityFdValue);
  if (!pair) {
    return [...BASE_STDIO];
  }

  const [first, second] = pair;
  const length = Math.max(first, second) + 1;
  const stdio = new Array(length).fill("ignore");
  stdio[0] = "inherit";
  stdio[1] = "inherit";
  stdio[2] = "inherit";
  stdio[first] = first;
  stdio[second] = second;
  return stdio;
}

assert.equal(buildChildStdio(undefined).length, 3);
assert.equal(buildChildStdio("x:y").length, 3);
assert.deepEqual(buildChildStdio("3:4"), [
  "inherit",
  "inherit",
  "inherit",
  3,
  4,
]);
