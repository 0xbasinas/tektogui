# Round 3 roadmap

Status: 2026-11-11. `nightly` is at `a263750` with all five CI jobs green. This file tracks what shipped in round 3, what is still open, and what was rejected. It is not a release promise or a list of issues. Sizes are rough: **S** a few days, **M** one to two weeks, **L** larger or cross-platform.

Tekto keeps its own scene tree and stable ids, first-class Rust and TSX APIs, agent-first tools, lean default builds, and a plain default look.

## Shipped

| Item | PR | Result |
| --- | --- | --- |
| Animation | #191 | Fill and text-colour tweens, four easings, ping-pong repeat, reduced motion |
| Native permissions | #192 | `applyNativeOp` allow-list per capability family |
| Design tokens | #193 | `Tokens`, `setTokens`, Rust and TSX; defaults unchanged |
| MCP flake fix | #194 | Overload test polls instead of sleeping |
| Responsive layout | #195 | `responsive_axis`, `split_pane`, `grow`, `collapseBelow` |
| Pointer capture | #196 | Capture lives in the tree; slider drag survives leaving bounds |
| Keyed lazy rows | #197 | `KeyedList` keeps under 20 row nodes for 1000 keys |
| Native typings | #198 | `native-ops.json` generates the TS union; Rust drift test |
| QuickJS bytecode | #199 | Versioned `TEKTOQBC` envelope, loader, `tekto bytecode` |
| Browser parity | #200, #201 | Headless Chrome drives mouse, touch, menu, tab and field input |
| App bar | #206 | `app_bar`, quiet actions, TSX helpers |
| Window chrome | #207 | Undecorated window, drag region, minimise/maximise/close |
| Resize and snap | #208 | Edge/corner resize, double-click maximise, edge snapping |
| Resize precision | #209 | Screen-space deltas, one atomic rect per move |
| Resize pacing | #210 | One layout per resize frame, global cursor |
| GPU surface present | #211 | Vello draws into a wgpu window surface; no readback |

Try the demo: `cargo run -p round3-demo --features gpu`. Undecorated window, draggable bar, working window controls, edge/corner resize, snap, blinking caret, repeating pulse, responsive split pane, slider capture.

## Still open

**Text and input**
- Real IME composition on Windows: the route exists, no IME layout has driven it.
- Android `EditText` overlay: the APK runs, but Notes has no field to type into.
- Browser IME and touch selection: wiring exists, not exercised in a real browser.
- Mixed bidirectional lines: one run today; per-run direction and visual caret movement are open.
- Field copy/paste lives in the clipboard probe, not an end-to-end field test.

**Window chrome**
- TSX props for `dragWindow` and `windowControl` are not exposed, so TSX apps cannot opt into undecorated chrome by themselves.
- Snapping uses the monitor rectangle; quarter heights are not taskbar-aware.
- The Windows 11 snap-layouts flyout is not reproduced.
- No drop shadow, rounded window corners, or overflow menu.

**Lists, drag, animation**
- No `ui.drag { from, to }`, no list reorder, no drag callbacks, no pinch.
- `KeyedList` is app-driven: the host must call `sync`; no TSX binding, variable heights, grid, or focus rule when a focused row leaves.
- No rotation channel, keyframes, springs, or timelines; tweens use the 180 ms default.

**Performance**
- Text shaping still runs whenever layout does; resize frames that change wrap width can spend 40–60 ms in layout. That is the remaining stutter source.
- No partial damage region: every frame repaints the whole scene.
- `TEKTO_FRAME_LOG=1` is the only frame timing tool (debug builds).

**Accessibility and packaging**
- Linux AT-SPI smoke test: read, activate, toggle, type, focus events.
- Packaging manifest, Android SAF picker, deep links, share sheet, read-only rich text spans.
- Generated-app builds pass on nightly but not PRs, and not against a published package.

**Later crates**
- `tekto-navigation`: push/pop route stack, route visible to agents, back restores focus.
- `tekto-i18n`: locale, string lookup, plural variants, RTL layout.
- `tekto-data-grid`: read-only table with realised rows and semantic cells.
- `tekto-charts`: bar/line with title, values and a semantic fallback.
- `tekto-svg`, `tekto-update`, `tekto-webview`: deferred; licence check first.

## Known limits

- GPU paths were checked on one NVIDIA adapter. Other drivers, Android GPUs, and browser WebGPU are unverified.
- Undecorated chrome is Windows-first; non-Windows falls back to `set_outer_position` + `request_inner_size` for resize.
- Tokens affect nodes created after `set_tokens`; there is no live re-theme.
- QuickJS bytecode is crate/CLI level; no release path loads a `.qbc` yet.
- Browser parity and IME checks are manual, not CI.

## Checks

Default verification: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo check -p tekto-wasm --target wasm32-unknown-unknown`, `npm test`.

Optional: `cargo test -p tekto --features gpu`, `cargo test -p tekto-js --features quickjs`, `npm run test:browser`.

## Not doing

- No wrapping Flutter, Compose, React Native, Tauri, Electron, Wails, Avalonia, MAUI, Uno, Qt, Dioxus Native, Blitz, or Masonry.
- No Slint (GPL or credit/embedded restrictions), no Zed app code (GPL), no Chromium/CEF/Servo/WebKitGTK. A web view, if ever added, stays opt-in and never the default renderer.
- No Skia/Impeller replacement, and no GPU, QuickJS, or OS accessibility in the default graph or on PR CI.
- No Material, Fluent, or Cupertino clone and no large theme system. Small tokens are enough.
- No maps, video, audio, spreadsheet, or full rich-text editor yet.
- No screenshot-only agent actions and no unrestricted shell through the dev server.
- New direct dependencies must be MIT, Apache-2.0, BSD, or zlib, with attribution for reused code. Confirm every licence upstream before adding it.

Windows runtime checks, macOS and iOS work, and merging `nightly` into `main` remain separate decisions.
