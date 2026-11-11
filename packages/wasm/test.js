import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { jsx, mount, renderToOps, For, Show } from "../jsx-runtime/src/index.js";
import { createEffect, createSignal, untrack } from "../core/src/index.js";
import { Notes } from "../jsx-runtime/notes-app.js";
import { attachCanvasInput, blitPng, decodePng, resizeCanvas } from "./src/canvas.js";
import { choosePaintBackend, detectWebGl2, detectWebGpu, paintPreferred } from "./src/gpu.js";
import { connectWasm, hostFromExports } from "./src/index.js";
import { wasmPath as findWasmPath } from "./src/wasm-path.js";
import { requireOrSkip } from "./test-prerequisites.js";

const wasmPath = findWasmPath(fileURLToPath(new URL("../..", import.meta.url)));

test("composition events replace selection, commit once and cancel on blur", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const field = host.create("text_field"), values = [];
  host.setProp(field, "width", "120"); host.setProp(field, "height", "40");
  host.onInput(field, value => values.push(value));
  host.clickAt(120, 40, 5, 5);
  host.typeText("abc"); host.fieldKey("SelectAll");
  const listeners = new Map();
  const canvas = { width: 120, height: 40,
    addEventListener: (name, fn) => listeners.set(name, fn),
    removeEventListener: (name, fn) => { if (listeners.get(name) === fn) listeners.delete(name); },
  };
  const dispose = attachCanvasInput(host, canvas, () => {});
  listeners.get("compositionstart")({});
  listeners.get("compositionupdate")({ data: "é" });
  assert.deepEqual(values, ["abc"]);
  listeners.get("compositionend")({ data: "é" });
  assert.deepEqual(values, ["abc", "é"]);
  listeners.get("compositionstart")({});
  listeners.get("compositionupdate")({ data: "x" });
  listeners.get("blur")({});
  listeners.get("compositionend")({ data: "x" });
  assert.deepEqual(values, ["abc", "é"]);
  assert.equal(host.typeText("!"), true);
  assert.deepEqual(values, ["abc", "é", "é!"]);
  listeners.get("compositionstart")({});
  listeners.get("compositionupdate")({ data: "z" });
  host.setProp(field, "disabled", "true");
  listeners.get("compositionend")({ data: "z" });
  assert.deepEqual(values, ["abc", "é", "é!"]);
  dispose(); assert.equal(listeners.size, 0);
});

test("editable browser host focuses a field and forwards IME and input", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const field = host.create("text_field"), values = [];
  host.setProp(field, "width", "120"); host.setProp(field, "height", "40");
  host.onInput(field, value => values.push(value));
  const events = new Map(), inputEvents = new Map();
  let focused = false, removed = false;
  const input = { style: {}, value: "", setAttribute() {}, focus() { focused = true; },
    addEventListener: (name, fn) => inputEvents.set(name, fn),
    removeEventListener: (name, fn) => { if (inputEvents.get(name) === fn) inputEvents.delete(name); },
    remove() { removed = true; },
  };
  const canvas = { width: 120, height: 40,
    ownerDocument: { body: { appendChild: child => assert.equal(child, input) }, createElement: () => input },
    getBoundingClientRect: () => ({ left: 0, top: 0, width: 120, height: 40 }),
    addEventListener: (name, fn) => events.set(name, fn),
    removeEventListener: (name, fn) => { if (events.get(name) === fn) events.delete(name); },
  };
  const dispose = attachCanvasInput(host, canvas, () => {});
  events.get("pointerdown")({ button: 0, pointerId: 1, pointerType: "mouse", clientX: 10, clientY: 10, preventDefault() {} });
  assert.equal(focused, true);
  inputEvents.get("compositionstart")({});
  inputEvents.get("compositionupdate")({ data: "é" });
  inputEvents.get("compositionend")({ data: "é" });
  inputEvents.get("input")({ data: "é", inputType: "insertText" });
  inputEvents.get("input")({ data: "!", inputType: "insertText" });
  assert.deepEqual(values, ["é", "é!"]);
  dispose(); assert.equal(removed, true); assert.equal(events.size, 0); assert.equal(inputEvents.size, 0);
});

test("multiline field handles Enter and visual-line selection", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const values = [];
  mount(jsx("text_field", { name: "Notes", multiline: true, width: 120, height: 60,
    onInput: value => values.push(value) }), host);
  host.clickAt(120, 60, 10, 10);
  assert.equal(host.typeText("first"), true);
  assert.equal(host.fieldKey("Enter"), true);
  assert.equal(host.typeText("second"), true);
  assert.equal(values.at(-1), "first\nsecond");
  host.renderPng(120, 60);
  assert.equal(host.fieldKey("ArrowUp"), true);
  assert.equal(host.fieldKey("ArrowDown", true), true);
  assert.notEqual(host.fieldSelection(), null);
});

test("TSX image pixels render through the WASM host", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  mount(jsx("image", { name: "Red", width: 10, height: 10,
    imageSource: { width: 1, height: 1, rgba: new Uint8Array([255, 0, 0, 255]) } }), host);
  const png = host.renderPng(10, 10);
  const image = await decodePng(png);
  assert.deepEqual(Array.from(image.data.slice(0, 4)), [255, 0, 0, 255]);
});

