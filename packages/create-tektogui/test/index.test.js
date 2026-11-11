import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync, copyFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("../src/index.js", import.meta.url));
test("wrapper resolves roots, forwards arguments, and reports failures", async (t) => {
  const tmp = mkdtempSync(path.join(tmpdir(), "tekto wrapper "));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const root = path.join(tmp, "checkout with spaces");
  const cwd = path.join(tmp, "destination with spaces");
  const bin = path.join(tmp, "bin");
  for (const dir of [path.join(root, "crates/tekto"), path.join(root, "crates/tekto-cli"), cwd, bin]) mkdirSync(dir, { recursive: true });
  for (const file of ["Cargo.toml", "crates/tekto/Cargo.toml", "crates/tekto-cli/Cargo.toml"]) writeFileSync(path.join(root, file), "");
  const capture = path.join(tmp, "capture.json");
  writeFileSync(path.join(bin, "cargo"), `#!${process.execPath}\nrequire('fs').writeFileSync(process.env.CAPTURE, JSON.stringify({args: process.argv.slice(2), cwd: process.cwd(), root: process.env.TEKTO_ROOT}));\n`, { mode: 0o755 });
  const run = (args, env = {}, entry = script) => new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [entry, ...args], { cwd, env: { ...process.env, PATH: bin, CAPTURE: capture, ...env } });
    let stdout = "", stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("close", (status) => resolve({ status, stdout, stderr }));
  });
  for (const rootValue of [root, path.relative(cwd, root)]) {
    const result = await run(["my-app"], { TEKTO_ROOT: rootValue });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(readFileSync(capture)), { args: ["run", "-q", "--manifest-path", path.join(root, "Cargo.toml"), "-p", "tekto-cli", "--", "new", "my-app"], cwd, root });
  }
  for (const value of ["", "missing", cwd]) {
    const result = await run(["hello"], { TEKTO_ROOT: value });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /TEKTO_ROOT=.* does not look like a tektogui checkout/);
  }
  for (const flag of ["--help", "-h"]) {
    const result = await run([flag], { TEKTO_ROOT: "invalid" });
    assert.equal(result.status, 0);
    assert.match(result.stdout, /usage:.*<name>[\s\S]*TEKTO_ROOT/);
  }
  for (const args of [[], ["hello", "extra"], ["--unknown"]]) assert.equal((await run(args)).status, 1);
  const missingCargo = await run(["hello"], { TEKTO_ROOT: root, PATH: path.join(tmp, "missing-bin") });
  assert.equal(missingCargo.status, 1);
  assert.match(missingCargo.stderr, /could not start cargo/);
  rmSync(path.join(root, "crates/tekto-cli/Cargo.toml"));
  assert.match((await run(["hello"], { TEKTO_ROOT: root })).stderr, /missing crates\/tekto-cli\/Cargo.toml/);
  const installed = path.join(tmp, "installed/src");
  mkdirSync(installed, { recursive: true });
  copyFileSync(script, path.join(installed, "index.mjs"));
  const noRoot = await run(["hello"], { TEKTO_ROOT: undefined }, path.join(installed, "index.mjs"));
  assert.equal(noRoot.status, 1);
  assert.match(noRoot.stderr, /set TEKTO_ROOT=<checkout>/);
});
