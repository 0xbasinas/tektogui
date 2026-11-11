/**
 * Bundle a Tekto TSX entry for the browser or desktop QuickJS host.
 * Node-only development tooling; esbuild is supplied by the caller.
 */

import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const spec = JSON.parse(readFileSync(new URL("./build-spec.json", import.meta.url), "utf8"));

export function repoRoot() {
  return resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
}

export function esbuildBin() {
  return process.env.ESBUILD || "esbuild";
}

export function wrapperSource(entry, target = "web") {
  const path = entry.replaceAll("\\", "/");
  if (target !== "web" && target !== "desktop") throw new Error(`unknown build target ${target}`);
  return `${spec.imports}import * as entry from ${JSON.stringify(path)};\n${spec[target]}`;
}

function esbuildArgs(wrapper, outfile, target = "web") {
  const root = repoRoot();
  const locate = (name, local) => {
    try { return require.resolve(name); } catch { return resolve(root, local); }
  };
  const core = locate("@tektogui/core", "packages/core/src/index.js");
  const jsx = locate("@tektogui/jsx-runtime", "packages/jsx-runtime/src/index.js");
  return [
    wrapper,
    ...spec.flags,
    `--format=${target === "web" ? "esm" : "iife"}`,
    `--alias:@tektogui/core=${core}`,
    `--alias:@tektogui/jsx-runtime=${jsx}`,
    `--alias:@tektogui/jsx-runtime/jsx-runtime=${jsx}`,
    `--alias:@tektogui/jsx-runtime/jsx-dev-runtime=${jsx}`,
    `--outfile=${outfile}`,
  ];
}

/** Write the bundle to `outfile` and return that path. */
export function bundleApp(entry, outfile, { target = "web" } = {}) {
  const abs = resolve(entry);
  const source = wrapperSource(abs, target);
  const scratch = mkdtempSync(resolve(tmpdir(), "tekto-web-"));
  const wrapper = resolve(scratch, "entry.js");
  let result;
  try {
    writeFileSync(wrapper, source);
    mkdirSync(dirname(outfile), { recursive: true });
    result = spawnSync(esbuildBin(), esbuildArgs(wrapper, outfile, target), { encoding: "utf8" });
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
  if (result.error) {
    const err = new Error(
      result.error.code === "ENOENT"
        ? "esbuild not found on PATH (or $ESBUILD)"
        : result.error.message,
    );
    err.code = result.error.code;
    throw err;
  }
  if (result.status !== 0) {
    throw new Error(result.stderr || result.stdout || "esbuild failed");
  }
  return outfile;
}

export function esbuildWatchArgs(entry, outfile) {
  const abs = resolve(entry);
  const scratch = resolve(dirname(outfile), "entry.js");
  mkdirSync(dirname(outfile), { recursive: true });
  writeFileSync(scratch, wrapperSource(abs));
  // `--watch` alone exits when stdin closes. The dev server does not keep
  // stdin open, so use `--watch=forever` (esbuild stays up until killed).
  return esbuildArgs(scratch, outfile).concat(["--log-level=info", "--watch=forever"]);
}
