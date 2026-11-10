import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

import { bundleApp, esbuildBin, repoRoot } from "./src/bundle-app.js";
import { startDevServer } from "./src/dev-server.js";
import { connectWasm } from "./src/index.js";
import { applyUiPatch } from "./src/patch.js";

const root = repoRoot();
const entry = resolve(root, "examples/notes/src/app.tsx");
const wasmPath =
  process.env.TEKTO_WASM ??
  resolve(root, "target/wasm32-unknown-unknown/debug/tekto_wasm.wasm");

function haveEsbuild() {
  const found = spawnSync(esbuildBin(), ["--version"], { encoding: "utf8" });
  return !found.error && found.status === 0;
}

test("Notes TSX bundle mounts on the wasm core", async (t) => {
  if (!haveEsbuild()) {
    t.skip("esbuild is not on PATH");
    return;
  }
  if (!existsSync(wasmPath)) {
    t.skip(`no wasm module at ${wasmPath}`);
    return;
  }
  const dir = mkdtempSync(resolve(tmpdir(), "tekto-tsx-"));
  const outfile = resolve(dir, "app.js");
  try {
    bundleApp(entry, outfile);
    const mod = await import(pathToFileURL(outfile).href);
    assert.equal(typeof mod.App, "function");
    assert.equal(typeof mod.mountApp, "function");
    const host = await connectWasm(readFileSync(wasmPath));
    host.reset();
    mod.mountApp(host);
    const png = host.renderPng(640, 400);
    const expected = readFileSync(resolve(root, "crates/tekto/tests/snapshots/notes-headless.png"));
    assert.deepEqual(Buffer.from(png), expected);

    applyUiPatch(host, { id: 4, text: "Patched in the browser engine" });
    const patched = host.renderPng(640, 400);
    assert.equal(patched[0], 0x89);
    assert.notDeepEqual(Buffer.from(patched), expected);
    assert.throws(() => applyUiPatch(host, {}), /missing id/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("web dev server serves the bundle and pushes ui.patch", async (t) => {
  if (!haveEsbuild()) {
    t.skip("esbuild is not on PATH");
    return;
  }
  const server = await startDevServer({
    entry,
    wasmPath,
    port: 0,
    watch: false,
  });
  try {
    const page = await fetch(server.url);
    assert.equal(page.status, 200);
    const html = await page.text();
    assert.match(html, /mountApp/);
    assert.match(html, /applyUiPatch/);

    const bundle = await fetch(new URL("/app.js", server.url));
    assert.equal(bundle.status, 200);
    const js = await bundle.text();
    assert.match(js, /Hello from Tekto/);

    const events = await fetch(new URL("/events", server.url), {
      headers: { accept: "text/event-stream" },
    });
    assert.equal(events.status, 200);
    const reader = events.body.getReader();
    const decoder = new TextDecoder();
    let buf = "";
    const first = await reader.read();
    buf += decoder.decode(first.value);

    const posted = await fetch(new URL("/patch", server.url), {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ id: 4, text: "From dev server" }),
    });
    const body = await posted.json();
    assert.equal(body.ok, true);
    assert.equal(body.delivered, 1);

    const deadline = Date.now() + 2000;
    while (!buf.includes("From dev server") && Date.now() < deadline) {
      const chunk = await reader.read();
      if (chunk.done) break;
      buf += decoder.decode(chunk.value);
    }
    assert.match(buf, /From dev server/);
    await reader.cancel();

    const missing = await fetch(new URL("/patch", server.url), {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: "{}",
    });
    assert.equal(missing.status, 400);
  } finally {
    await server.close();
  }
});
