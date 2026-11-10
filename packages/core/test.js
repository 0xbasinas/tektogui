import { test } from "node:test";
import assert from "node:assert/strict";
import { createSignal, createEffect, transitionOp, nativeOpJson } from "./src/index.js";

test("transition op is a quiet style tween", () => {
  assert.deepEqual(transitionOp(4, { opacity: 0, x: 12, durationMs: 180 }), {
    op: "transition",
    id: 4,
    opacity: "0",
    x: "12",
    durationMs: 180,
  });
});

test("shortcut native ops encode register, unregister, and take", () => {
  assert.equal(
    nativeOpJson({ op: "shortcutsRegister", accelerator: "Ctrl+K", id: "search" }),
    '{"op":"shortcutsRegister","accelerator":"Ctrl+K","id":"search"}',
  );
  assert.equal(
    nativeOpJson({ op: "shortcutsUnregister", id: "search" }),
    '{"op":"shortcutsUnregister","id":"search"}',
  );
  assert.equal(
    nativeOpJson({ op: "shortcutsTakeActivation" }),
    '{"op":"shortcutsTakeActivation"}',
  );
  assert.throws(() => nativeOpJson({}), /op name/);
});

test("signal updates an effect", () => {
  const [count, setCount] = createSignal(0);
  let seen = 0;
  createEffect(() => {
    seen = count();
  });
  assert.equal(seen, 0);
  setCount(2);
  assert.equal(seen, 2);
});
