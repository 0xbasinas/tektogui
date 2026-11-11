#!/usr/bin/env node
// Generate an app with create-tektogui, then install, typecheck, and build it
// with every package manager that is available. A missing manager skips unless
// TEKTO_REQUIRE_PMS names it (comma separated), which mirrors the required
// esbuild mode used by the WASM tests.
const { spawnSync } = require("node:child_process");
const { existsSync, mkdtempSync, readFileSync, rmSync } = require("node:fs");
const { tmpdir } = require("node:os");
const { dirname, join, resolve } = require("node:path");

const root = resolve(__dirname, "..");
const create = join(root, "packages", "create-tektogui", "src", "index.js");
const required = (process.env.TEKTO_REQUIRE_PMS || "")
  .split(",")
  .map((name) => name.trim())
  .filter(Boolean);
const managers = ["npm", "pnpm", "bun"];

function quote(arg) {
  return /[\s"]/.test(arg) ? `"${arg.replaceAll('"', '\\"')}"` : arg;
}

// npm, pnpm, and bun on Windows are .cmd shims, so the command needs a shell.
function run(bin, args, options = {}) {
  const win = process.platform === "win32";
  const result = win
    ? spawnSync([quote(bin), ...args.map(quote)].join(" "), { ...options, shell: true })
    : spawnSync(bin, args, options);
  return { ...result, out: `${result.stdout || ""}${result.stderr || ""}`.trim() };
}

function available(bin) {
  return run(bin, ["--version"], { encoding: "utf8" }).status === 0;
}

function fail(message, result) {
  const tail = result ? result.out.split(/\r?\n/).slice(-8).join("\n") : "";
  throw new Error(tail ? `${message}\n${tail}` : message);
}

function buildWith(manager, name) {
  const dir = mkdtempSync(join(tmpdir(), `tekto-${manager}-`));
  try {
    const scaffold = run(process.execPath, [create, name], {
      cwd: dir,
      encoding: "utf8",
      env: { ...process.env, TEKTO_ROOT: root },
    });
    if (scaffold.status !== 0) fail(`${manager}: create-tektogui failed`, scaffold);
    const app = join(dir, name);
    const install = run(manager, ["install"], { cwd: app, encoding: "utf8" });
    if (install.status !== 0) fail(`${manager}: install failed`, install);
    for (const script of ["typecheck", "build:web", "build:desktop"]) {
      const step = run(manager, ["run", script], { cwd: app, encoding: "utf8" });
      if (step.status !== 0) fail(`${manager}: ${script} failed`, step);
    }
    for (const file of ["dist/app.mjs", "dist/app.js"]) {
      if (!existsSync(join(app, file))) fail(`${manager}: ${file} was not written`);
    }
    const desktop = readFileSync(join(app, "dist", "app.js"), "utf8");
    if (!desktop.includes("globalThis.tekto")) fail(`${manager}: desktop bundle is not QuickJS`);
    return { manager, ok: true };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

let ok = 0;
let failed = false;
for (const manager of managers) {
  if (!available(manager)) {
    if (required.includes(manager)) {
      console.error(`${manager}: required but not installed`);
      failed = true;
    } else {
      console.log(`${manager}: not installed, skipped`);
    }
    continue;
  }
  try {
    buildWith(manager, "hello");
    ok += 1;
    console.log(`${manager}: install, typecheck, build:web and build:desktop ok`);
  } catch (error) {
    console.error(error.message);
    failed = true;
  }
}

if (ok === 0 && !failed) {
  console.error("no package manager was available");
  process.exit(1);
}
process.exit(failed ? 1 : 0);
