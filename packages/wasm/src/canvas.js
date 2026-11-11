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
  function point(event) {
    const rect = canvas.getBoundingClientRect();
    if (!rect.width || !rect.height || !canvas.width || !canvas.height) return null;
    return [(event.clientX - rect.left) * canvas.width / rect.width,
      (event.clientY - rect.top) * canvas.height / rect.height];
  }
  function click(event) {
    if (event.button !== 0) return;
    const p = point(event);
    if (p && host.clickAt(canvas.width, canvas.height, ...p) !== null) repaint();
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
  canvas.addEventListener("click", click);
  canvas.addEventListener("wheel", wheel, { passive: false });
  return () => {
    canvas.removeEventListener("click", click);
    canvas.removeEventListener("wheel", wheel);
  };
}
