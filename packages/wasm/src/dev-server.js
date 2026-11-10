/**
 * One-command web dev server. Node only.
 *
 * Bundles a TSX entry with esbuild, serves it next to the wasm core,
 * and reloads the page when the bundle changes. `POST /patch` is the
 * same `ui.patch` JSON as the desktop dev server. Open pages apply it
 * on the wasm host. If `forwardUrl` is set (the desktop dev server),
 * the body is also POSTed there so source writeback still runs.
 */

import { spawn } from "node:child_process";
import { createReadStream, existsSync, mkdtempSync, rmSync, statSync, watch } from "node:fs";
import { createServer } from "node:http";
import { dirname, extname, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

import { bundleApp, esbuildBin, esbuildWatchArgs } from "./bundle-app.js";

const srcDir = dirname(fileURLToPath(import.meta.url));

function pageHtml() {
  return `<!DOCTYPE html>
<meta charset="utf-8">
<title>Tekto Notes</title>
<p>Host page. The Notes sketch is the TSX bundle. The wasm core paints the canvas. QuickJS is not used.</p>
<canvas id="view" width="640" height="400" style="border:1px solid #ccc; max-width:100%"></canvas>
<pre id="log"></pre>
<script type="module">
import { connectWasm } from "./src/index.js";
import { paintCanvas } from "./src/canvas.js";
import { applyUiPatch } from "./src/patch.js";
import { mountApp } from "./app.js";

const canvas = document.querySelector("#view");
const log = document.querySelector("#log");
const host = await connectWasm("./tekto_wasm.wasm");

async function show() {
  host.reset();
  mountApp(host);
  await paintCanvas(host, canvas, canvas.width, canvas.height);
  log.textContent = "painted " + canvas.width + "×" + canvas.height;
}

await show();

const events = new EventSource("/events");
events.onmessage = async (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.type === "reload") {
    location.reload();
    return;
  }
  if (msg.type === "patch") {
    applyUiPatch(host, msg.patch);
    await paintCanvas(host, canvas, canvas.width, canvas.height);
    log.textContent = "patched #" + msg.patch.id;
  }
};
</script>
`;
}

function contentType(path) {
  switch (extname(path)) {
    case ".js":
      return "text/javascript; charset=utf-8";
    case ".wasm":
      return "application/wasm";
    default:
      return "application/octet-stream";
  }
}

function readBody(req) {
  return new Promise((resolveBody, reject) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => resolveBody(Buffer.concat(chunks).toString("utf8")));
    req.on("error", reject);
  });
}

function pipeFile(res, path, type) {
  res.writeHead(200, { "content-type": type });
  createReadStream(path).pipe(res);
}

/**
 * @param {{ entry: string, wasmPath: string, port?: number, forwardUrl?: string, watch?: boolean }} opts
 */
