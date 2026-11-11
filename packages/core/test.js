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

test("nested effects restore outer tracking and dispose independently", () => {
  const [before, setBefore] = createSignal(0);
  const [innerValue, setInner] = createSignal(0);
  const [after, setAfter] = createSignal(0);
  const seen = [];
  let inner;
  const outer = createEffect(() => {
    seen.push(`outer-before:${before()}`);
    if (!inner) inner = createEffect(() => { seen.push(`inner:${innerValue()}`); });
    seen.push(`outer-after:${after()}`);
  });
  setInner(1);
  setAfter(1);
  setBefore(1);
  assert.deepEqual(seen, ["outer-before:0", "inner:0", "outer-after:0", "inner:1", "outer-before:0", "outer-after:1", "outer-before:1", "outer-after:1"]);
  outer.dispose(); inner.dispose();
  setAfter(2); setInner(2);
  assert.equal(seen.length, 8);
});
