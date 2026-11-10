/**
 * Host glue: pick WebGPU or the CPU canvas blit.
 *
 * App code still has no DOM types. The page that owns the canvas calls
 * `paintPreferred`. Default (`gpu: false`) is always the CPU PNG blit,
 * even when the browser has WebGPU.
 *
 * WebGL2 is detected and then refused: the desktop `gpu` backend is Vello,
 * which needs compute shaders. WebGL2 has none, so paint stays on the CPU.
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

/** `true` when `getContext("webgl2")` returns an object. */
export function detectWebGl2(canvas) {
  if (!canvas || typeof canvas.getContext !== "function") return false;
  try {
    return canvas.getContext("webgl2") != null;
  } catch {
    return false;
  }
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
  const detected =
    options.detect ??
    {
      webgpu: await detectWebGpu(options.env ?? globalNavigator()),
      webgl2: detectWebGl2(canvas),
    };
  const choice = choosePaintBackend({
    gpuFlag: options.gpu === true,
    webgpu: detected.webgpu === true,
    webgl2: detected.webgl2 === true,
  });

  if (choice.backend === "webgpu" && host && typeof host.paintWebGpu === "function") {
    try {
      await host.paintWebGpu(canvas, width, height);
      return { backend: "webgpu", reason: "", used: "webgpu" };
    } catch (err) {
      const png = await paintCanvas(host, canvas, width, height);
      return {
        backend: "cpu",
        reason: "webgpu-failed",
        error: String(err && err.message ? err.message : err),
        used: "cpu",
        png,
      };
    }
  }

  const png = await paintCanvas(host, canvas, width, height);
  const reason = choice.backend === "webgpu" ? "no-paint-export" : choice.reason;
  return { backend: "cpu", reason, used: "cpu", png };
}

function globalNavigator() {
  if (typeof globalThis.navigator === "undefined") return undefined;
  return globalThis.navigator;
}
