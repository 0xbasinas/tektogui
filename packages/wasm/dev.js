/**
 * One command for the web Notes sketch:
 *   node packages/wasm/dev.js
 *
 * Builds tekto-wasm when the module is missing, bundles
 * examples/notes/src/app.tsx with esbuild, and serves the host page.
 * Saving the TSX reloads the page. POST /patch is live ui.patch.
 * Set TEKTO_DEV_URL to the desktop dev server to forward writeback.
 */

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";

import { repoRoot } from "./src/bundle-app.js";
import { startDevServer } from "./src/dev-server.js";
import { wasmPath as findWasmPath } from "./src/wasm-path.js";

const root = repoRoot();
const entry = process.argv[2] || resolve(root, "examples/notes/src/app.tsx");
const wasmPath = findWasmPath(root);

if (!existsSync(wasmPath)) {
  const build = spawnSync(
    "cargo",
    ["build", "-p", "tekto-wasm", "--target", "wasm32-unknown-unknown"],
    { cwd: root, stdio: "inherit", env: process.env },
  );
  if (build.status !== 0) {
    process.stderr.write("cargo build -p tekto-wasm failed\n");
    process.exit(build.status || 1);
  }
}

const server = await startDevServer({
  entry,
  wasmPath,
  port: Number(process.env.PORT || 8080),
  forwardUrl: process.env.TEKTO_DEV_URL || "",
  watch: true,
});

process.stdout.write(`web dev ${server.url}\n`);
process.stdout.write(`TSX ${entry}\n`);
process.stdout.write("Saving the TSX rebuilds the bundle and reloads the page.\n");
process.stdout.write("POST /patch applies ui.patch on the open page (same JSON as desktop).\n");
if (process.env.TEKTO_DEV_URL) {
  process.stdout.write(`Also forwarding /patch to ${process.env.TEKTO_DEV_URL}\n`);
} else {
  process.stdout.write("Set TEKTO_DEV_URL to the desktop dev server to forward writeback.\n");
}
