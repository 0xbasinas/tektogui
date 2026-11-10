import { test } from "node:test";
import assert from "node:assert/strict";
import { jsx, jsxs, isElement, Fragment } from "./src/index.js";

test("jsx builds a plain element, not a DOM node", () => {
  const el = jsxs("window", { title: "Notes", children: [jsx("text", { children: "Hi" })] });
  assert.equal(isElement(el), true);
  assert.equal(el.type, "window");
  assert.equal(el.props.title, "Notes");
  assert.equal(typeof document, "undefined");
  assert.ok(Fragment);
});
