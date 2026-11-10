/**
 * Host glue: pick WebGPU or the CPU canvas blit.
 *
 * App code still has no DOM types. The page that owns the canvas calls
 * `paintPreferred`. Default (`gpu: false`) is always the CPU PNG blit,
 * even when the browser has WebGPU.
 *
 * WebGL2 is detected on a detached canvas and then refused: the desktop `gpu`
 * backend is Vello, which needs compute shaders. WebGL2 has none, so paint
 * stays on the CPU. The paint canvas is not probed. If a GPU context was
 * already taken and then fails, the canvas is replaced so the CPU blit can
 * acquire `"2d"`.
 */

import { paintCanvas } from "./canvas.js";

/**
 * @param {{ gpuFlag?: boolean, webgpu?: boolean, webgl2?: boolean }} probe
 * @returns {{ backend: "webgpu" | "cpu", reason: string }}
 */
export function choosePaintBackend(probe) {
  if (!probe || probe.gpuFlag !== true) {
    return { backend: "cpu", reason: "flag-off" };
  }
  if (probe.webgpu === true) {
    return { backend: "webgpu", reason: "" };
  }
  if (probe.webgl2 === true) {
    return { backend: "cpu", reason: "webgl2-no-compute" };
  }
  return { backend: "cpu", reason: "no-adapter" };
}

/**
 * Same checks wgpu uses before it will create a WebGPU adapter:
 * a `gpu` object, then `requestAdapter()` resolving to a non-null adapter.
 * `env` is `navigator` in a browser, or a stub in Node.
 */
export async function detectWebGpu(env) {
  const gpu = env && env.gpu;
  if (!gpu || typeof gpu.requestAdapter !== "function") return false;
  try {
    const adapter = await gpu.requestAdapter();
    return adapter != null;
  } catch {
    return false;
  }
}

/**
 * `true` when `getContext("webgl2")` returns an object.
 * Pass a detached canvas. A successful call commits that canvas to WebGL2,
 * and it can never take `"2d"` or `"webgpu"` afterwards.
 */
export function detectWebGl2(canvas) {
  if (!canvas || typeof canvas.getContext !== "function") return false;
  try {
    return canvas.getContext("webgl2") != null;
  } catch {
    return false;
  }
}

/**
 * Canvas that is not in the document. Probing it cannot commit the paint canvas.
 * `env.document` overrides the page document (tests). Returns null when no
 * document exists, which means "no WebGL2" rather than touching the paint canvas.
 */
export function detachedCanvas(env) {
  const doc =
    (env && env.document) ||
    (typeof globalThis.document !== "undefined" ? globalThis.document : undefined);
  if (!doc || typeof doc.createElement !== "function") return null;
  const canvas = doc.createElement("canvas");
  if (!canvas || typeof canvas.getContext !== "function") return null;
  canvas.width = 1;
  canvas.height = 1;
  return canvas;
}

/**
 * Swap `canvas` for a fresh element in the same parent.
 * A canvas that already has a WebGPU or WebGL context cannot take `"2d"`.
 * Returns the new canvas, or null when there is no parent to replace into.
 */
export function replacePaintCanvas(canvas) {
  const doc = canvas && canvas.ownerDocument;
  const parent = canvas && canvas.parentNode;
  if (!doc || !parent || typeof doc.createElement !== "function" || typeof parent.replaceChild !== "function") {
    return null;
  }
  const next = doc.createElement("canvas");
  if (!next) return null;
  if (canvas.id) {
    next.id = canvas.id;
    canvas.id = "";
  }
  if (canvas.className) next.className = canvas.className;
  if (canvas.style && next.style && canvas.style.cssText) next.style.cssText = canvas.style.cssText;
  next.width = canvas.width;
  next.height = canvas.height;
  parent.replaceChild(next, canvas);
  return next;
}

/**
 * Paint one frame.
 *
 * `options.gpu === true` opts into WebGPU. Without that flag the CPU blit
 * runs. When WebGPU is selected but `host.paintWebGpu` is missing or throws,
 * this falls back to `paintCanvas` and reports why.
 *
 * Returns `{ backend, reason, used, png }`. `used` is the path that drew.
 * `png` is set when the CPU blit ran.
 */
export async function paintPreferred(host, canvas, options = {}) {
  const width = options.width ?? canvas.width;
  const height = options.height ?? canvas.height;
  const env = options.env ?? globalNavigator();
  let webgpu = false;
  let webgl2 = false;
  if (options.detect) {
    webgpu = options.detect.webgpu === true;
    webgl2 = options.detect.webgl2 === true;
  } else if (options.gpu === true) {
    // WebGL2 only matters when WebGPU is missing. Never probe the paint canvas:
    // getContext commits that element to one context type.
    webgpu = await detectWebGpu(env);
    if (!webgpu) webgl2 = detectWebGl2(detachedCanvas(env));
  }
  const choice = choosePaintBackend({
    gpuFlag: options.gpu === true,
    webgpu,
    webgl2,
  });

  if (choice.backend === "webgpu" && host && typeof host.paintWebGpu === "function") {
    try {
      await host.paintWebGpu(canvas, width, height);
      return { backend: "webgpu", reason: "", used: "webgpu", surface: canvas };
    } catch (err) {
      const painted = await paintCpu(host, canvas, width, height);
      return {
        backend: "cpu",
        reason: "webgpu-failed",
        error: String(err && err.message ? err.message : err),
        used: "cpu",
        png: painted.png,
        surface: painted.surface,
      };
    }
  }

  const painted = await paintCpu(host, canvas, width, height);
  const reason = choice.backend === "webgpu" ? "no-paint-export" : choice.reason;
  return { backend: "cpu", reason, used: "cpu", png: painted.png, surface: painted.surface };
}

/** CPU blit. If this canvas is already committed to another context, replace it. */
async function paintCpu(host, canvas, width, height) {
  try {
    const png = await paintCanvas(host, canvas, width, height);
    return { png, surface: canvas };
  } catch (err) {
    if (!isMissing2d(err)) throw err;
    const next = replacePaintCanvas(canvas);
    if (!next) throw err;
    const png = await paintCanvas(host, next, width, height);
    return { png, surface: next };
  }
}

function isMissing2d(err) {
  return Boolean(err && typeof err.message === "string" && err.message.includes("canvas 2d context"));
}

function globalNavigator() {
  if (typeof globalThis.navigator === "undefined") return undefined;
  return globalThis.navigator;
}
