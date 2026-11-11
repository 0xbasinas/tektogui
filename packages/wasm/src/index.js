/**
 * Talk to the `tekto-wasm` module with the same HostOp JSON as desktop.
 * The browser (or Node) runs the app. This file does not embed QuickJS
 * and does not expose DOM types.
 */

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

function readOut(memory, outPtr, outLen) {
  const start = outPtr();
  const len = outLen();
  return new Uint8Array(memory.buffer.slice(start, start + len));
}

/**
 * Build a host from an instantiated `tekto-wasm` module.
 * `exports` must include `memory` and the `tekto_*` functions.
 */
export function hostFromExports(exports) {
  const {
    memory,
    tekto_alloc: alloc,
    tekto_apply_ops: applyOpsExport,
    tekto_set_fallback_font: setFallbackFontExport,
    tekto_render_png: renderExport,
    tekto_click_at: clickExport,
    tekto_context_menu_at: contextMenuExport,
    tekto_field_begin: fieldBeginExport,
    tekto_field_move: fieldMoveExport,
    tekto_escape: escapeExport,
    tekto_overlay_id: overlayIdExport,
    tekto_field_input: fieldInputExport,
    tekto_field_ime: fieldImeExport,
    tekto_field_key: fieldKeyExport,
    tekto_field_selection: fieldSelectionExport,
    tekto_focused_field: focusedFieldExport,
    tekto_slider_begin: sliderBeginExport,
    tekto_slider_move: sliderMoveExport,
    tekto_slider_key: sliderKeyExport,
    tekto_activate_focused: activateExport,
    tekto_open_select_state: selectStateExport,
    tekto_select_value: selectValueExport,
    tekto_scroll_at: scrollExport,
    tekto_reset: resetExport,
    tekto_out_ptr: outPtr,
    tekto_out_len: outLen,
  } = exports;
  if (
    !memory ||
    typeof alloc !== "function" ||
    typeof applyOpsExport !== "function" ||
    typeof setFallbackFontExport !== "function" ||
    typeof renderExport !== "function" ||
    typeof clickExport !== "function" ||
    typeof contextMenuExport !== "function" ||
    typeof fieldBeginExport !== "function" ||
    typeof fieldMoveExport !== "function" ||
    typeof escapeExport !== "function" ||
    typeof overlayIdExport !== "function" ||
    typeof fieldInputExport !== "function" ||
    typeof fieldImeExport !== "function" ||
    typeof fieldKeyExport !== "function" ||
    typeof fieldSelectionExport !== "function" ||
    typeof focusedFieldExport !== "function" ||
    typeof sliderBeginExport !== "function" ||
    typeof sliderMoveExport !== "function" ||
    typeof sliderKeyExport !== "function" ||
    typeof activateExport !== "function" ||
    typeof selectStateExport !== "function" ||
    typeof selectValueExport !== "function" ||
    typeof scrollExport !== "function"
  ) {
    throw new Error("tekto wasm exports are missing (rebuild tekto-wasm)");
  }

  function applyOps(ops) {
    const created = [];
    for (const op of ops) {
      const bytes = textEncoder.encode(JSON.stringify([op]));
      const ptr = alloc(bytes.length);
      new Uint8Array(memory.buffer).set(bytes, ptr);
      applyOpsExport(ptr, bytes.length);
      const body = JSON.parse(textDecoder.decode(readOut(memory, outPtr, outLen)));
      if (!body.ok) throw new Error(body.error || "apply ops failed");
      created.push(...body.created);
      if (op.op === "append") {
        for (const ids of children.values()) ids.delete(op.child);
        if (!children.has(op.parent)) children.set(op.parent, new Set());
        children.get(op.parent).add(op.child);
      } else if (op.op === "bindTabPanel") {
        for (const ids of children.values()) ids.delete(op.panel);
        if (!children.has(op.tab)) children.set(op.tab, new Set());
        children.get(op.tab).add(op.panel);
      } else if (op.op === "bindContextMenu") {
        contextMenus.add(op.menu);
        if (!children.has(op.target)) children.set(op.target, new Set());
        children.get(op.target).add(op.menu);
      } else if (op.op === "remove") {
        try { forget(op.id); } finally { syncOverlay(); }
        continue;
      }
      syncOverlay();
    }
    return created;
  }

  const clicks = new Map(), inputs = new Map(), cleanups = new Map(), children = new Map(), contextMenus = new Set();
  let overlay = null, imeTarget = null;
  function forget(id) {
    let firstError;
    function visit(node) {
      if (overlay === node) overlay = null;
      const descendants = [...(children.get(node) || [])];
      children.delete(node); clicks.delete(node); inputs.delete(node); contextMenus.delete(node);
      for (const ids of children.values()) ids.delete(node);
      const callbacks = cleanups.get(node) || [];
      cleanups.delete(node);
      for (const child of descendants) visit(child);
      for (const callback of callbacks) {
        try { callback(); } catch (error) { firstError ??= error; }
      }
    }
    visit(id);
    if (firstError) throw firstError;
  }
  function syncOverlay() {
    const current = overlayIdExport() || null;
    if (overlay !== null && overlay !== current) {
      const previous = overlay;
      overlay = current;
      if (!contextMenus.has(previous)) forget(previous);
    } else {
      overlay = current;
    }
  }
  function selectState() {
    selectStateExport();
    return JSON.parse(textDecoder.decode(readOut(memory, outPtr, outLen)));
  }
  function deliverSelection(before) {
    if (!before) return;
    selectValueExport(before[0]);
    const value = JSON.parse(textDecoder.decode(readOut(memory, outPtr, outLen)));
    if (value !== null && value !== before[1]) inputs.get(before[0])?.(value);
  }
  function input(text, backspace = false) {
    const bytes = textEncoder.encode(text);
    const ptr = alloc(bytes.length);
    new Uint8Array(memory.buffer).set(bytes, ptr);
    fieldInputExport(ptr, bytes.length, Number(backspace));
    const result = JSON.parse(textDecoder.decode(readOut(memory, outPtr, outLen)));
    if (!result) return false;
    inputs.get(result[0])?.(result[1]);
    return true;
  }

  return {
    /** Apply a HostOp JSON array. Returns created ids. */
    applyOps,
    /** Set a font or collection before text initialization. */
    setFallbackFont(bytes) {
      if (!(bytes instanceof Uint8Array)) throw new TypeError("font bytes must be a Uint8Array");
      // alloc may grow memory and detach a view into the module's memory.
      const copy = new Uint8Array(bytes);
      const ptr = alloc(copy.length);
      new Uint8Array(memory.buffer).set(copy, ptr);
      setFallbackFontExport(ptr, copy.length);
      const body = JSON.parse(textDecoder.decode(readOut(memory, outPtr, outLen)));
      if (!body.ok) throw new Error(body.error || "setting fallback font failed");
    },
    /** Drop the scene. The next create returns id 1. */
    reset() {
      let firstError;
      for (const id of new Set([...children.keys(), ...clicks.keys(), ...inputs.keys(), ...cleanups.keys()])) {
        try { forget(id); } catch (error) { firstError ??= error; }
      }
      overlay = null;
      imeTarget = null;
      resetExport();
      if (firstError) throw firstError;
    },
    /** CPU-renderer PNG bytes for the current tree. */
    renderPng(width, height) {
      renderExport(width, height);
      const bytes = readOut(memory, outPtr, outLen);
      if (bytes.length < 8 || bytes[0] !== 0x89 || bytes[1] !== 0x50) {
        throw new Error(textDecoder.decode(bytes) || "render failed");
      }
      return bytes;
    },
    /** Hit-test in the supplied viewport and deliver a registered click callback. */
    clickAt(width, height, x, y) {
      const before = selectState();
      const id = clickExport(width, height, x, y) || null;
      try { if (id !== null) clicks.get(id)?.(); deliverSelection(before); }
      finally { syncOverlay(); }
      return id;
    },
    contextMenuAt(width, height, x, y) {
      const id = contextMenuExport(width, height, x, y) || null;
      syncOverlay();
      return id;
    },
    /** Dismiss an overlay when Escape is pressed. */
    escape() { const closed = escapeExport() !== 0; syncOverlay(); return closed; },
    fieldBegin(width, height, x, y) {
      return fieldBeginExport(width, height, x, y) || null;
    },
    fieldMove(id, x, y) { return fieldMoveExport(id, x, y) !== 0; },
    fieldClick(id) { clicks.get(id)?.(); },
    sliderBegin(width, height, x, y) { return sliderBeginExport(width, height, x, y) || null; },
    sliderMove(id, x) { return sliderMoveExport(id, x) !== 0; },
    activate(space = false) {
      const before = selectState();
      const id = activateExport(Number(space)) || null;
      try { if (id !== null) clicks.get(id)?.(); deliverSelection(before); }
      finally { syncOverlay(); }
      return id !== null;
    },
    sliderKey(key) {
      const handled = sliderKeyExport(({ ArrowLeft: 1, ArrowRight: 2, Home: 3, End: 4, ArrowUp: 5, ArrowDown: 6 })[key] || 0) !== 0;
      const id = JSON.parse(textDecoder.decode(readOut(memory, outPtr, outLen)));
      try { if (id !== null) clicks.get(id)?.(); }
      finally { syncOverlay(); }
      return handled;
    },
    typeText(text) { return input(String(text)); },
    backspace() { return input("", true); },
    fieldIme(kind, text = "") {
      const code = ({ preedit: 1, commit: 2, cancel: 3 })[kind];
      if (!code) throw new TypeError("unknown IME operation");
      if (kind !== "preedit" && imeTarget === null) return false;
      const bytes = textEncoder.encode(String(text));
      const ptr = alloc(bytes.length);
      new Uint8Array(memory.buffer).set(bytes, ptr);
      fieldImeExport(ptr, bytes.length, code, imeTarget ?? 0);
      const result = JSON.parse(textDecoder.decode(readOut(memory, outPtr, outLen)));
      if (kind !== "preedit") imeTarget = null;
      if (!result) return false;
      if (kind === "preedit") imeTarget = result[0];
      if (result[1] !== null) inputs.get(result[0])?.(result[1]);
      return true;
    },
    fieldKey(key, shift = false) {
      const code = ({ ArrowLeft: shift ? 6 : 1, ArrowRight: shift ? 7 : 2,
        Home: shift ? 8 : 3, End: shift ? 9 : 4, Delete: 5, SelectAll: 10,
        ArrowUp: shift ? 13 : 11, ArrowDown: shift ? 14 : 12, Enter: 15 })[key] || 0;
      fieldKeyExport(code);
      const result = JSON.parse(textDecoder.decode(readOut(memory, outPtr, outLen)));
      if (!result) return false;
      if (result[1] !== null) inputs.get(result[0])?.(result[1]);
      return true;
    },
    fieldSelection() {
      fieldSelectionExport();
      return JSON.parse(textDecoder.decode(readOut(memory, outPtr, outLen)));
    },
    focusedField() { return focusedFieldExport() || null; },
    onClick(id, fn) { clicks.set(id, fn); },
    onInput(id, fn) { inputs.set(id, fn); },
    onDispose(id, fn) { if (!cleanups.has(id)) cleanups.set(id, []); cleanups.get(id).push(fn); },
    /** Scroll in layout pixels; null means no scroll target. */
    scrollAt(width, height, x, y, delta) {
      const result = scrollExport(width, height, x, y, delta);
      return Number.isNaN(result) ? null : result;
    },
    /** Same shape `mount` expects (`@tektogui/jsx-runtime`). */
    create(kind) {
      const created = applyOps([{ op: "create", kind }]);
      return created[0];
    },
    setText(id, text) {
      applyOps([{ op: "setText", id, text: String(text) }]);
    },
    setProp(id, key, value) {
      applyOps([{ op: "setProp", id, key, value: String(value) }]);
    },
    append(parent, child) {
      applyOps([{ op: "append", parent, child }]);

    },
    remove(id) {
      applyOps([{ op: "remove", id }]);
    },
    bindTabPanel(tab, panel) { applyOps([{ op: "bindTabPanel", tab, panel }]); },
    bindContextMenu(target, menu) { applyOps([{ op: "bindContextMenu", target, menu }]); },
    openOverlay(id, anchor, side) {
      applyOps([{ op: "openOverlay", id, anchor, side }]);
    },
    closeOverlay() {
      applyOps([{ op: "closeOverlay" }]);
    },
  };
}

/**
 * Instantiate wasm bytes (or an `ArrayBuffer`) and return the host.
 * A string is fetched as a URL (browser or Node 18+). No DOM types.
 */
export async function connectWasm(source) {
  const bytes = typeof source === "string" ? await (await fetch(source)).arrayBuffer() : source;
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return hostFromExports(instance.exports);
}
