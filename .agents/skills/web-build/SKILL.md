---
name: web-build
description: How to check the Tekto WASM core, the browser HostOp glue, the CPU canvas blit, and the optional WebGPU fallback. Use when touching wasm, tekto-wasm, @tektogui/wasm, or the web platform path.
---

# Web build

The Rust core builds for `wasm32-unknown-unknown`. TypeScript talks to it through the same `HostOp` JSON as desktop. The browser JS engine runs the app. QuickJS is not in this graph. The public app API (`@tektogui/core`, `@tektogui/jsx-runtime`, `@tektogui/wasm` entry) has no HTML, CSS, or DOM types.

CPU paint is the default. `@tektogui/wasm/canvas` (`paintCanvas`, `resizeCanvas`, `blitPng`) decodes the tiny-skia PNG and calls `putImageData`. `@tektogui/wasm/gpu` (`paintPreferred`, `choosePaintBackend`, `detectWebGpu`) is host glue too. Both take a structural canvas, not a DOM type in the app API. WebGPU is off unless the page passes `gpu: true` and the wasm file was built with `--features gpu`. Otherwise paint falls back to the CPU blit. WebGL2 is detected on a detached canvas and still uses that blit: Vello needs compute shaders. The paint canvas is not probed. If WebGPU takes a context and then fails, `paintPreferred` replaces that canvas so `getContext("2d")` can succeed. `PaintResult.surface` is the element that was painted.

## What builds

- `crates/tekto-wasm` — tree, layout, text, CPU renderer (`tiny-skia`), `HostOp` / `apply_ops`. No `quickjs`. `gpu` is optional and off by default.
- `packages/wasm` (`@tektogui/wasm`) — `connectWasm`, `applyOps`, `renderPng`, and `create` / `setText` / `setProp` / `append` / `remove` for `mount`. No DOM.
- `packages/wasm/src/canvas.js` (`@tektogui/wasm/canvas`) — blit and basic resize. No new crates.
- `packages/wasm/src/gpu.js` (`@tektogui/wasm/gpu`) — feature detect and CPU fallback. No new npm packages.
- `packages/wasm/dev.js` — one-command host page for a TSX entry (default Notes). `applyUiPatch` (`@tektogui/wasm/patch`) is the live patch hook. No new crates and no npm packages. The dev page uses the CPU blit.
- `cargo check -p tekto --target wasm32-unknown-unknown --no-default-features --features cpu,js` — the app facade without the desktop shell.

The default ABI adds no `wasm-bindgen` (the host calls the `tekto_*` exports directly). Direct deps on that default path stay MIT / Apache-2.0 / BSD / zlib. The text stack (Parley / ICU) already pulls Unicode License v3 crates into both the default host tree and `tekto-wasm`: `icu_segmenter`, `icu_normalizer`, `icu_provider`, `zerovec`, `writeable`, `yoke`, `tinystr`, and the other `icu_*` data crates, plus `unicode-ident` (`(MIT OR Apache-2.0) AND Unicode-3.0`). Unicode-3.0 is permissive. It is not MIT, Apache-2.0, BSD, or zlib, and it is not GPL or LGPL. Do not add a new direct dependency outside MIT / Apache-2.0 / BSD / zlib. Do not rip out the text stack to drop these transitives. The canvas path adds no npm packages.

The optional wasm GPU build adds direct deps only when `--features gpu` is on: `wasm-bindgen` 0.2.130 and `wasm-bindgen-futures` 0.4.80 (both MIT OR Apache-2.0) and `web-sys` 0.3.107 (MIT OR Apache-2.0, `HtmlCanvasElement`). wgpu's `webgpu` feature pulls `js-sys` under the same license. Do not enable this feature on PR CI. It is a large compile. `hexf-parse` (CC0-1.0) and the Unicode-3.0 ICU crates can show up through the existing desktop GPU and text stacks. They are not GPL or LGPL. Do not add a crate to remove them.

## Check

```bash
rustup target add wasm32-unknown-unknown
export CARGO_TARGET_DIR=/path/to/this/worktree/target

# CI commands (nightly push or workflow_dispatch, not every PR):
cargo build -p tekto-wasm --target wasm32-unknown-unknown
cargo check -p tekto --target wasm32-unknown-unknown --no-default-features --features cpu,js

# Node smoke (ordinary mode: skips if the wasm file or esbuild is missing):
node --test packages/wasm/test.js

# Required mode (what CI runs): missing module or esbuild fails, skips fail.
# esbuild must be on PATH or set in ESBUILD.
npm run test:wasm:required        # or: pnpm run test:wasm:required / bun run test:wasm:required
TEKTO_REQUIRE_WASM=1 node --test packages/wasm/test.js   # direct Node run, module test only
```

