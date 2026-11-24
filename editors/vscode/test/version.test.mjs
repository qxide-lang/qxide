// The "qxide is too old" check (src/version.ts), loaded from the compiled `out/` directory.
import { test } from "node:test";
import assert from "node:assert/strict";
import { isTooOld, parseVersion } from "../out/version.js";

test("version parsing", () => {
  assert.deepEqual(parseVersion("0.8.0"), [0, 8, 0]);
  assert.deepEqual(parseVersion("0.9.0-dev"), [0, 9, 0]);
  assert.equal(parseVersion("nightly"), undefined);
  assert.equal(parseVersion(undefined), undefined);
});

test("only servers known to be older than the documented language are reported", () => {
  // The released 0.8.0 (no `while`) reports no language level.
  assert.equal(isTooOld({ version: "0.8.0" }), true);
  assert.equal(isTooOld({ version: "0.7.2", languageLevel: undefined }), true);
  // A build between releases: still named 0.8.0, but it reports the level.
  assert.equal(isTooOld({ version: "0.8.0", languageLevel: 1 }), false);
  assert.equal(isTooOld({ version: "0.8.0", languageLevel: 0 }), true);
  assert.equal(isTooOld({ version: "0.8.0", languageLevel: "1" }), true);
  assert.equal(isTooOld({ version: "0.9.0" }), false);
  assert.equal(isTooOld({ version: "1.0.0" }), false);
  // Unknown versions are not flagged.
  assert.equal(isTooOld({}), false);
  assert.equal(isTooOld({ version: "custom" }), false);
});
