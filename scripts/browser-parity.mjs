#!/usr/bin/env node
// Run the browser parity page in headless Chrome and check the PASS lines.
// Manual check: it needs Chrome, a wasm build, and a free debugging port.
// It is not part of CI.
import { spawn, spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, readFileSync, statSync } from "node:fs";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const wasm = join(root, "target/wasm32-unknown-unknown/debug/tekto_wasm.wasm");
if (!existsSync(wasm)) {
  console.log("building tekto-wasm…");
  const cargo = process.platform === "win32" ? "cargo.exe" : "cargo";
  const build = spawnSync(cargo, ["build", "-p", "tekto-wasm", "--target", "wasm32-unknown-unknown"], { stdio: "inherit" });
  if (build.status !== 0) process.exit(build.status ?? 1);
}

const types = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".json": "application/json", ".css": "text/css" };
const server = createServer((request, response) => {
  const path = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
  if (path === "/packages/wasm/tekto_wasm.wasm") {
    response.writeHead(200, { "content-type": "application/wasm" });
    response.end(readFileSync(wasm));
    return;
  }
  const file = normalize(join(root, path));
  if (!file.startsWith(root) || !existsSync(file) || !statSync(file).isFile()) {
    response.writeHead(404).end("not found");
    return;
  }
  response.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream" });
  response.end(readFileSync(file));
});

function chrome() {
  const candidates = [];
  if (process.platform === "win32") {
    candidates.push(
      join(process.env.ProgramFiles ?? "C:/Program Files", "Google/Chrome/Application/chrome.exe"),
      join(process.env["ProgramFiles(x86)"] ?? "C:/Program Files (x86)", "Google/Chrome/Application/chrome.exe"),
      join(process.env.ProgramFiles ?? "C:/Program Files", "Microsoft/Edge/Application/msedge.exe"),
    );
  } else if (process.platform === "darwin") {
    candidates.push("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
  } else {
    candidates.push("/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser");
  }
  return candidates.find(existsSync);
}

class Cdp {
  constructor(url) {
    this.ws = new WebSocket(url);
    this.next = 1;
    this.pending = new Map();
    this.ready = new Promise((resolve, reject) => {
      this.ws.addEventListener("open", resolve);
      this.ws.addEventListener("error", reject);
    });
    this.ws.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      const slot = this.pending.get(message.id);
      if (slot) {
        this.pending.delete(message.id);
        slot(message);
      }
    });
  }
  send(method, params = {}) {
    const id = this.next++;
    this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve) => this.pending.set(id, resolve));
  }
  async evaluate(expression) {
    const reply = await this.send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    return reply.result?.result?.value;
  }
}

const binary = chrome();
if (!binary) {
  console.error("Chrome or Edge was not found");
  process.exit(1);
}
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const { port } = server.address();
const url = `http://127.0.0.1:${port}/packages/wasm/parity.html`;
const debugPort = 9300 + (process.pid % 500);
const browser = spawn(binary, [
  "--headless=new",
  "--disable-gpu",
  "--no-sandbox",
  "--hide-scrollbars",
  `--user-data-dir=${join(process.env.TEMP ?? "/tmp", `tekto-parity-${process.pid}`)}`,
  `--remote-debugging-port=${debugPort}`,
  url,
], { stdio: "ignore" });

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
let report = "(no report)";
try {
  let target = null;
  for (let attempt = 0; attempt < 60 && !target; attempt++) {
    await sleep(250);
    try {
      const response = await fetch(`http://127.0.0.1:${debugPort}/json/list`);
      const pages = await response.json();
      target = pages.find((page) => page.type === "page" && page.url.includes("parity.html"));
    } catch {
      // Chrome is still starting.
    }
  }
  if (!target) throw new Error("Chrome did not expose the page");
  const cdp = new Cdp(target.webSocketDebuggerUrl);
  await cdp.ready;
  for (let attempt = 0; attempt < 240; attempt++) {
    report = (await cdp.evaluate("document.getElementById('log')?.textContent ?? ''")) ?? "";
    if (/ALL PASS|FAILED \d+|FAIL threw/.test(report)) break;
    await sleep(500);
  }
  cdp.ws.close();
} catch (error) {
  report = `FAIL threw: ${error.message}\nFAILED 1`;
} finally {
  browser.kill();
  server.close();
}

console.log(report.trim() || "(no report)");
const ok = /ALL PASS/.test(report) && !/^FAIL /m.test(report);
if (!ok) process.exit(1);