test("Show retains its active branch and disposes it on transition", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const [visible, setVisible] = createSignal({ label: "one" });
  const clicks = [];
  const root = mount(jsx("window", { padding: 0, gap: 0, children: jsx(Show, {
    when: visible,
    children: value => jsx("button", { width: 100, height: 30, onClick: () => clicks.push(value().label) }),
    fallback: jsx("button", { width: 100, height: 30, onClick: () => clicks.push("fallback") }),
  }) }), host, { createEffect, createSignal, untrack });
  const first = host.clickAt(100, 100, 5, 5);
  setVisible({ label: "two" });
  assert.equal(host.clickAt(100, 100, 5, 5), first);
  setVisible(null);
  const fallback = host.clickAt(100, 100, 5, 5);
  assert.notEqual(fallback, first);
  setVisible({ label: "three" });
  const next = host.clickAt(100, 100, 5, 5);
  assert.notEqual(next, first);
  assert.deepEqual(clicks, ["one", "two", "fallback", "three"]);
  host.remove(root);
  setVisible(null);
});

test("keyed JSX reorder retains scene ids and callbacks on WASM", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const [items, setItems] = createSignal([{ id: "a" }, { id: "b" }]);
  const calls = [];
  const root = mount(jsx("window", { padding: 0, gap: 0, children: jsx(For, {
    each: items, by: item => item.id,
    children: item => jsx("button", { width: 100, height: 30, onClick: () => calls.push(item().id) }),
  }) }), host, { createEffect, createSignal, untrack });
  const beforeA = host.clickAt(100, 100, 5, 5);
  const beforeB = host.clickAt(100, 100, 5, 35);
  assert.deepEqual(calls, ["a", "b"]);
  setItems([{ id: "b" }, { id: "a" }, { id: "c" }]);
  assert.equal(host.clickAt(100, 100, 5, 5), beforeB);
  assert.equal(host.clickAt(100, 100, 5, 35), beforeA);
  assert.deepEqual(calls, ["a", "b", "b", "a"]);
  setItems([{ id: "a" }]);
  assert.equal(host.clickAt(100, 100, 5, 5), beforeA);
  assert.throws(() => setItems([{ id: "x" }, { id: "x" }]), /unique/);
  assert.equal(host.clickAt(100, 100, 5, 5), beforeA);
  host.remove(root);
  setItems([{ id: "a" }, { id: "d" }]);
});

test("wasm host applies HostOps and returns a PNG", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  host.reset();
  const id = host.create("text");
  assert.equal(id, 1);
  host.setText(id, "Hello from WASM");
  host.setProp(id, "fontSize", "18");
  const png = host.renderPng(120, 40);
  assert.equal(png[0], 0x89);
  assert.equal(png[1], 0x50);
  assert.equal(png[2], 0x4e);
  assert.equal(png[3], 0x47);
  assert.ok(png.length > 32);

  host.reset();
  const again = host.applyOps([{ op: "create", kind: "rect" }]);
  assert.deepEqual(again, [1]);
  await assert.rejects(() => Promise.resolve().then(() => host.applyOps([{ op: "create", kind: "dom-node" }])));
});

test("wasm JSX dispatches clicks and field input and clears callbacks", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const clicks = [], values = [];
  const root = mount(jsx("window", { padding: 0, gap: 0, children: [
    jsx("button", { width: 80, height: 30, onClick: () => clicks.push("click") }),
    jsx("text_field", { width: 80, height: 30, name: "Name", onInput: (value) => values.push(value) }),
  ] }), host);
  assert.equal(host.clickAt(100, 100, 10, 10), 2);
  assert.deepEqual(clicks, ["click"]);
  assert.equal(host.clickAt(100, 100, 10, 40), 3);
  assert.equal(host.typeText("a"), true);
  assert.equal(host.backspace(), true);
  assert.deepEqual(values, ["a", ""]);
  host.setProp(3, "disabled", "true");
  assert.equal(host.typeText("z"), false);
  assert.deepEqual(values, ["a", ""]);
  host.setProp(2, "disabled", "true");
  assert.equal(host.clickAt(100, 100, 10, 10), null);
  assert.deepEqual(clicks, ["click"]);
  host.remove(root);
  host.reset();
  const replacement = host.create("button");
  host.clickAt(100, 100, 5, 5);
  assert.equal(replacement, 1);
  assert.deepEqual(clicks, ["click"]);
});

test("canvas selects, copies, cuts and pastes through the wasm field", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const field = host.create("text_field"), values = [];
  host.setProp(field, "width", "100"); host.setProp(field, "height", "30");
  host.onInput(field, value => values.push(value));
  host.clickAt(100, 30, 5, 5);
  host.typeText("a👩‍💻b");
  const listeners = new Map();
  const canvas = { width: 100, height: 30,
    addEventListener: (name, fn) => listeners.set(name, fn),
    removeEventListener: (name, fn) => { if (listeners.get(name) === fn) listeners.delete(name); },
  };
  const dispose = attachCanvasInput(host, canvas, () => {});
  let prevented = 0, clipboard = "";
  const key = (key, extra = {}) => listeners.get("keydown")({ key, preventDefault: () => prevented++, ...extra });
  const data = { setData: (_, value) => { clipboard = value; }, getData: () => clipboard };
  const clip = type => listeners.get(type)({ type, clipboardData: data, preventDefault: () => prevented++ });
  key("ArrowLeft"); key("ArrowLeft", { shiftKey: true });
  clip("copy"); assert.equal(clipboard, "👩‍💻");
  clip("cut"); assert.equal(values.at(-1), "ab");
  clip("paste"); assert.equal(values.at(-1), "a👩‍💻b");
  key("a", { ctrlKey: true }); assert.equal(host.fieldSelection(), "a👩‍💻b");
  key("Delete"); assert.equal(values.at(-1), "");
  assert.equal(prevented, 7);
  host.setProp(field, "disabled", "true");
  key("a", { ctrlKey: true });
  assert.equal(host.fieldSelection(), null);
  dispose(); assert.equal(listeners.size, 0);
});

