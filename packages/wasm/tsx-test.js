import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

import { bundleApp, esbuildBin, repoRoot } from "./src/bundle-app.js";
import { startDevServer } from "./src/dev-server.js";
import { connectWasm } from "./src/index.js";
import { applyUiPatch } from "./src/patch.js";
import { wasmPath as findWasmPath } from "./src/wasm-path.js";
import { requireOrSkip } from "./test-prerequisites.js";

const root = repoRoot();
const entry = resolve(root, "examples/notes/src/app.tsx");
const wasmPath = findWasmPath(root);

function haveEsbuild() {
  const found = spawnSync(esbuildBin(), ["--version"], { encoding: "utf8" });
  return !found.error && found.status === 0;
}

test("applyUiPatch forwards paint fields and aliases", () => {
  const calls = [];
  const host = { setText() {}, setProp: (...args) => calls.push(args) };
  applyUiPatch(host, { id: 3, radius: 4, border_color: "#123456", border_width: 2 });
  assert.deepEqual(calls, [[3, "radius", "4"], [3, "borderColor", "#123456"], [3, "borderWidth", "2"]]);
});

test("applyUiPatch forwards disabled booleans", () => {
  const calls = [];
  const host = { setText() {}, setProp: (...args) => calls.push(args) };
  applyUiPatch(host, { id: 3, disabled: true });
  applyUiPatch(host, { id: 3, disabled: false });
  assert.deepEqual(calls, [[3, "disabled", "true"], [3, "disabled", "false"]]);
});

test("Notes TSX bundle mounts on the wasm core", async (t) => {
  if (!requireOrSkip(t, haveEsbuild(), "esbuild is unavailable (PATH or ESBUILD)")) return;
  if (!requireOrSkip(t, existsSync(wasmPath), `no wasm module at ${wasmPath}`)) return;
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
  if (!requireOrSkip(t, haveEsbuild(), "esbuild is unavailable (PATH or ESBUILD)")) return;
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

test("web dev server rebuilds the bundle when the TSX entry changes", async (t) => {
  if (!requireOrSkip(t, haveEsbuild(), "esbuild is unavailable (PATH or ESBUILD)")) return;
  const dir = mkdtempSync(resolve(tmpdir(), "tekto-watch-"));
  const tsx = resolve(dir, "app.tsx");
  const before = "watch-token-before";
  const after = "watch-token-after";
  writeFileSync(
    tsx,
    `export function App() {\n  return <text>${before}</text>;\n}\n`,
  );
  const server = await startDevServer({
    entry: tsx,
    wasmPath,
    port: 0,
    watch: true,
  });
  try {
    const first = await fetch(new URL("/app.js", server.url));
    assert.equal(first.status, 200);
    assert.match(await first.text(), new RegExp(before));

    writeFileSync(
      tsx,
      `export function App() {\n  return <text>${after}</text>;\n}\n`,
    );

    const deadline = Date.now() + 5000;
    let js = "";
    while (Date.now() < deadline) {
      const next = await fetch(new URL("/app.js", server.url));
      js = await next.text();
      if (js.includes(after)) break;
      await new Promise((r) => setTimeout(r, 100));
    }
    assert.match(js, new RegExp(after));
    assert.doesNotMatch(js, new RegExp(before));
  } finally {
    await server.close();
    rmSync(dir, { recursive: true, force: true });
  }
});


test("web dev server waits for its watcher and isolates concurrent bundles", async (t) => {
  if (!requireOrSkip(t, haveEsbuild(), "esbuild is unavailable (PATH or ESBUILD)")) return;
  const dir = mkdtempSync(resolve(tmpdir(), "tekto-watch-ready-"));
  const firstEntry = resolve(dir, "first.tsx");
  const secondEntry = resolve(dir, "second.tsx");
  writeFileSync(firstEntry, 'export function App() { return <text>first-server</text>; }');
  writeFileSync(secondEntry, 'export function App() { return <text>second-server</text>; }');
  let signalReady, release;
  const watcherReady = new Promise(resolve => { signalReady = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  let firstServer;
  let secondServer;
  try {
    let started = false;
    const starting = startDevServer({ entry: firstEntry, wasmPath, port: 0,
      beforeWatcherReady: () => { signalReady(); return gate; } }).then((server) => {
      started = true;
      return server;
    });
    await Promise.race([watcherReady, starting.then(() => { throw new Error("server started before watcher gate"); })]);
    await new Promise((resolveTurn) => setImmediate(resolveTurn));
    assert.equal(started, false, "server must wait for the watcher initial build");
    release();
    firstServer = await starting;
    secondServer = await startDevServer({ entry: secondEntry, wasmPath, port: 0, watch: false });
    assert.match(await (await fetch(new URL("/app.js", firstServer.url))).text(), /first-server/);
    assert.match(await (await fetch(new URL("/app.js", secondServer.url))).text(), /second-server/);
  } finally {
    release();
    if (firstServer) await firstServer.close();
    if (secondServer) await secondServer.close();
    rmSync(dir, { recursive: true, force: true });
  }
});
