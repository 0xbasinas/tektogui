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

import { mount, renderToOps } from "./src/index.js";
import { Notes } from "./notes-app.js";

test("Notes JSX lowers to the same tree shape as the Rust example", () => {
  const ops = renderToOps(Notes());
  const creates = ops.filter((o) => o.op === "create").map((o) => o.kind);
  assert.deepEqual(creates, ["window", "rect", "text", "text"]);
  assert.ok(ops.some((o) => o.op === "setProp" && o.id === 1 && o.key === "name" && o.value === "Notes"));
  assert.ok(ops.some((o) => o.op === "setProp" && o.id === 2 && o.key === "name" && o.value === "banner"));
  assert.ok(ops.some((o) => o.op === "setText" && o.id === 3 && o.text === "Notes"));
  assert.ok(ops.some((o) => o.op === "setText" && o.id === 4 && o.text === "Hello from Tekto"));
  assert.ok(ops.some((o) => o.op === "append" && o.parent === 1 && o.child === 2));
  assert.ok(ops.some((o) => o.op === "append" && o.parent === 2 && o.child === 3));
  assert.ok(ops.some((o) => o.op === "append" && o.parent === 1 && o.child === 4));
});

test("mount talks to a host by id, not through Notes-only APIs", () => {
  const calls = [];
  let n = 1;
  const host = {
    create(kind) {
      const id = n++;
      calls.push(["create", kind, id]);
      return id;
    },
    setText(id, text) {
      calls.push(["setText", id, text]);
    },
    setProp(id, key, value) {
      calls.push(["setProp", id, key, value]);
    },
    append(parent, child) {
      calls.push(["append", parent, child]);
    },
  };
  const root = mount(jsx("text", { children: "Hi" }), host);
  assert.equal(root, 1);
  assert.deepEqual(
    calls.filter((c) => c[0] === "create" || c[0] === "setText"),
    [
      ["create", "text", 1],
      ["setText", 1, "Hi"],
    ],
  );
});
