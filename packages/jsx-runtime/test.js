import { test } from "node:test";
import assert from "node:assert/strict";
import { jsx, jsxs, isElement, Fragment, componentSchema } from "./src/index.js";

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
  assert.ok(ops.some((o) => o.op === "setProp" && o.id === 1 && o.key === "padding" && o.value === "24"));
  assert.ok(ops.some((o) => o.op === "setProp" && o.id === 1 && o.key === "gap" && o.value === "20"));
  assert.ok(ops.some((o) => o.op === "setProp" && o.id === 1 && o.key === "fill" && o.value === "#f4f3f0"));
  assert.ok(ops.some((o) => o.op === "setProp" && o.id === 2 && o.key === "fill" && o.value === "#3a424a"));
  assert.ok(ops.some((o) => o.op === "setProp" && o.id === 2 && o.key === "height" && o.value === "64"));
  assert.ok(ops.some((o) => o.op === "setProp" && o.id === 2 && o.key === "padding" && o.value === "20"));
  assert.ok(ops.some((o) => o.op === "setProp" && o.id === 2 && o.key === "justifyCenter" && o.value === "true"));
  assert.ok(ops.some((o) => o.op === "setProp" && o.id === 3 && o.key === "color" && o.value === "#f4f3f0"));
  assert.ok(ops.some((o) => o.op === "setProp" && o.id === 4 && o.key === "color" && o.value === "#2c333a"));
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

 test("button lowers to a labeled host node", () => {
  const ops = renderToOps(jsx("button", { children: "Save" }));
  assert.deepEqual(ops, [
    { op: "create", kind: "button" },
    { op: "setText", id: 1, text: "Save" },
  ]);
});

test("scroll lowers to a scroll container", () => {
  const ops = renderToOps(
    jsx("scroll", { width: 200, height: 120, children: [jsx("rect", { height: 400 })] }),
  );
  assert.equal(ops[0].op, "create");
  assert.equal(ops[0].kind, "scroll");
  assert.ok(ops.some((o) => o.op === "setProp" && o.key === "height" && o.value === "120"));
  assert.ok(ops.some((o) => o.op === "append" && o.parent === 1 && o.child === 2));
});

test("list lowers to list and list_item rows", () => {
  const ops = renderToOps(
    jsx("list", {
      width: 200,
      children: [
        jsx("list_item", { height: 36, children: "Short" }),
        jsx("list_item", { height: 80, children: "Tall" }),
      ],
    }),
  );
  assert.equal(ops[0].kind, "list");
  assert.ok(ops.some((o) => o.op === "create" && o.kind === "list_item"));
  assert.ok(ops.some((o) => o.op === "setProp" && o.key === "height" && o.value === "80"));
  assert.ok(ops.some((o) => o.op === "setText" && o.text === "Tall"));
});

test("component schema is what the runtime mounts", () => {
  const names = componentSchema.components.map((c) => c.name);
  for (const kind of ["button", "text_field", "scroll", "list", "list_item", "text", "window"]) {
    const spec = componentSchema.components.find((c) => c.name === kind);
    assert.ok(spec, `schema omits core widget ${kind}`);
    assert.ok(Array.isArray(spec.props));
    assert.ok(Array.isArray(spec.events));
  }
  assert.ok(names.includes("button"));
  assert.throws(() => renderToOps(jsx("nope", {})), /unknown element type nope/);
});

test("text_field lowers to a labeled value", () => {
  const ops = renderToOps(jsx("text_field", { name: "Title", children: "Hello" }));
  assert.deepEqual(ops, [
    { op: "create", kind: "text_field" },
    { op: "setProp", id: 1, key: "name", value: "Title" },
    { op: "setText", id: 1, text: "Hello" },
  ]);
});
