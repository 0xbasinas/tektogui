import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";
import { bundleApp, wrapperSource, esbuildBin, resolveEsbuildBinary } from "./src/index.js";

test("wrapper escapes paths and rejects invalid targets", () => {
  assert.match(wrapperSource('C:/path with spaces/a"b.tsx'), /a\\"b/);
  assert.match(wrapperSource("/app.tsx", "desktop"), /globalThis\.tekto/);
  assert.throws(() => wrapperSource("/app.tsx", "bad"), /unknown build target/);
});

test("windows shims resolve to the native esbuild binary", () => {
  const shim = join("npm", "esbuild.cmd");
  const native = resolve(
    dirname(shim),
    "node_modules",
    "@esbuild",
    "win32-x64",
    "esbuild.exe",
  );
  const seen = [];
  const resolveWindows = (candidate) =>
    resolveEsbuildBinary(candidate, {
      platform: "win32",
      arch: "x64",
      exists: (path) => {
        seen.push(path);
        return path === native;
      },
    });
  assert.equal(resolveWindows(shim), native);
  assert.ok(seen.includes(native));
  // A local install keeps @esbuild beside node_modules/.bin, not inside it.
  const localShim = join("app", "node_modules", ".bin", "esbuild.cmd");
  const localNative = resolve(
    dirname(localShim),
    "..",
    "@esbuild",
    "win32-x64",
    "esbuild.exe",
  );
  assert.equal(
    resolveEsbuildBinary(localShim, {
      platform: "win32",
      arch: "x64",
      exists: (path) => path === localNative,
    }),
    localNative,
  );
  // A native path, another platform, and a missing sibling stay untouched.
  assert.equal(resolveWindows(native), native);
  assert.equal(resolveEsbuildBinary(shim, { platform: "linux" }), shim);
  assert.equal(
    resolveEsbuildBinary(shim, {
      platform: "win32",
      arch: "arm64",
      exists: () => false,
    }),
    shim,
  );
});

test("CLI reports invalid arguments", () => {
  const cli = new URL("./src/cli.js", import.meta.url);
  const result = spawnSync(process.execPath, [fileURLToPath(cli), "bad"], { encoding: "utf8" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /usage: tekto-build/);
});

test("same TSX entry builds for web and desktop", async (t) => {
  if (spawnSync(esbuildBin(), ["--version"], { encoding: "utf8" }).status !== 0) {
    if (process.env.TEKTO_REQUIRE_ESBUILD === "1") throw new Error("esbuild is required");
    t.skip("esbuild is unavailable (PATH or ESBUILD)"); return;
  }
  const dir = mkdtempSync(join(tmpdir(), "tekto-build-test-"));
  try {
    const entry = resolve("examples/notes/src/app.tsx");
    const web = join(dir, "web.mjs"), desktop = join(dir, "desktop.js");
    bundleApp(entry, web);
    bundleApp(entry, desktop, { target: "desktop" });
    const module = await import(pathToFileURL(web));
    assert.equal(typeof module.App, "function");
    assert.equal(typeof module.mountApp, "function");
    assert.match(readFileSync(desktop, "utf8"), /globalThis\.tekto/);
    assert.doesNotMatch(readFileSync(desktop, "utf8"), /export\s*\{/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