test("canvas mouse drag selects text and a click positions the caret", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const field = host.create("text_field"), values = [];
  host.setProp(field, "width", "200"); host.setProp(field, "height", "30");
  host.onInput(field, value => values.push(value));
  host.clickAt(200, 30, 5, 10); host.typeText("Hello world");
  const listeners = new Map();
  const canvas = { width: 200, height: 30,
    getBoundingClientRect: () => ({ left: 0, top: 0, width: 200, height: 30 }),
    addEventListener: (name, fn) => listeners.set(name, fn),
    removeEventListener: (name, fn) => { if (listeners.get(name) === fn) listeners.delete(name); },
  };
  const dispose = attachCanvasInput(host, canvas, () => {});
  const event = (x, type = "pointerdown") => ({ type, pointerId: 1, pointerType: "mouse", button: 0, clientX: x, clientY: 15, preventDefault() {} });
  listeners.get("pointerdown")(event(0));
  listeners.get("pointermove")(event(199, "pointermove"));
  listeners.get("pointerup")(event(199, "pointerup"));
  assert.equal(host.fieldSelection(), "Hello world");
  host.typeText("X"); assert.equal(values.at(-1), "X");
  listeners.get("pointerdown")(event(0));
  listeners.get("pointerup")(event(0, "pointerup"));
  host.typeText("Y"); assert.equal(values.at(-1), "YX");
  dispose(); assert.equal(listeners.size, 0);
});

test("cleanup failure does not retain other callbacks on reset", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const root = host.create("window"), first = host.create("button"), second = host.create("button");
  host.append(root, first); host.append(root, second);
  let disposed = 0, clicks = 0;
  host.onDispose(first, () => { throw new Error("cleanup failed"); });
  host.onDispose(second, () => disposed++);
  host.onClick(second, () => clicks++);
  assert.throws(() => host.reset(), /cleanup failed/);
  assert.equal(disposed, 1);
  host.create("window"); host.create("button"); host.create("button");
  host.clickAt(100, 100, 10, 10);
  assert.equal(clicks, 0);
});

test("raw HostOps clean callbacks and disabled overlay clicks keep it open", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const root = host.create("window"); host.setProp(root, "padding", "0");
  const anchor = host.create("button");
  host.setProp(anchor, "width", "20"); host.setProp(anchor, "height", "20");
  host.append(root, anchor);
  const popup = host.create("rect");
  host.setProp(popup, "width", "30"); host.setProp(popup, "height", "30");
  const button = host.create("button");
  host.setProp(button, "width", "20"); host.setProp(button, "height", "20");
  host.setProp(button, "disabled", "true");
  let disposed = 0;
  host.onDispose(button, () => disposed++);
  host.applyOps([{ op: "append", parent: popup, child: button }, { op: "openOverlay", id: popup, anchor, side: "below" }]);
  assert.equal(host.clickAt(100, 100, 5, 25), null);
  assert.equal(disposed, 0);
  host.applyOps([{ op: "remove", id: popup }]);
  assert.equal(disposed, 1);
  assert.equal(host.clickAt(100, 100, 5, 5), anchor);
});

test("wasm select preserves option ids and emits selected value", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const values = [];
  mount(jsx("window", { padding: 0, children: jsx("select", { name: "Size", onInput: value => values.push(value), children: [
    jsx("menu_item", { checked: true, children: "Small" }),
    jsx("menu_item", { disabled: true, children: "Medium" }),
    jsx("menu_item", { children: "Large" }),
  ] }) }), host);
  assert.equal(host.clickAt(240, 200, 10, 10), 2);
  assert.equal(host.sliderKey("ArrowDown"), true);
  assert.equal(host.activate(), true);
  assert.deepEqual(values, ["Large"]);
  assert.equal(host.clickAt(240, 200, 10, 10), 2);
  assert.equal(host.sliderKey("ArrowUp"), true);
  assert.equal(host.activate(), true);
  assert.deepEqual(values, ["Large", "Small"]);
});

test("wasm menu activates callback before disposing its item", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const root = mount(jsx("window", { padding: 0, children: jsx("button", { children: "Actions" }) }), host);
  const events = [];
  const popup = mount(jsx("menu", { name: "Actions", children: [
    jsx("menu_item", { children: "Open", onClick: () => events.push("open") }),
    jsx("menu_item", { children: "Close", onClick: () => events.push("close") }),
  ] }), host);
  host.onDispose(5, () => events.push("disposed"));
  assert.equal(host.clickAt(320, 240, 5, 5), 2);
  host.openOverlay(popup, 2, "below");
  assert.equal(host.sliderKey("ArrowDown"), true);
  assert.equal(host.activate(), true);
  assert.deepEqual(events, ["close", "disposed"]);
  assert.equal(host.activate(), true); // Focus returned to the anchor button.
  assert.deepEqual(events, ["close", "disposed"]);
  assert.equal(root, 1);
});

