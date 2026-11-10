---
name: web-build
description: How the future Tekto web target should be built and how to keep today’s platform/JS layers ready for it. Use when touching wasm, web paint, or tekto-platform / tekto-js abstractions. There is no working WASM build or `tekto build --web` command yet.
---

# Web build

The web target is planned (Month 4, issues for WASM, canvas paint, WebGPU, and browser JS). It is **not implemented**. Do not invent a bundler command that does not exist. Do keep the abstractions ready.

## Design that must stay true

- Rust core (tree, layout, text, CPU renderer, ops) builds to WASM later (`wasm32-unknown-unknown` or wasm-bindgen).
- Paint: first blit the CPU renderer (tiny-skia) to a `<canvas>`; later WebGPU / WebGL2 behind the `gpu` feature. Fallback to canvas if WebGPU is missing.
- TypeScript apps run in the **browser JS engine**, not QuickJS. `tekto-js` is an op protocol (`HostOp`, `Host::apply`). Desktop will embed QuickJS-ng behind `quickjs` (off by default). Web calls the same ops on the WASM module.
- Public app API has **no** HTML, CSS, or DOM types. The platform paints our tree; apps never see the DOM.
- `tekto-platform` already cfg-gates `desktop` out on `target_arch = "wasm32"`. `tekto-native` file read/write is stubbed on wasm (`Unsupported`).

## Files to touch when you start the real work

- `crates/tekto-platform` — a web backend next to `desktop.rs`
- `crates/tekto-render` — canvas blit; GPU path only behind `gpu`
- `crates/tekto-js` — do not enable `quickjs` on web
- `packages/core`, `packages/jsx-runtime` — same JSX API as desktop
- `.github/workflows/ci.yml` — wasm check only on nightly or `workflow_dispatch` (save Actions minutes)
- This skill, **in the same PR**, with the exact commands you added

## Temporary verify (today)

```bash
# There is no wasm job yet. At least keep desktop/default builds green
# and avoid DOM types in public APIs:
cargo check --workspace --all-targets
npm test
```

When a wasm build exists, this section should list the exact `cargo check --target wasm32-unknown-unknown …` (or wasm-bindgen) command and how to open the Notes example in a browser.
