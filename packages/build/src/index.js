/**
 * Bundle a Tekto TSX entry for the browser or desktop QuickJS host.
 * Node-only development tooling; esbuild is supplied by the caller.
 */

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const spec = JSON.parse(readFileSync(new URL("./build-spec.json", import.meta.url), "utf8"));

export function repoRoot() {
  return resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
}

/**
 * Map a Windows npm or pnpm shim to the native esbuild binary next to it.
 * `spawnSync` cannot start a `.cmd` or extensionless shim without a shell.
 * A global install keeps `@esbuild` under the shim's directory; a local one
 * keeps it one level up, beside `node_modules/.bin`.
 */
export function resolveEsbuildBinary(
  candidate,
  { platform = process.platform, arch = process.arch, exists = existsSync } = {},
) {
  if (platform !== "win32" || !candidate || /\.exe$/i.test(candidate)) return candidate;
  const bin = dirname(candidate);
  const archDir = `win32-${arch}`;
  for (const root of [bin, resolve(bin, "..")]) {
    const native = resolve(root, "node_modules", "@esbuild", archDir, "esbuild.exe");
    if (exists(native)) return native;
    const sibling = resolve(root, "@esbuild", archDir, "esbuild.exe");
    if (exists(sibling)) return sibling;
  }
  return candidate;
}

export function esbuildBin() {
  const configured = process.env.ESBUILD;
  if (configured) return resolveEsbuildBinary(configured);
  if (process.platform !== "win32") return "esbuild";
  for (const dir of (process.env.PATH || "").split(delimiter).filter(Boolean)) {
    const exe = resolve(dir, "esbuild.exe");
    if (existsSync(exe)) return exe;
    const shim = resolve(dir, "esbuild.cmd");
    if (existsSync(shim)) {
      // pnpm keeps the native binary under `.pnpm`, so the shim is the entry
      // point there; spawnEsbuild runs it through cmd.exe.
      return resolveEsbuildBinary(shim);
    }
  }
  return "esbuild";
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

function quoteArg(arg) {
  return /[\s"]/.test(arg) ? `"${arg.replaceAll('"', '\\"')}"` : arg;
}

/**
 * Start esbuild. A Windows `.cmd` shim (what npm, pnpm, and bun put in
 * `node_modules/.bin`) cannot be started without a shell, and pnpm keeps its
 * native binary under `.pnpm`, so the shim is the reliable entry point there.
 */
function spawnEsbuild(bin, args) {
  if (process.platform === "win32" && /\.(cmd|bat)$/i.test(bin)) {
    const command = [bin, ...args].map(quoteArg).join(" ");
    return spawnSync(command, { encoding: "utf8", shell: true });
  }
  return spawnSync(bin, args, { encoding: "utf8" });
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
    result = spawnEsbuild(esbuildBin(), esbuildArgs(wrapper, outfile, target));
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
  if (result.error) {
    const missing = result.error.code === "ENOENT" || result.error.code === "EINVAL";
    const err = new Error(
      missing
        ? "esbuild not found on PATH (or $ESBUILD)" +
            (process.platform === "win32"
              ? "; on Windows point ESBUILD at esbuild.exe, not the .cmd shim"
              : "")
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