test("wasm mounts a centered modal and dismisses with Escape", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const root = mount(jsx("window", { padding: 0, children: jsx("button", { children: "Open" }) }), host);
  const popup = mount(jsx("dialog", { name: "Confirm", width: 100, height: 80, children: jsx("button", { children: "OK" }) }), host);
  host.openOverlay(popup, root, "center");
  const modal = host.renderPng(240, 160);
  assert.equal(host.clickAt(240, 160, 1, 1), null);
  assert.notDeepEqual(host.renderPng(240, 160), modal);
  const second = mount(jsx("dialog", { name: "Again", width: 100, height: 80 }), host);
  host.openOverlay(second, root, "center");
  assert.equal(host.escape(), true);
  assert.equal(host.escape(), false);
});

test("wasm radio options select exclusively with pointer and arrows", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  mount(jsx("window", { padding: 0, children: jsx("radio_group", { name: "Size", children: [
    jsx("radio", { checked: true, children: "Small" }),
    jsx("radio", { children: "Large" }),
  ] }) }), host);
  assert.equal(host.clickAt(240, 100, 10, 10), 3);
  const before = host.renderPng(240, 100);
  assert.equal(host.sliderKey("ArrowDown"), true);
  const after = host.renderPng(240, 100);
  assert.notDeepEqual(before, after);
  assert.equal(host.sliderKey("ArrowUp"), true);
  assert.deepEqual(host.renderPng(240, 100), before);
});

test("wasm context menus reopen and retain callbacks", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  let clicks = 0, cleanups = 0;
  mount(jsx("window", { padding: 0, children: jsx("button", { children: "Target", contextMenu:
    jsx("menu", { name: "Actions", children: jsx("menu_item", { children: "Open", onClick: () => clicks++ }) }) }) }), host);
  // IDs: window 1, target 2, menu 3, item 4.
  host.onDispose(4, () => cleanups++);
  assert.equal(host.contextMenuAt(320, 200, 10, 10), 2);
  assert.equal(host.activate(), true);
  assert.equal(clicks, 1);
  assert.equal(cleanups, 0);
  assert.equal(host.contextMenuAt(320, 200, 10, 10), 2);
  assert.equal(host.activate(), true);
  assert.equal(clicks, 2);
  assert.equal(cleanups, 0);
  host.remove(2);
  assert.equal(cleanups, 1);
});

test("conditional tab panels are removed and recreated with their owners", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const [shown, setShown] = createSignal(true);
  let disposed = 0;
  const panel = () => jsx("tab_list", { name: "Pages", children: jsx("tab", { checked: true, children: "First",
    panel: jsx("tab_panel", { name: "Panel", children: jsx("button", { children: "Inside", onClick: () => {} }) }) }) });
  const root = mount(jsx("window", { padding: 0, children: jsx(Show, { when: shown, children: panel }) }), host, { createEffect, createSignal, untrack });
  const image = host.renderPng(320, 180);
  // root, Show group, For group, tab list, tab, panel, button.
  host.onDispose(7, () => disposed++);
  setShown(false);
  assert.equal(disposed, 1);
  const empty = host.renderPng(320, 180);
  assert.notDeepEqual(empty, image);
  setShown(true);
  assert.deepEqual(host.renderPng(320, 180), image);
  host.remove(root);
});

test("wasm tab panels preserve their content when selection changes", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const selected = [];
  mount(jsx("window", { padding: 0, children: jsx("tab_list", { name: "Pages", children: [
    jsx("tab", { checked: true, children: "First", panel: jsx("tab_panel", { name: "First panel", children: jsx("text", { children: "First content" }) }) }),
    jsx("tab", { children: "Second", onClick: () => selected.push("second"), panel: jsx("tab_panel", { name: "Second panel", children: jsx("text", { children: "Second content" }) }) }),
  ] }) }), host);
  assert.equal(host.clickAt(320, 180, 10, 10), 3);
  const first = host.renderPng(320, 180);
  assert.equal(host.sliderKey("ArrowRight"), true);
  assert.deepEqual(selected, ["second"]);
  assert.notDeepEqual(host.renderPng(320, 180), first);
  assert.equal(host.sliderKey("ArrowLeft"), true);
  assert.deepEqual(host.renderPng(320, 180), first);
});

test("wasm tabs deliver retained callbacks for arrow selection", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const selected = [];
  mount(jsx("window", { padding: 0, children: jsx("tab_list", { name: "Pages", children: [
    jsx("tab", { checked: true, children: "First", onClick: () => selected.push("first") }),
    jsx("tab", { children: "Second", onClick: () => selected.push("second") }),
  ] }) }), host);
  assert.equal(host.clickAt(320, 100, 10, 10), 3);
  selected.length = 0;
  assert.equal(host.sliderKey("ArrowRight"), true);
  assert.deepEqual(selected, ["second"]);
  assert.equal(host.sliderKey("ArrowLeft"), true);
  assert.deepEqual(selected, ["second", "first"]);
});

test("wasm slider drag and keyboard use the shared state", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  mount(jsx("window", { padding: 0, children: jsx("slider", { name: "Volume", value: 0.5 }) }), host);
  const middle = host.renderPng(240, 100);
  assert.equal(host.sliderBegin(240, 100, 80, 10), 2);
  assert.equal(host.sliderMove(2, 400), true);
  const end = host.renderPng(240, 100);
  assert.notDeepEqual(end, middle);
  assert.equal(host.sliderKey("Home"), true);
  assert.notDeepEqual(host.renderPng(240, 100), end);
  host.remove(2);
  assert.equal(host.sliderMove(2, 10), false);
});

