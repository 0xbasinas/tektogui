import { resolve } from "node:path";

// Cargo resolves a relative CARGO_TARGET_DIR from its working directory.
export function wasmPath(root) {
  return process.env.TEKTO_WASM || resolve(
    root,
    process.env.CARGO_TARGET_DIR || "target",
    "wasm32-unknown-unknown/debug/tekto_wasm.wasm",
  );
}
