---
name: web-build
description: How to check the Tekto WASM core, the browser HostOp glue, and the CPU canvas blit. Use when touching wasm, tekto-wasm, @tektogui/wasm, or the web platform path.
---

# Web build

The Rust core builds for `wasm32-unknown-unknown`. TypeScript talks to it through the same `HostOp` JSON as desktop. The browser JS engine runs the app. QuickJS is not in this graph. The public app API (`@tektogui/core`, `@tektogui/jsx-runtime`, `@tektogui/wasm` entry) has no HTML, CSS, or DOM types.

CPU paint is in. `@tektogui/wasm/canvas` (`paintCanvas`, `resizeCanvas`, `blitPng`) decodes the tiny-skia PNG and calls `putImageData`. That module is host glue. It takes a structural canvas (`getContext("2d")`), not a DOM type in the app API. WebGPU is still `#42`.

## What builds

- `crates/tekto-wasm` — tree, layout, text, CPU renderer (`tiny-skia`), `HostOp` / `apply_ops`. No `quickjs`, no `gpu`.
- `packages/wasm` (`@tektogui/wasm`) — `connectWasm`, `applyOps`, `renderPng`, and `create` / `setText` / `setProp` / `append` / `remove` for `mount`. No DOM.
- `packages/wasm/src/canvas.js` (`@tektogui/wasm/canvas`) — blit and basic resize. No new crates.
- `cargo check -p tekto --target wasm32-unknown-unknown --no-default-features --features cpu,js` — the app facade without the desktop shell.

No new crates were added for the ABI (no `wasm-bindgen`). Direct deps added for this path stay MIT / Apache-2.0 / BSD / zlib. The text stack (Parley / ICU) already pulls Unicode License v3 crates into both the default host tree and `tekto-wasm`: `icu_segmenter`, `icu_normalizer`, `icu_provider`, `zerovec`, `writeable`, `yoke`, `tinystr`, and the other `icu_*` data crates, plus `unicode-ident` (`(MIT OR Apache-2.0) AND Unicode-3.0`). Unicode-3.0 is permissive. It is not MIT, Apache-2.0, BSD, or zlib, and it is not GPL or LGPL. Do not add a new direct dependency outside MIT / Apache-2.0 / BSD / zlib. Do not rip out the text stack to drop these transitives. The canvas path adds no npm packages.

## Check

```bash
rustup target add wasm32-unknown-unknown
export CARGO_TARGET_DIR=/path/to/this/worktree/target

# CI command (nightly push or workflow_dispatch, not every PR):
cargo check -p tekto-wasm --target wasm32-unknown-unknown
cargo check -p tekto --target wasm32-unknown-unknown --no-default-features --features cpu,js

# Module + Node smoke (skips pixel checks if the wasm file is missing):
cargo build -p tekto-wasm --target wasm32-unknown-unknown
node --test packages/wasm/test.js
```

`node --test packages/wasm/test.js` mounts the Notes JSX tree, compares `renderPng(640, 400)` to `crates/tekto/tests/snapshots/notes-headless.png`, blits those pixels onto a fake canvas, and resizes to 393×240. No display. Do not use displays :1 or :2.

`RUSTFLAGS=-D warnings` is set in CI. Do not enable `quickjs` or `gpu` on these commands.

## Optional browser smoke

```bash
cargo build -p tekto-wasm --target wasm32-unknown-unknown
cp "$CARGO_TARGET_DIR/wasm32-unknown-unknown/debug/tekto_wasm.wasm" packages/wasm/tekto_wasm.wasm
# from packages/wasm: python3 -m http.server 8080
# open http://127.0.0.1:8080/notes.html   (Notes at 640×400, resize buttons)
# open http://127.0.0.1:8080/smoke.html   (one text node, PNG in an img)
```

Do not commit `tekto_wasm.wasm`. `notes.html` is a host page. It is not the app API.

## Design that must stay true

- CPU first: `renderPng` is the tiny-skia path. The host blits that PNG. WebGPU / WebGL2 stays behind `gpu` (`#42`). Fall back to this canvas path if WebGPU is missing.
- Desktop QuickJS stays behind `--features quickjs` (off by default, not on PR CI).
- `tekto-platform` keeps the desktop shell cfg'd out on `target_arch = "wasm32"`.
- Apps never see DOM types. `paintCanvas` lives in the host glue only.