test("wasm renders and patches progress", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  mount(jsx("window", { padding: 0, children: jsx("progress", { name: "Upload", value: 0.25 }) }), host);
  const before = host.renderPng(240, 100);
  host.setProp(2, "value", "1");
  assert.notDeepEqual(host.renderPng(240, 100), before);
  assert.throws(() => host.setProp(2, "value", "NaN"));
});

test("wasm mounts and activates a switch", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  mount(jsx("window", { padding: 0, children: jsx("switch", { checked: false, children: "Alerts" }) }), host);
  const before = host.renderPng(240, 100);
  assert.equal(host.clickAt(240, 100, 5, 10), 2);
  assert.notDeepEqual(host.renderPng(240, 100), before);
  host.setProp(2, "disabled", "true");
  assert.equal(host.clickAt(240, 100, 5, 10), null);
});

test("wasm host opens and dismisses an anchored overlay", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const root = host.create("window");
  host.setProp(root, "padding", "0");
  const anchor = host.create("button");
  host.setProp(anchor, "width", "20"); host.setProp(anchor, "height", "20");
  host.append(root, anchor);
  const popup = host.create("rect");
  host.setProp(popup, "width", "20"); host.setProp(popup, "height", "20");
  host.openOverlay(popup, anchor, "below");
  assert.equal(host.clickAt(100, 100, 5, 25), popup);
  assert.equal(host.clickAt(100, 100, 5, 5), null);
  assert.equal(host.clickAt(100, 100, 5, 5), anchor);
  const popup2 = host.create("rect");
  host.openOverlay(popup2, anchor, "right");
  assert.equal(host.escape(), true);
  assert.equal(host.escape(), false);
  assert.throws(() => host.openOverlay(popup, anchor, "diagonal"), /side/);
});

test("fallback font config uses isolated module lifetime", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const moduleBytes = readFileSync(wasmPath);
  const font = readFileSync(fileURLToPath(new URL("../../crates/tekto-text/fonts/Roboto-Regular.ttf", import.meta.url)));
  const host = await connectWasm(moduleBytes);
  assert.throws(() => host.setFallbackFont(new Uint8Array([0, 1, 2])), /no usable fonts/);
  host.reset();
  host.setFallbackFont(font);
  assert.throws(() => host.setFallbackFont(font), /already initialized/);
  host.reset();
  assert.throws(() => host.setFallbackFont(font), /already initialized/);
  const id = host.create("text");
  host.setText(id, "Font configuration");
  assert.deepEqual(Array.from(host.renderPng(120, 40).slice(0, 8)), [137, 80, 78, 71, 13, 10, 26, 10]);

  const late = await connectWasm(moduleBytes);
  const lateId = late.create("text");
  late.setText(lateId, "Initialize text");
  late.renderPng(120, 40);
  late.reset();
  assert.throws(() => late.setFallbackFont(font), /already initialized/);
});

test("optional supplied CJK font renders distinct missing glyphs", async (t) => {
  if (!process.env.TEKTO_TEST_CJK_FONT) return;
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const moduleBytes = readFileSync(wasmPath);
  async function render(text, font) {
    const host = await connectWasm(moduleBytes);
    if (font) host.setFallbackFont(font);
    const id = host.create("text");
    host.setText(id, text);
    return host.renderPng(80, 40);
  }
  const font = readFileSync(process.env.TEKTO_TEST_CJK_FONT);
  const defaultA = await render("漢");
  const defaultB = await render("字");
  assert.deepEqual(defaultA, defaultB);
  assert.notDeepEqual(await render("漢", font), await render("字", font));
});

test("font host copies aliased bytes across memory growth and reports errors", () => {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const input = new Uint8Array(memory.buffer, 10, 3);
  input.set([1, 2, 3]);
  let output = new TextEncoder().encode('{"ok":true}');
  let seen;
  const exports = {
    memory,
    tekto_alloc() { memory.grow(1); return 100; },
    tekto_set_fallback_font(ptr, len) {
      seen = Array.from(new Uint8Array(memory.buffer, ptr, len));
      new Uint8Array(memory.buffer, 200, output.length).set(output);
    },
    tekto_apply_ops() {}, tekto_render_png() {}, tekto_click_at() {}, tekto_context_menu_at() {}, tekto_field_begin() {}, tekto_field_move() {}, tekto_escape() {}, tekto_overlay_id() {}, tekto_field_input() {}, tekto_field_ime() {}, tekto_field_key() {}, tekto_field_selection() {}, tekto_focused_field() {}, tekto_slider_begin() {}, tekto_slider_move() {}, tekto_slider_key() {}, tekto_activate_focused() {}, tekto_open_select_state() {}, tekto_select_value() {}, tekto_scroll_at() {}, tekto_reset() {},
    tekto_out_ptr() { return 200; }, tekto_out_len() { return output.length; },
  };
  const host = hostFromExports(exports);
  host.setFallbackFont(input);
  assert.deepEqual(seen, [1, 2, 3]);
  output = new TextEncoder().encode('{"ok":false,"error":"bad font"}');
  assert.throws(() => host.setFallbackFont(new Uint8Array([4])), /bad font/);
  assert.throws(() => host.setFallbackFont([1]), TypeError);
});

/** In-memory canvas. No display and no DOM. */
function fakeCanvas(width, height) {
  const frames = [];
  const canvas = {
    width,
    height,
    getContext() {
      return {
        createImageData(w, h) {
          return { width: w, height: h, data: new Uint8ClampedArray(w * h * 4) };
        },
        putImageData(imageData) {
          frames.push(imageData);
        },
      };
    },
  };
  return { canvas, frames };
}

