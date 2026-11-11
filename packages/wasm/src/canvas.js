/**
 * Host glue: blit a CPU-renderer PNG onto a canvas.
 *
 * This file is not the app API. App code (`@tektogui/core`,
 * `@tektogui/jsx-runtime`) still has no DOM types. The page that owns
 * the `<canvas>` calls `paintCanvas` / `resizeCanvas`.
 *
 * Paint is the existing tiny-skia PNG from `host.renderPng`. WebGPU, when
 * the host asks for it, lives in `./gpu.js` and falls back to this blit.
 */

const PNG_SIG = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

function readU32(bytes, offset) {
  return (
    (bytes[offset] << 24) |
    (bytes[offset + 1] << 16) |
    (bytes[offset + 2] << 8) |
    bytes[offset + 3]
  ) >>> 0;
}

function concat(chunks) {
  let len = 0;
  for (const chunk of chunks) len += chunk.length;
  const out = new Uint8Array(len);
  let at = 0;
  for (const chunk of chunks) {
    out.set(chunk, at);
    at += chunk.length;
  }
  return out;
}

function paeth(left, up, upLeft) {
  const p = left + up - upLeft;
  const pa = Math.abs(p - left);
  const pb = Math.abs(p - up);
  const pc = Math.abs(p - upLeft);
  if (pa <= pb && pa <= pc) return left;
  if (pb <= pc) return up;
  return upLeft;
}

