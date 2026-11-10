import { test } from "node:test";
import assert from "node:assert/strict";
import { createSignal, createEffect } from "./src/index.js";

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