test("notes at 640x400 matches the headless PNG and blits on resize", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const referencePath = fileURLToPath(
    new URL("../../crates/tekto/tests/snapshots/notes-headless.png", import.meta.url),
  );
  const expected = readFileSync(referencePath);
  const host = await connectWasm(readFileSync(wasmPath));
  host.reset();
  host.applyOps(renderToOps(Notes()));

  const png = host.renderPng(640, 400);
  assert.deepEqual(Buffer.from(png), expected);

  const { canvas, frames } = fakeCanvas(1, 1);
  const decoded = await blitPng(canvas, png);
  assert.equal(decoded.width, 640);
  assert.equal(decoded.height, 400);
  assert.equal(frames.length, 1);
  assert.equal(frames[0].data.length, 640 * 400 * 4);
  // Page fill #f4f3f0 is the top-left pixel of the headless Notes shot.
  assert.equal(frames[0].data[0], 0xf4);
  assert.equal(frames[0].data[1], 0xf3);
  assert.equal(frames[0].data[2], 0xf0);
  assert.equal(frames[0].data[3], 0xff);

  const narrow = await resizeCanvas(host, canvas, 393, 240);
  assert.equal(narrow[0], 0x89);
  assert.notEqual(narrow.length, png.length);
  assert.equal(canvas.width, 393);
  assert.equal(canvas.height, 240);
  assert.equal(frames.at(-1).width, 393);
  assert.equal(frames.at(-1).height, 240);

  const roundTrip = await decodePng(png);
  assert.equal(roundTrip.data.length, expected.length > 0 ? 640 * 400 * 4 : 0);
  assert.equal(roundTrip.data[0], 0xf4);
});

test("WebGPU feature detect falls back to the CPU blit", async () => {
  assert.deepEqual(choosePaintBackend({ gpuFlag: false, webgpu: true, webgl2: true }), {
    backend: "cpu",
    reason: "flag-off",
  });
  assert.deepEqual(choosePaintBackend({ gpuFlag: true, webgpu: true, webgl2: true }), {
    backend: "webgpu",
    reason: "",
  });
  assert.deepEqual(choosePaintBackend({ gpuFlag: true, webgpu: false, webgl2: true }), {
    backend: "cpu",
    reason: "webgl2-no-compute",
  });
  assert.deepEqual(choosePaintBackend({ gpuFlag: true, webgpu: false, webgl2: false }), {
    backend: "cpu",
    reason: "no-adapter",
  });

  assert.equal(await detectWebGpu(undefined), false);
  assert.equal(await detectWebGpu({ gpu: {} }), false);
  assert.equal(
    await detectWebGpu({
      gpu: {
        requestAdapter: async () => null,
      },
    }),
    false,
  );
  assert.equal(
    await detectWebGpu({
      gpu: {
        requestAdapter: async () => {
          throw new Error("blocked");
        },
      },
    }),
    false,
  );
  assert.equal(
    await detectWebGpu({
      gpu: {
        requestAdapter: async () => ({ name: "stub" }),
      },
    }),
    true,
  );

  const contexts = [];
  const canvas = {
    width: 8,
    height: 4,
    getContext(kind) {
      contexts.push(kind);
      if (kind === "webgl2") return { stub: true };
      if (kind !== "2d") return null;
      return {
        createImageData(w, h) {
          return { width: w, height: h, data: new Uint8ClampedArray(w * h * 4) };
        },
        putImageData(imageData) {
          canvas.frames.push(imageData);
        },
      };
    },
    frames: [],
  };
  assert.equal(detectWebGl2(canvas), true);
  assert.equal(detectWebGl2({ getContext: () => null }), false);

  const host = {
    renderPng() {
      // 1×1 RGBA PNG. The blit only needs a real CPU-renderer payload.
      return PNG_1X1;
    },
  };

  const flaggedOff = await paintPreferred(host, canvas, {
    gpu: false,
    detect: { webgpu: true, webgl2: true },
    width: 2,
    height: 2,
  });
  assert.equal(flaggedOff.used, "cpu");
  assert.equal(flaggedOff.reason, "flag-off");
  assert.equal(flaggedOff.png[0], 0x89);

  const noExport = await paintPreferred(host, canvas, {
    gpu: true,
    detect: { webgpu: true, webgl2: false },
    width: 2,
    height: 2,
  });
  assert.equal(noExport.used, "cpu");
  assert.equal(noExport.reason, "no-paint-export");

  const webglOnly = await paintPreferred(host, canvas, {
    gpu: true,
    detect: { webgpu: false, webgl2: true },
    width: 2,
    height: 2,
  });
  assert.equal(webglOnly.used, "cpu");
  assert.equal(webglOnly.reason, "webgl2-no-compute");

  let gpuCalls = 0;
  const gpuHost = {
    ...host,
    async paintWebGpu(target, w, h) {
      gpuCalls += 1;
      target.width = w;
      target.height = h;
    },
  };
  const gpu = await paintPreferred(gpuHost, canvas, {
    gpu: true,
    detect: { webgpu: true, webgl2: false },
    width: 3,
    height: 3,
  });
  assert.equal(gpu.used, "webgpu");
  assert.equal(gpuCalls, 1);
  assert.equal(canvas.width, 3);

  const broke = await paintPreferred(
    {
      ...host,
      async paintWebGpu() {
        throw new Error("device lost");
      },
    },
    canvas,
    { gpu: true, detect: { webgpu: true }, width: 2, height: 2 },
  );
  assert.equal(broke.used, "cpu");
  assert.equal(broke.reason, "webgpu-failed");
  assert.match(broke.error, /device lost/);
  assert.ok(canvas.frames.length > 0);
});

