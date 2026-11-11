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

test("image validates its description and forwards bounded source data", () => {
  const source = { width: 1, height: 1, rgba: new Uint8Array([255, 0, 0, 255]) };
  const ops = renderToOps(jsx("image", { name: "Logo", imageSource: source, imageFit: "cover" }));
  assert.ok(ops.some(op => op.key === "imageSource" && JSON.parse(op.value).rgba.join(",") === "255,0,0,255"));
  assert.throws(() => renderToOps(jsx("image", { imageSource: source })), /name or decorative/);
  assert.throws(() => renderToOps(jsx("image", { name: "Logo" })), /imageSource/);
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

test("component schema lists every prop JSX forwards on each kind", () => {
  const names = componentSchema.components.map((c) => c.name);
  for (const kind of ["button", "text_field", "scroll", "list", "list_item", "text", "window", "group", "rect"]) {
    assert.ok(names.includes(kind), `schema omits core widget ${kind}`);
  }
  assert.throws(() => renderToOps(jsx("nope", {})), /unknown element type nope/);

  // Disabled is supported only on interactive controls.
  const aliasKeys = new Set(Object.keys(componentSchema.propAliases ?? {}));
  const forwarded = new Set(
    componentSchema.components.flatMap((c) => c.props.map((p) => p.name)).filter((name) => !aliasKeys.has(name)),
  );
  forwarded.delete("disabled");
  for (const key of ["imageSource", "imageFit", "decorative", "multiline"]) forwarded.delete(key);
  forwarded.add("title");
  for (const spec of componentSchema.components) {
    assert.ok(Array.isArray(spec.props));
    assert.ok(Array.isArray(spec.events));
    const listed = new Set(spec.props.map((p) => p.name));
    for (const key of ["imageSource", "imageFit", "decorative"]) assert.equal(listed.has(key), spec.name === "image");
    assert.equal(listed.has("multiline"), spec.name === "text_field");
    assert.equal(listed.has("disabled"), ["button", "text_field", "checkbox", "switch", "slider", "radio", "tab", "menu_item", "select"].includes(spec.name));
    for (const key of forwarded) {
      assert.ok(listed.has(key), `${spec.name} omits prop ${key} that JSX forwards`);
    }
  }

  const button = renderToOps(jsx("button", { width: 200, height: 40, children: "Save" }));
  assert.ok(button.some((o) => o.op === "setProp" && o.key === "width" && o.value === "200"));
  assert.ok(button.some((o) => o.op === "setProp" && o.key === "height" && o.value === "40"));
  const buttonSpec = componentSchema.components.find((c) => c.name === "button");
  assert.deepEqual(buttonSpec.events.map((event) => event.name), ["click"]);
  assert.ok(buttonSpec.jsxProps.some((prop) => prop.name === "onClick"));
  for (const key of ["width", "height"]) {
    assert.ok(
      buttonSpec.props.some((p) => p.name === key),
      `button schema omits ${key}`,
    );
  }
});

test("text_field lowers to a labeled value", () => {
  const ops = renderToOps(jsx("text_field", { name: "Title", children: "Hello" }));
  assert.deepEqual(ops, [
    { op: "create", kind: "text_field" },
    { op: "setProp", id: 1, key: "name", value: "Title" },
    { op: "setText", id: 1, text: "Hello" },
  ]);
});

test("empty text_field keeps its label out of the value", () => {
  assert.deepEqual(renderToOps(jsx("text_field", { name: "Title" })), [
    { op: "create", kind: "text_field" },
    { op: "setProp", id: 1, key: "name", value: "Title" },
  ]);
});

test("reactive text and button callbacks require a retained host", async () => {
  const { createSignal, createEffect } = await import("../core/src/index.js");
  const [count, setCount] = createSignal(0);
  const callbacks = new Map(), disposers = new Map(), text = new Map();
  let next = 1;
  const host = { create: () => next++, setProp() {}, append() {},
    setText: (id, value) => text.set(id, value),
    onClick: (id, fn) => callbacks.set(id, fn),
    onDispose: (id, fn) => disposers.set(id, fn), remove() {} };
  mount(jsxs("group", { children: [jsx("text", { text: () => count() }),
    jsx("button", { onClick: () => setCount((n) => n + 1), children: "+" })] }), host, { createEffect });
  assert.equal(text.get(2), "0");
  callbacks.get(3)(); callbacks.get(3)();
  assert.equal(text.get(2), "2");
  disposers.get(2)(); callbacks.get(3)();
  assert.equal(text.get(2), "2");
  assert.throws(() => renderToOps(jsx("button", { onClick() {} })), /retained host/);
  assert.throws(() => renderToOps(jsx("text", { text: count })), /retained host/);
});

test("failed mount rolls back created nodes and reactive bindings", async () => {
  const { createSignal, createEffect } = await import("../core/src/index.js");
  const [value, setValue] = createSignal(0);
  const cleanups = new Map(), removed = [], writes = [];
  let id = 0;
  const host = { create: () => ++id, setText: (id, text) => writes.push([id, text]),
    setProp() {}, append() {}, onDispose: (id, fn) => cleanups.set(id, fn),
    remove(id) { removed.push(id); cleanups.get(id)?.(); } };
  assert.throws(() => mount(jsxs("group", { children: [jsx("text", { text: value }), jsx("bad", {})] }), host, { createEffect }), /unknown element/);
  assert.deepEqual(removed, [2, 1]);
  setValue(1);
  assert.deepEqual(writes, [[2, "0"]]);
});

test("disabled controls lower their state to host props", () => {
  for (const kind of ["button", "text_field"]) {
    const ops = renderToOps(jsx(kind, { disabled: true }));
    assert.ok(renderToOps(jsx(kind, { disabled: false })).some((op) => op.op === "setProp" && op.key === "disabled" && op.value === "false"));
    assert.ok(ops.some((op) => op.op === "setProp" && op.key === "disabled" && op.value === "true"));
  }
});

// Input handlers are registered only on retained text fields.
{
  let callback;
  const host = { create: () => 1, setText() {}, setProp() {}, append() {}, remove() {}, onInput: (_id, fn) => { callback = fn; } };
  let value = '';
  mount(jsx('text_field', { onInput: v => { value = v; } }), host);
  callback('hello');
  assert.equal(value, 'hello');
  assert.throws(() => renderToOps(jsx('text_field', { onInput() {} })), /retained host/);
  assert.throws(() => mount(jsx('button', { onInput() {} }), host), /text_field/);
}

test("checkbox lowers checked state", () => {
  const ops = renderToOps(jsx("checkbox", { checked: true, children: "Remember" }));
  assert.ok(ops.some(o => o.op === "create" && o.kind === "checkbox"));
  assert.ok(ops.some(o => o.op === "setProp" && o.key === "checked" && o.value === "true"));
});


test("switch lowers checked state", () => {
  const ops = renderToOps(jsx("switch", { checked: true, children: "Alerts" }));
  assert.ok(ops.some(o => o.op === "create" && o.kind === "switch"));
  assert.ok(ops.some(o => o.op === "setProp" && o.key === "checked" && o.value === "true"));
});


test("progress lowers its numeric value", () => {
  const ops = renderToOps(jsx("progress", { name: "Upload", value: 0.5 }));
  assert.ok(ops.some(o => o.op === "create" && o.kind === "progress"));
  assert.ok(ops.some(o => o.op === "setProp" && o.key === "value" && o.value === "0.5"));
});


test("slider lowers a value and disabled state", () => {
  const ops = renderToOps(jsx("slider", { name: "Volume", value: 0.5, disabled: true }));
  assert.ok(ops.some(o => o.op === "create" && o.kind === "slider"));
  assert.ok(ops.some(o => o.op === "setProp" && o.key === "value" && o.value === "0.5"));
});


test("radio group lowers labeled options", () => {
  const ops = renderToOps(jsx("radio_group", { name: "Size", children: [jsx("radio", { checked: true, children: "Small" }), jsx("radio", { children: "Large" })] }));
  assert.ok(ops.some(o => o.op === "create" && o.kind === "radio_group"));
  assert.equal(ops.filter(o => o.op === "create" && o.kind === "radio").length, 2);
});


test("context menu lowers as a detached owned menu", () => {
  const ops = renderToOps(jsx("window", { children: jsx("button", { children: "Target", contextMenu: jsx("menu", { name: "Actions", children: jsx("menu_item", { children: "Open" }) }) }) }));
  assert.ok(ops.some(op => op.op === "bindContextMenu" && op.target === 2 && op.menu === 3));
  assert.ok(!ops.some(op => op.op === "append" && op.child === 3));
});

test("tab list lowers options and registers tab activation", () => {
  const ops = renderToOps(jsx("tab_list", { name: "Pages", children: [jsx("tab", { checked: true, children: "General" }), jsx("tab", { children: "Advanced" })] }));
  assert.ok(ops.some(o => o.op === "create" && o.kind === "tab_list"));
  assert.equal(ops.filter(o => o.op === "create" && o.kind === "tab").length, 2);
  let callback;
  const host = { create: () => 1, setText() {}, setProp() {}, append() {}, remove() {}, onClick: (_id, fn) => { callback = fn; } };
  mount(jsx("tab", { onClick: () => 42, children: "General" }), host);
  assert.equal(callback(), 42);
});

test("dialog lowers its accessible name and children", () => {
  const ops = renderToOps(jsx("dialog", { name: "Confirm", children: jsx("button", { children: "OK" }) }));
  assert.ok(ops.some(o => o.op === "create" && o.kind === "dialog"));
  assert.ok(ops.some(o => o.op === "setProp" && o.key === "name" && o.value === "Confirm"));
});


test("menu item registers its callback", () => {
  let callback;
  const host = { create: () => 1, setText() {}, setProp() {}, append() {}, remove() {}, onClick: (_id, fn) => { callback = fn; } };
  mount(jsx("menu_item", { onClick: () => 42, children: "Open" }), host);
  assert.equal(callback(), 42);
});


test("select registers an input callback", () => {
  let callback;
  const host = { create: () => 1, setText() {}, setProp() {}, append() {}, remove() {}, onInput: (_id, fn) => { callback = fn; } };
  mount(jsx("select", { name: "Size", onInput: value => value }), host);
  assert.equal(callback("Small"), "Small");
});
