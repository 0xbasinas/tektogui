/**
 * Bundle a Tekto TSX entry to one ESM file with the esbuild binary (MIT).
 * The browser (or Node) runs that file. QuickJS is not involved.
 * Node-only. Not part of the public app API.
 */

import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function repoRoot() {
  return resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
}

export function esbuildBin() {
  return process.env.ESBUILD || "esbuild";
}

export function wrapperSource(entry) {
  const path = entry.replaceAll("\\", "/");
  return `import { mount } from "@tektogui/jsx-runtime";
import * as entry from ${JSON.stringify(path)};
export const App = entry.App || entry.default;
export function mountApp(host) {
  if (typeof App !== "function") {
    throw new Error("TSX entry must export a function App (named or default)");
  }
  return mount(App(), host);
}
`;
}

function esbuildArgs(wrapper, outfile) {
  const root = repoRoot();
  const core = resolve(root, "packages/core/src/index.js");
  const jsx = resolve(root, "packages/jsx-runtime/src/index.js");
  return [
    wrapper,
    "--bundle",
    "--format=esm",
    "--platform=neutral",
    "--target=es2020",
    "--jsx=automatic",
    "--jsx-import-source=@tektogui/jsx-runtime",
    `--alias:@tektogui/core=${core}`,
    `--alias:@tektogui/jsx-runtime=${jsx}`,
    `--alias:@tektogui/jsx-runtime/jsx-runtime=${jsx}`,
    `--alias:@tektogui/jsx-runtime/jsx-dev-runtime=${jsx}`,
    `--outfile=${outfile}`,
    "--log-level=warning",
  ];
}

/** Write the bundle to `outfile` and return that path. */
export function bundleApp(entry, outfile) {
  const abs = resolve(entry);
  const scratch = mkdtempSync(resolve(tmpdir(), "tekto-web-"));
  const wrapper = resolve(scratch, "entry.js");
  writeFileSync(wrapper, wrapperSource(abs));
  mkdirSync(dirname(outfile), { recursive: true });
  const result = spawnSync(esbuildBin(), esbuildArgs(wrapper, outfile), { encoding: "utf8" });
  rmSync(scratch, { recursive: true, force: true });
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
  return esbuildArgs(scratch, outfile).concat(["--watch"]);
}