/**
 * One context type per canvas. A second type returns null, as in HTML.
 * `webgl2` / `webgpu` only say the type is available, not that it is current.
 */
function strictCanvas(doc, { webgl2 = false, webgpu = false } = {}) {
  let locked = null;
  const canvas = {
    width: 8,
    height: 4,
    id: "",
    className: "",
    style: { cssText: "" },
    ownerDocument: doc,
    parentNode: null,
    frames: [],
    calls: [],
    getContext(kind) {
      canvas.calls.push(kind);
      if (locked && locked !== kind) return null;
      if (kind === "2d") {
        locked = "2d";
        return {
          createImageData(w, h) {
            return { width: w, height: h, data: new Uint8ClampedArray(w * h * 4) };
          },
          putImageData(imageData) {
            canvas.frames.push(imageData);
          },
        };
      }
      if (kind === "webgl2" && webgl2) {
        locked = "webgl2";
        return { kind: "webgl2" };
      }
      if (kind === "webgpu" && webgpu) {
        locked = "webgpu";
        return { kind: "webgpu" };
      }
      return null;
    },
  };
  return canvas;
}

function strictDocument() {
  const doc = {
    createElement(tag) {
      if (tag !== "canvas") return null;
      doc.created += 1;
      // Detached probes and replacements can take WebGL2. They start unlocked.
      return strictCanvas(doc, { webgl2: true, webgpu: true });
    },
    created: 0,
  };
  const body = {
    childNodes: [],
    replaceChild(next, prev) {
      const at = body.childNodes.indexOf(prev);
      if (at < 0) throw new Error("canvas is not mounted");
      body.childNodes[at] = next;
      prev.parentNode = null;
      next.parentNode = body;
    },
  };
  doc.body = body;
  return doc;
}

function mountStrict(doc, canvas) {
  canvas.ownerDocument = doc;
  canvas.parentNode = doc.body;
  doc.body.childNodes.push(canvas);
  return canvas;
}

test("strict canvas falls back to a 2D blit without committing the paint canvas", async () => {
  const host = {
    renderPng() {
      return PNG_1X1;
    },
  };
  const doc = strictDocument();
  const paint = mountStrict(doc, strictCanvas(doc, { webgl2: true, webgpu: true }));
  paint.id = "view";

  const flaggedOff = await paintPreferred(host, paint, {
    gpu: false,
    env: {
      document: doc,
      gpu: { requestAdapter: async () => ({ name: "stub" }) },
    },
    width: 2,
    height: 2,
  });
  assert.equal(flaggedOff.used, "cpu");
  assert.equal(flaggedOff.reason, "flag-off");
  assert.equal(flaggedOff.png[0], 0x89);
  assert.deepEqual(paint.calls, ["2d"]);
  assert.equal(paint.frames.length, 1);
  assert.equal(doc.created, 0);
  assert.equal(doc.body.childNodes[0], paint);

  const missingAdapter = await paintPreferred(host, paint, {
    gpu: true,
    env: {
      document: doc,
      gpu: { requestAdapter: async () => null },
    },
    width: 2,
    height: 2,
  });
  assert.equal(missingAdapter.used, "cpu");
  assert.equal(missingAdapter.reason, "webgl2-no-compute");
  assert.equal(missingAdapter.surface, paint);
  assert.ok(missingAdapter.png);
  assert.deepEqual(paint.calls, ["2d", "2d"]);
  assert.equal(doc.created, 1);
  assert.equal(paint.frames.length, 2);

  const noExport = await paintPreferred(host, paint, {
    gpu: true,
    env: {
      document: doc,
      gpu: { requestAdapter: async () => ({ name: "stub" }) },
    },
    width: 2,
    height: 2,
  });
  assert.equal(noExport.used, "cpu");
  assert.equal(noExport.reason, "no-paint-export");
  assert.equal(noExport.surface, paint);
  assert.deepEqual(paint.calls, ["2d", "2d", "2d"]);
  assert.equal(paint.frames.length, 3);

  const gpuPaint = mountStrict(doc, strictCanvas(doc, { webgl2: true, webgpu: true }));
  gpuPaint.id = "view";
  const createdBeforeFail = doc.created;
  const failed = await paintPreferred(
    {
      ...host,
      async paintWebGpu(target) {
        const gpuCtx = target.getContext("webgpu");
        if (!gpuCtx) throw new Error("expected a webgpu context");
        throw new Error("device lost");
      },
    },
    gpuPaint,
    {
      gpu: true,
      env: {
        document: doc,
        gpu: { requestAdapter: async () => ({ name: "stub" }) },
      },
      width: 2,
      height: 2,
    },
  );
  assert.equal(failed.used, "cpu");
  assert.equal(failed.reason, "webgpu-failed");
  assert.match(failed.error, /device lost/);
  assert.ok(failed.png);
  assert.notEqual(failed.surface, gpuPaint);
  assert.equal(failed.surface, doc.body.childNodes[1]);
  assert.equal(failed.surface.id, "view");
  assert.equal(gpuPaint.id, "");
  assert.equal(gpuPaint.frames.length, 0);
  assert.equal(failed.surface.frames.length, 1);
  assert.deepEqual(failed.surface.calls, ["2d"]);
  assert.deepEqual(gpuPaint.calls, ["webgpu", "2d"]);
  assert.equal(doc.created, createdBeforeFail + 1);
});

