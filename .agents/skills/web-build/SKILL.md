---
name: web-build
description: How to check the Tekto WASM core and the browser HostOp glue. Use when touching wasm, tekto-wasm, @tektogui/wasm, or the web platform path. Canvas paint is still a later issue.
---

# Web build

The Rust core builds for `wasm32-unknown-unknown`. TypeScript talks to it through the same `HostOp` JSON as desktop. The browser JS engine runs the app. QuickJS is not in this graph. The public app API has no HTML, CSS, or DOM types.

Canvas blit and WebGPU paint are not in this crate (`#41`). `packages/wasm/smoke.html` only proves ops plus PNG bytes.

## What builds

- `crates/tekto-wasm` — tree, layout, text, CPU renderer (`tiny-skia`), `HostOp` / `apply_ops`. No `quickjs`, no `gpu`.
- `packages/wasm` (`@tektogui/wasm`) — small JS glue: `connectWasm`, `applyOps`, `renderPng`, and `create` / `setText` / `setProp` / `append` / `remove` for `mount`.
- `cargo check -p tekto --target wasm32-unknown-unknown --no-default-features --features cpu,js` — the app facade without the desktop shell.

No new crates were added for the ABI (no `wasm-bindgen`). Existing deps stay MIT / Apache-2.0 / BSD / zlib.

## Check

```bash
rustup target add wasm32-unknown-unknown
export CARGO_TARGET_DIR=/path/to/this/worktree/target

# CI command (nightly push or workflow_dispatch, not every PR):
cargo check -p tekto-wasm --target wasm32-unknown-unknown
cargo check -p tekto --target wasm32-unknown-unknown --no-default-features --features cpu,js

# Module + Node smoke (skips if the wasm file is missing):
cargo build -p tekto-wasm --target wasm32-unknown-unknown
node --test packages/wasm/test.js
```

`RUSTFLAGS=-D warnings` is set in CI. Do not enable `quickjs` or `gpu` on these commands.

## Optional browser smoke

```bash
cargo build -p tekto-wasm --target wasm32-unknown-unknown
cp "$CARGO_TARGET_DIR/wasm32-unknown-unknown/debug/tekto_wasm.wasm" packages/wasm/tekto_wasm.wasm
# from packages/wasm: python3 -m http.server 8080
# open http://127.0.0.1:8080/smoke.html
```

Do not commit `tekto_wasm.wasm`. Do not add a canvas Notes window here.

## Design that must stay true

- Paint later: blit the CPU PNG (or RGBA) to a `<canvas>`, then WebGPU / WebGL2 behind `gpu`. Fall back to canvas if WebGPU is missing.
- Desktop QuickJS stays behind `--features quickjs` (off by default, not on PR CI).
- `tekto-platform` keeps the desktop shell cfg'd out on `target_arch = "wasm32"`.
- Apps never see DOM types. The smoke page is not the app API.
