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
    tekto_reset: resetExport,
    tekto_out_ptr: outPtr,
    tekto_out_len: outLen,
  } = exports;
  if (
    !memory ||
    typeof alloc !== "function" ||
    typeof applyOpsExport !== "function" ||
    typeof setFallbackFontExport !== "function" ||
    typeof renderExport !== "function"
  ) {
    throw new Error("tekto wasm exports are missing (rebuild tekto-wasm)");
  }

  function applyOps(ops) {
    const bytes = textEncoder.encode(JSON.stringify(ops));
    const ptr = alloc(bytes.length);
    new Uint8Array(memory.buffer).set(bytes, ptr);
    applyOpsExport(ptr, bytes.length);
    const body = JSON.parse(textDecoder.decode(readOut(memory, outPtr, outLen)));
    if (!body.ok) {
      throw new Error(body.error || "apply ops failed");
    }
    return body.created;
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
      resetExport();
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