The wasm file is found in this order: `TEKTO_WASM`, then `$CARGO_TARGET_DIR/wasm32-unknown-unknown/debug/tekto_wasm.wasm`, then `target/...`. A relative `CARGO_TARGET_DIR` resolves from the repo root.

These are Node tests. They do not show that the wasm core or the host glue works in a browser. Browser runtime compatibility is not checked by CI.

`node --test packages/wasm/test.js` mounts the Notes JSX tree, compares `renderPng(640, 400)` to `crates/tekto/tests/snapshots/notes-headless.png`, blits those pixels onto a fake canvas, and resizes to 393×240. `node --test packages/wasm/tsx-test.js` bundles `examples/notes/src/app.tsx` with esbuild (MIT binary on `PATH`) and mounts that same sketch on the wasm host. It also starts the web dev server and checks `POST /patch`. No display. Do not use displays :1 or :2. In ordinary mode both tests skip when the wasm file or esbuild is missing. With `TEKTO_REQUIRE_WASM=1` (set by `npm run test:wasm:required`) a missing module or esbuild fails instead, and the script also fails if any test skips.

`RUSTFLAGS=-D warnings` is set in CI. Do not enable `quickjs` or `gpu` on these commands.

`node --test packages/wasm/test.js` also stubs WebGPU and WebGL2. No browser and no GPU. The flag-off path, a null adapter, WebGL2, a missing `paintWebGpu` export, and a thrown WebGPU paint all end on the CPU blit. One stub allows only one context type per canvas. Probes use `document.createElement`, not the paint canvas. A GPU context that then throws is replaced before the blit.

Optional WebGPU wasm module (not CI, not the default file):

```bash
cargo build -p tekto-wasm --target wasm32-unknown-unknown --features gpu
# Then wasm-bindgen on that wasm if you want to call `tekto_gpu_frame`.
# Install the glue as `host.paintWebGpu` and pass `{ gpu: true }` to paintPreferred.
```

Without that module, `packages/wasm/gpu.html` (open with `?gpu=1`) still loads the default wasm and blits the CPU PNG. Leave the query off and it does not ask for WebGPU at all.

## One-command dev

```bash
node packages/wasm/dev.js
```

That command builds `tekto-wasm` if the wasm file is missing (lookup: `TEKTO_WASM`, then `$CARGO_TARGET_DIR/wasm32-unknown-unknown/debug/tekto_wasm.wasm`, then `target/...`), bundles `examples/notes/src/app.tsx`, and serves `http://127.0.0.1:8080/`. Saving the TSX rebuilds with esbuild and reloads the page. The browser JS engine runs the app. QuickJS stays off.

Another entry: `node packages/wasm/dev.js path/to/app.tsx`. `PORT` picks the listen port. `TEKTO_WASM` picks the wasm file.

Live `ui.patch` uses the same JSON as desktop `POST /patch` (`id` plus `text`, `name`, `fill`, `color`, `fontSize`, `padding`, `gap`). The page applies it with `applyUiPatch` from `@tektogui/wasm/patch` and repaints. The wasm core does not return a node dump, so this response is `{ ok, delivered }`. Set `TEKTO_DEV_URL` to the desktop dev server (`http://127.0.0.1:<port>`) to forward the same body there. That keeps source writeback on the desktop server. The open page still updates from the web server event stream.

## Optional static smoke

```bash
cargo build -p tekto-wasm --target wasm32-unknown-unknown
cp "$CARGO_TARGET_DIR/wasm32-unknown-unknown/debug/tekto_wasm.wasm" packages/wasm/tekto_wasm.wasm
# from packages/wasm: python3 -m http.server 8080
# open http://127.0.0.1:8080/notes.html   (Notes at 640×400, resize buttons)
# open http://127.0.0.1:8080/gpu.html     (CPU blit; ?gpu=1 tries WebGPU, then the blit)
# open http://127.0.0.1:8080/smoke.html   (one text node, PNG in an img)
```

Do not commit `tekto_wasm.wasm`. `notes.html` is a host page. It is not the app API.

## Design that must stay true

- CPU first: `renderPng` is the tiny-skia path. The host blits that PNG. WebGPU stays behind `tekto-wasm`'s `gpu` feature and `paintPreferred({ gpu: true })`. Missing WebGPU, WebGL2-only, or a failed GPU paint falls back to this canvas path. App ops are unchanged.
- Desktop QuickJS stays behind `--features quickjs` (off by default, not on PR CI).
- `tekto-platform` keeps the desktop shell cfg'd out on `target_arch = "wasm32"`.
- Apps never see DOM types. `paintCanvas` lives in the host glue only.