const PNG_1X1 = new Uint8Array([
  137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0,
  0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0, 5, 0, 1,
  255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
]);


test("canvas captures slider drags and suppresses the click", () => {
  const listeners = new Map(), moves = [];
  let focus = 0, captures = 0, clicks = 0, paints = 0;
  const canvas = { width: 100, height: 100,
    focus: () => focus++, setPointerCapture: () => captures++,
    getBoundingClientRect: () => ({ left: 0, top: 0, width: 100, height: 100 }),
    addEventListener: (name, fn) => listeners.set(name, fn),
    removeEventListener: name => listeners.delete(name),
  };
  const host = { sliderBegin: () => 7, sliderMove: (id, x) => (moves.push([id, x]), true), clickAt: () => clicks++, scrollAt: () => null };
  const dispose = attachCanvasInput(host, canvas, () => paints++);
  listeners.get("pointerdown")({ button: 0, pointerId: 3, clientX: 10, clientY: 10, preventDefault() {} });
  listeners.get("pointermove")({ pointerId: 3, clientX: 150, clientY: 10 });
  listeners.get("pointerup")({ type: "pointerup", pointerId: 3 });
  listeners.get("click")({ button: 0, clientX: 10, clientY: 10 });
  assert.deepEqual(moves, [[7, 150]]); assert.equal(captures, 1); assert.equal(focus, 1);
  assert.equal(clicks, 0); assert.equal(paints, 2);
  dispose(); assert.equal(listeners.size, 0);
});

test("canvas routes Escape only when an overlay closes", () => {
  const listeners = new Map();
  const canvas = { width: 100, height: 100,
    getBoundingClientRect: () => ({ left: 0, top: 0, width: 100, height: 100 }),
    addEventListener: (name, fn) => listeners.set(name, fn),
    removeEventListener: (name) => listeners.delete(name),
  };
  let open = true, paints = 0, prevented = 0;
  const host = { clickAt: () => null, scrollAt: () => null, escape: () => { const wasOpen = open; open = false; return wasOpen; } };
  const dispose = attachCanvasInput(host, canvas, () => paints++);
  const event = { key: "Escape", preventDefault: () => prevented++ };
  listeners.get("keydown")(event);
  listeners.get("keydown")(event);
  assert.equal(paints, 1); assert.equal(prevented, 1);
  dispose(); assert.equal(listeners.size, 0);
});

test("canvas repaints after an outside click closes an overlay", () => {
  const listeners = new Map();
  const canvas = { width: 100, height: 100,
    getBoundingClientRect: () => ({ left: 0, top: 0, width: 100, height: 100 }),
    addEventListener: (name, fn) => listeners.set(name, fn),
    removeEventListener: (name) => listeners.delete(name),
  };
  let paints = 0;
  const dispose = attachCanvasInput({ clickAt: () => null, scrollAt: () => null }, canvas, () => paints++);
  listeners.get("click")({ button: 0, clientX: 10, clientY: 10 });
  assert.equal(paints, 1);
  dispose();
});

test("canvas input scales coordinates, wheel units and cleans up", () => {
  const listeners = new Map();
  const canvas = { width: 400, height: 200,
    getBoundingClientRect: () => ({ left: 10, top: 20, width: 200, height: 100 }),
    addEventListener: (name, fn) => listeners.set(name, fn),
    removeEventListener: (name, fn) => { if (listeners.get(name) === fn) listeners.delete(name); },
  };
  const calls = [];
  const host = { clickAt: (...args) => (calls.push(args), 2), scrollAt: (...args) => (calls.push(args), 0) };
  let paints = 0, prevented = 0;
  const dispose = attachCanvasInput(host, canvas, () => paints++);
  listeners.get("click")({ button: 0, clientX: 60, clientY: 45 });
  for (const [deltaMode, deltaY] of [[0, 5], [1, 2], [2, 1]]) {
    listeners.get("wheel")({ clientX: 60, clientY: 45, deltaMode, deltaY, preventDefault: () => prevented++ });
  }
  assert.deepEqual(calls, [[400, 200, 100, 50], [400, 200, 100, 50, 10], [400, 200, 100, 50, 96], [400, 200, 100, 50, 200]]);
  canvas.width = 200; canvas.height = 100;
  listeners.get("click")({ button: 0, clientX: 60, clientY: 45 });
  assert.deepEqual(calls.at(-1), [200, 100, 50, 25]);
  assert.equal(paints, 5); assert.equal(prevented, 3);
  dispose(); assert.equal(listeners.size, 0);
});

test("wasm input hits and scrolls the rendered tree", async (t) => {
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
  const host = await connectWasm(readFileSync(wasmPath));
  const button = host.create("button");
  host.setProp(button, "width", "100"); host.setProp(button, "height", "40");
  host.renderPng(100, 80);
  assert.equal(host.clickAt(100, 80, 10, 10), button);
  host.reset();
  const scroll = host.create("scroll");
  host.setProp(scroll, "width", "100"); host.setProp(scroll, "height", "80");
  const child = host.create("rect");
  host.setProp(child, "width", "100"); host.setProp(child, "height", "300");
  host.append(scroll, child);
  host.renderPng(100, 80);
  assert.equal(host.scrollAt(100, 80, 10, 10, 30), 30);
  host.renderPng(100, 80);
  assert.equal(host.scrollAt(100, 80, 10, 10, 10000), 220);
  assert.equal(host.scrollAt(100, 80, 10, 10, -10000), 0);
});
