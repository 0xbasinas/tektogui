import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { renderToOps } from "../jsx-runtime/src/index.js";
import { Notes } from "../jsx-runtime/notes-app.js";
import { blitPng, decodePng, resizeCanvas } from "./src/canvas.js";
import { choosePaintBackend, detectWebGl2, detectWebGpu, paintPreferred } from "./src/gpu.js";
import { connectWasm } from "./src/index.js";

const wasmPath =
  process.env.TEKTO_WASM ??
  fileURLToPath(
    new URL("../../target/wasm32-unknown-unknown/debug/tekto_wasm.wasm", import.meta.url),
  );

test("wasm host applies HostOps and returns a PNG", async (t) => {
  if (!existsSync(wasmPath)) {
    t.skip(`no wasm module at ${wasmPath} (cargo build -p tekto-wasm --target wasm32-unknown-unknown)`);
    return;
  }
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
  if (!existsSync(wasmPath)) {
    t.skip(`no wasm module at ${wasmPath} (cargo build -p tekto-wasm --target wasm32-unknown-unknown)`);
    return;
  }
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

const PNG_1X1 = new Uint8Array([
  137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0,
  0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0, 5, 0, 1,
  255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
]);
