import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

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