async function inflateZlib(bytes) {
  if (typeof DecompressionStream !== "function") {
    throw new Error("DecompressionStream is required to blit a PNG");
  }
  const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream("deflate"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

/**
 * Decode an 8-bit RGB or RGBA PNG (the shape `render_tree_png` writes).
 * Returns `{ width, height, data }` with straight RGBA bytes.
 */
export async function decodePng(bytes) {
  const u8 = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  for (let i = 0; i < 8; i++) {
    if (u8[i] !== PNG_SIG[i]) throw new Error("render output is not a PNG");
  }
  let pos = 8;
  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  const idat = [];
  while (pos + 12 <= u8.length) {
    const len = readU32(u8, pos);
    const type = String.fromCharCode(u8[pos + 4], u8[pos + 5], u8[pos + 6], u8[pos + 7]);
    const data = u8.subarray(pos + 8, pos + 8 + len);
    pos += 12 + len;
    if (type === "IHDR") {
      width = readU32(data, 0);
      height = readU32(data, 4);
      bitDepth = data[8];
      colorType = data[9];
    } else if (type === "IDAT") {
      idat.push(data);
    } else if (type === "IEND") {
      break;
    }
  }
  if (!width || !height) throw new Error("PNG is missing IHDR");
  if (bitDepth !== 8 || (colorType !== 6 && colorType !== 2)) {
    throw new Error(`unsupported PNG (depth ${bitDepth}, color ${colorType})`);
  }
  const channels = colorType === 6 ? 4 : 3;
  const raw = await inflateZlib(concat(idat));
  const stride = width * channels;
  const expected = height * (stride + 1);
  if (raw.length < expected) throw new Error("PNG pixel data is short");
  const out = new Uint8ClampedArray(width * height * 4);
  let s = 0;
  let prev = new Uint8Array(stride);
  for (let y = 0; y < height; y++) {
    const filter = raw[s++];
    const row = raw.subarray(s, s + stride);
    s += stride;
    const recon = new Uint8Array(stride);
    for (let i = 0; i < stride; i++) {
      const left = i >= channels ? recon[i - channels] : 0;
      const up = prev[i];
      const upLeft = i >= channels ? prev[i - channels] : 0;
      let value = row[i];
      if (filter === 1) value += left;
      else if (filter === 2) value += up;
      else if (filter === 3) value += (left + up) >> 1;
      else if (filter === 4) value += paeth(left, up, upLeft);
      else if (filter !== 0) throw new Error(`bad PNG filter ${filter}`);
      recon[i] = value & 255;
    }
    for (let x = 0; x < width; x++) {
      const si = x * channels;
      const di = (y * width + x) * 4;
      out[di] = recon[si];
      out[di + 1] = recon[si + 1];
      out[di + 2] = recon[si + 2];
      out[di + 3] = channels === 4 ? recon[si + 3] : 255;
    }
    prev = recon;
  }
  return { width, height, data: out };
}

/**
 * Draw PNG bytes into `canvas` with `putImageData`.
 * `canvas` is any object with `getContext("2d")`, `createImageData`, and
 * `putImageData` (a browser canvas, or a test double). No DOM import.
 */
export async function blitPng(canvas, pngBytes) {
  const ctx = canvas.getContext("2d");
  if (!ctx || typeof ctx.createImageData !== "function" || typeof ctx.putImageData !== "function") {
    throw new Error("canvas 2d context cannot blit pixels");
  }
  const image = await decodePng(pngBytes);
  const imageData = ctx.createImageData(image.width, image.height);
  imageData.data.set(image.data);
  ctx.putImageData(imageData, 0, 0);
  return image;
}

/**
 * Render the current scene with the CPU path and blit it.
 * Uses `width`/`height` when given, otherwise the canvas backing-store size.
 */
export async function paintCanvas(host, canvas, width, height) {
  const w = width ?? canvas.width;
  const h = height ?? canvas.height;
  if (!w || !h) throw new Error("canvas width and height must be non-zero");
  canvas.width = w;
  canvas.height = h;
  const png = host.renderPng(w, h);
  await blitPng(canvas, png);
  return png;
}

/**
 * Basic resize: set the backing store and paint again.
 * Layout runs inside `renderPng`, so the scene follows the new size.
 */
export function resizeCanvas(host, canvas, width, height) {
  return paintCanvas(host, canvas, width, height);
}

/** Attach canvas input. Returns a function that removes the listeners. */
export function attachCanvasInput(host, canvas, repaint = () => paintCanvas(host, canvas)) {
  // Browser host only: IMEs require an editable element, not a canvas.
  const document = canvas.ownerDocument;
  const input = document?.body && document.createElement ? document.createElement("textarea") : null;
  if (input) {
    input.setAttribute("aria-hidden", "true");
    input.setAttribute("autocomplete", "off");
    input.setAttribute("autocapitalize", "off");
    input.setAttribute("spellcheck", "false");
    input.tabIndex = -1;
    Object.assign(input.style, { position: "fixed", width: "1px", height: "1px", opacity: "0.01", padding: "0", border: "0", zIndex: "-1" });
    document.body.appendChild(input);
  }
  function focusInput(event) {
    if (!input) { canvas.focus?.(); return; }
    const rect = canvas.getBoundingClientRect();
    input.style.left = `${Number.isFinite(event?.clientX) ? event.clientX : rect.left}px`;
    input.style.top = `${Number.isFinite(event?.clientY) ? event.clientY : rect.top}px`;
    input.focus({ preventScroll: true });
  }
  function point(event) {
    const rect = canvas.getBoundingClientRect();
    if (!rect.width || !rect.height || !canvas.width || !canvas.height) return null;
    return [(event.clientX - rect.left) * canvas.width / rect.width,
      (event.clientY - rect.top) * canvas.height / rect.height];
  }
  let capture = null, suppressClick = false, composing = false, lastCommit = null;
  function click(event) {
    if (suppressClick) { suppressClick = false; return; }
    if (event.button !== 0) return;
    const p = point(event);
    if (p) {
      const id = host.clickAt(canvas.width, canvas.height, ...p);
      if (id != null && host.focusedField?.() === id) focusInput(event);
      repaint();
    }
  }
  function contextmenu(event) {
    const p = point(event);
    if (!p) return;
    if (host.contextMenuAt?.(canvas.width, canvas.height, ...p) != null) {
      event.preventDefault();
      canvas.focus?.();
    }
    repaint();
  }
  function pointerdown(event) {
    if (capture || event.button !== 0) return;
    const p = point(event);
    if (!p) return;
    const field = event.pointerType === "mouse" ? host.fieldBegin?.(canvas.width, canvas.height, ...p) : null;
    const slider = field == null ? host.sliderBegin?.(canvas.width, canvas.height, ...p) : null;
    const id = field ?? slider;
    if (id != null) {
      capture = { id, pointerId: event.pointerId, field: field != null, origin: p, dragged: false };
      if (field != null) focusInput(event); else canvas.focus?.();
      canvas.setPointerCapture?.(event.pointerId);
      event.preventDefault();
      repaint();
    }
  }
  function pointermove(event) {
    if (!capture || capture.pointerId !== event.pointerId) return;
    const p = point(event);
    if (p && Math.abs(p[0] - capture.origin[0]) + Math.abs(p[1] - capture.origin[1]) >= 4) capture.dragged = true;
    if (capture.field && !capture.dragged) return;
    if (p && (capture.field ? host.fieldMove(capture.id, ...p) : host.sliderMove(capture.id, p[0]))) repaint();
    else capture = null;
  }
  function pointerend(event) {
    if (!capture || capture.pointerId !== event.pointerId) return;
    const completed = capture;
    capture = null;
    if (event.type === "pointerup") {
      if (completed.field && !completed.dragged) host.fieldClick?.(completed.id);
      suppressClick = true;
      setTimeout(() => { suppressClick = false; }, 0);
    }
  }
  function blur(event) {
    if (event?.currentTarget === canvas && event.relatedTarget === input) return;
    capture = null;
    if (composing) { composing = false; if (host.fieldIme?.("cancel")) repaint(); }
  }
  function compositionstart() {
    if (composing) host.fieldIme?.("cancel");
    composing = host.fieldIme?.("preedit", "") === true;
  }
  function compositionupdate(event) {
    if (composing && host.fieldIme?.("preedit", event.data ?? "")) repaint();
  }
  function compositionend(event) {
    if (!composing) return;
    composing = false;
    lastCommit = event.data || null;
    if (host.fieldIme?.(event.data ? "commit" : "cancel", event.data ?? "")) repaint();
    if (input) input.value = "";
  }
  function inputText(event) {
    if (!input || composing || event.isComposing) return;
    const text = event.data ?? input.value;
    input.value = "";
    if (text && lastCommit === text) { lastCommit = null; return; }
    lastCommit = null;
    const handled = event.inputType === "deleteContentBackward" ? host.backspace?.() :
      event.inputType === "deleteContentForward" ? host.fieldKey?.("Delete") :
      text ? host.typeText?.(text) : false;
    if (handled) repaint();
  }
  function wheel(event) {
    const p = point(event);
    if (!p) return;
    const rect = canvas.getBoundingClientRect();
    const delta = event.deltaY * (event.deltaMode === 1 ? 48 : event.deltaMode === 2 ? canvas.height : canvas.height / rect.height);
    if (host.scrollAt(canvas.width, canvas.height, ...p, delta) !== null) {
      event.preventDefault();
      repaint();
    }
  }
  function keydown(event) {
    if (event.key === "Escape" && typeof host.escape === "function" && host.escape()) {
      event.preventDefault();
      repaint();
    } else if (!composing && !event.isComposing && !event.altKey && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a" && host.fieldKey?.("SelectAll")) {
      event.preventDefault();
      repaint();
    } else if (!composing && !event.ctrlKey && !event.altKey && !event.metaKey && !event.isComposing) {
      lastCommit = null;
      if ((event.key === "Enter" || event.key === " ") && host.activate?.(event.key === " ")) { event.preventDefault(); repaint(); return; }
      if (host.sliderKey?.(event.key)) { event.preventDefault(); repaint(); return; }
      if (host.fieldKey?.(event.key, event.shiftKey)) { event.preventDefault(); repaint(); return; }
      const handled = event.key === "Backspace" ? host.backspace?.() :
        ([...event.key].length === 1 ? host.typeText?.(event.key) : false);
      if (handled) { event.preventDefault(); repaint(); }
    }
  }
  function copy(event) {
    const text = host.fieldSelection?.();
    if (text == null || !event.clipboardData) return;
    event.clipboardData.setData("text/plain", text);
    event.preventDefault();
    if (event.type === "cut" && host.fieldKey?.("Delete")) repaint();
  }
  function paste(event) {
    if (!event.clipboardData) return;
    if (host.typeText?.(event.clipboardData.getData("text/plain"))) {
      event.preventDefault();
      repaint();
    }
  }
  canvas.addEventListener("pointerdown", pointerdown);
  canvas.addEventListener("pointermove", pointermove);
  canvas.addEventListener("pointerup", pointerend);
  canvas.addEventListener("pointercancel", pointerend);
  canvas.addEventListener("blur", blur);
  canvas.addEventListener("keydown", keydown);
  canvas.addEventListener("compositionstart", compositionstart);
  canvas.addEventListener("compositionupdate", compositionupdate);
  canvas.addEventListener("compositionend", compositionend);
  canvas.addEventListener("copy", copy);
  canvas.addEventListener("cut", copy);
  canvas.addEventListener("paste", paste);
  canvas.addEventListener("click", click);
  canvas.addEventListener("contextmenu", contextmenu);
  canvas.addEventListener("wheel", wheel, { passive: false });
  if (input) {
    for (const [type, listener] of [["keydown", keydown], ["compositionstart", compositionstart],
      ["compositionupdate", compositionupdate], ["compositionend", compositionend],
      ["input", inputText], ["blur", blur], ["copy", copy], ["cut", copy], ["paste", paste]]) {
      input.addEventListener(type, listener);
    }
  }
  return () => {
    if (composing) host.fieldIme?.("cancel");
    canvas.removeEventListener("pointerdown", pointerdown);
    canvas.removeEventListener("pointermove", pointermove);
    canvas.removeEventListener("pointerup", pointerend);
    canvas.removeEventListener("pointercancel", pointerend);
    canvas.removeEventListener("blur", blur);
    canvas.removeEventListener("keydown", keydown);
    canvas.removeEventListener("compositionstart", compositionstart);
    canvas.removeEventListener("compositionupdate", compositionupdate);
    canvas.removeEventListener("compositionend", compositionend);
    canvas.removeEventListener("copy", copy);
    canvas.removeEventListener("cut", copy);
    canvas.removeEventListener("paste", paste);
    canvas.removeEventListener("click", click);
    canvas.removeEventListener("contextmenu", contextmenu);
    canvas.removeEventListener("wheel", wheel);
    if (input) {
      for (const [type, listener] of [["keydown", keydown], ["compositionstart", compositionstart],
        ["compositionupdate", compositionupdate], ["compositionend", compositionend],
        ["input", inputText], ["blur", blur], ["copy", copy], ["cut", copy], ["paste", paste]]) {
        input.removeEventListener(type, listener);
      }
      input.remove();
    }
  };
}