export async function startDevServer(opts) {
  const entry = opts.entry;
  const wasmPath = opts.wasmPath;
  const forwardUrl = opts.forwardUrl || "";
  const dir = mkdtempSync(resolve(tmpdir(), "tekto-web-server-"));
  const outfile = resolve(dir, "app.js");
  let watchChild = null;
  try {
    bundleApp(entry, outfile);
    if (opts.watch !== false) {
      watchChild = spawn(esbuildBin(), esbuildWatchArgs(entry, outfile), {
        stdio: ["ignore", "ignore", "pipe"],
      });
      await new Promise((ready, reject) => {
        let output = "";
        const onError = (err) => reject(err);
        const onExit = (code) => reject(new Error(`esbuild watcher exited before ready (${code}): ${output}`));
        watchChild.once("error", onError);
        watchChild.once("exit", onExit);
        const onData = (chunk) => {
          output += chunk;
          if (!output.includes("[watch] build finished, watching for changes...")) return;
          watchChild.removeListener("error", onError);
          watchChild.removeListener("exit", onExit);
          watchChild.stderr.removeListener("data", onData);
          watchChild.stderr.resume();
          if (output.includes("[ERROR]")) reject(new Error(`esbuild watcher failed: ${output}`));
          else ready();
        };
        watchChild.stderr.on("data", onData);
      });
    }
  } catch (err) {
    if (watchChild) watchChild.kill();
    rmSync(dir, { recursive: true, force: true });
    throw err;
  }

  const clients = new Set();

  function send(obj) {
    const line = `data: ${JSON.stringify(obj)}\n\n`;
    for (const res of clients) res.write(line);
  }

  let watcher = null;
  try {
    watcher = watch(outfile, () => send({ type: "reload" }));
  } catch {
    watcher = null;
  }

  const server = createServer(async (req, res) => {
    const url = new URL(req.url || "/", "http://127.0.0.1");
    try {
      if (req.method === "GET" && url.pathname === "/events") {
        res.writeHead(200, {
          "content-type": "text/event-stream",
          "cache-control": "no-cache",
          connection: "keep-alive",
        });
        res.write(": ok\n\n");
        clients.add(res);
        req.on("close", () => clients.delete(res));
        return;
      }
      if (req.method === "POST" && url.pathname === "/patch") {
        const raw = await readBody(req);
        const patch = JSON.parse(raw || "{}");
        if (typeof patch.id !== "number") {
          res.writeHead(400, { "content-type": "application/json" });
          res.end(JSON.stringify({ error: "missing id" }));
          return;
        }
        send({ type: "patch", patch });
        let forward = null;
        if (forwardUrl) {
          const target = forwardUrl.replace(/\/$/, "") + "/patch";
          try {
            const upstream = await fetch(target, {
              method: "POST",
              headers: { "content-type": "application/json" },
              body: raw || JSON.stringify(patch),
            });
            forward = { status: upstream.status, body: await upstream.text() };
          } catch (err) {
            forward = { error: String(err) };
          }
        }
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ok: true, delivered: clients.size, forward }));
        return;
      }
      if (req.method === "GET" && (url.pathname === "/" || url.pathname === "/index.html")) {
        res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
        res.end(pageHtml());
        return;
      }
      if (req.method === "GET" && url.pathname === "/app.js") {
        pipeFile(res, outfile, "text/javascript; charset=utf-8");
        return;
      }
      if (req.method === "GET" && url.pathname === "/tekto_wasm.wasm") {
        if (!existsSync(wasmPath)) {
          res.writeHead(404, { "content-type": "application/json" });
          res.end(JSON.stringify({ error: "wasm missing" }));
          return;
        }
        pipeFile(res, wasmPath, "application/wasm");
        return;
      }
      if (req.method === "GET" && url.pathname.startsWith("/src/")) {
        const file = resolve(srcDir, url.pathname.slice("/src/".length));
        if (!file.startsWith(srcDir) || !existsSync(file) || !statSync(file).isFile()) {
          res.writeHead(404);
          res.end("not found");
          return;
        }
        pipeFile(res, file, contentType(file));
        return;
      }
      res.writeHead(404, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: "not found" }));
    } catch (err) {
      res.writeHead(500, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: String(err && err.message ? err.message : err) }));
    }
  });

  const port = opts.port ?? 0;
  return new Promise((resolveListen, reject) => {
    server.once("error", (err) => {
      if (watcher) watcher.close();
      if (watchChild) watchChild.kill();
      rmSync(dir, { recursive: true, force: true });
      reject(err);
    });
    server.listen(port, "127.0.0.1", () => {
      const address = server.address();
      const bound = typeof address === "object" && address ? address.port : port;
      resolveListen({
        port: bound,
        url: `http://127.0.0.1:${bound}/`,
        close() {
          if (watcher) watcher.close();
          if (watchChild) watchChild.kill();
          for (const client of clients) client.end();
          return new Promise((done) => server.close(() => {
            rmSync(dir, { recursive: true, force: true });
            done();
          }));
        },
      });
    });
  });
}
