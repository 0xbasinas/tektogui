import assert from "node:assert/strict";

export function requireOrSkip(t, available, message) {
  if (available) return true;
  if (process.env.TEKTO_REQUIRE_WASM === "1") assert.fail(message);
  t.skip(message);
  return false;
}
