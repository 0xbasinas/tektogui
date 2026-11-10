# Architecture

Tekto is a stack of small crates around one scene tree.

```
TypeScript + JSX (signals)     Rust app API (crate `tekto`)
        |                                |
        +----------- Host ops -----------+
                         |
                    tekto-tree
                         |
         +---------------+---------------+
         |               |               |
   tekto-layout    tekto-text      tekto-a11y
      (Taffy)       (Parley)      (AccessKit)
         |               |               |
         +-------+-------+               |
                 |                       |
           tekto-render                  |
        (trait + CPU now,                |
         GPU / web later)                |
                 |                       |
           tekto-platform                |
     desktop window | Android | web      |
                 |                       |
              dev server <---------------+
           (tekto-mcp)   agent MCP, CLI,
                         live patches
```

## Scene tree

`tekto-tree` owns nodes and **stable, monotonic ids**. Ids are never reused. That is what AccessKit, MCP tools, and live patches all use.

A `NodePatch` can change text, name, fill, or color at runtime. That is the live-edit path.

## Layout and text

`tekto-layout` maps nodes to Taffy (flex column or row, padding, gap, sizes).

`tekto-text` uses Parley and a bundled **Roboto Regular** (Apache-2.0) so headless screenshots look the same everywhere.

## Renderer

`tekto-render` is a trait:

- `CpuRenderer` (default): tiny-skia, glyph outlines via Skrifa. Used for windows (through softbuffer), headless PNGs, tests, and later a canvas2d-style web fallback.
- `GpuRenderer`: stub. wgpu / Vello (and WebGPU on the web) will live behind the `gpu` feature so default builds stay fast.

Apps never call tiny-skia or wgpu directly.

## Platform

`tekto-platform` hides the host:

- **Desktop (Windows, Linux, macOS):** winit window, softbuffer presents CPU frames.
- **Android:** `examples/notes-android` is a `NativeActivity` APK (`cargo-apk2`, `winit` + `android-activity`). `android_main` calls `tekto::run_android`. The window / softbuffer surface is created on `resumed` and dropped on `suspended`. The CPU core also still builds headless for host tests and for `cargo check --target aarch64-linux-android`.
- **Web:** planned. The Rust core builds to WASM. Painting uses WebGPU / WebGL2 (GPU backend) or the CPU renderer to a `<canvas>`. TypeScript runs in the **browser JS engine**, not QuickJS. The public API stays the same; no DOM in app code.

## Agents

`tekto-a11y` builds an AccessKit tree from the scene (same stable ids, plus roles, labels, bounds, and focus). `GET /accesskit` and MCP `ui.accesskit` return that dump. The desktop window rebuilds it after every layout so a live patch can stay in sync. Publishing to the OS (AT-SPI / UIA / VoiceOver) is behind the optional `a11y-os` feature, which pulls in `accesskit_winit` and is off by default so compile times stay short.


`tekto-mcp` starts a localhost HTTP server and writes `tekto-dev.json` (or `$TEKTO_DEV_FILE`). The `tekto` CLI and the stdio MCP adapter both use that server.

Dev-mode tools (first slice): `ui.tree`, `ui.screenshot`, `ui.click`, `ui.type`, `ui.patch`, `app.state`, `app.logs`. Record / replay and write-back to source files are planned.

Hot reload (rebuild or reload TypeScript) and live patches (change a running node) are both intended in dev mode.

## TypeScript

Packages under `packages/` (`@tektogui/core`, `@tektogui/jsx-runtime`, `create-tektogui`) must work with **pnpm, npm, and bun**. We use a standard `workspaces` field. Do not use pnpm-only fields in published `package.json` files.

`tekto-js` will embed QuickJS-ng on desktop / Android later, behind a feature flag. It is off by default because it is slow to compile.

## Native plugins

`tekto-native` is a small plugin API: files (read/write, open/save dialogs), clipboard, notifications, system tray, global shortcuts. The same API is meant to be called from Rust and from TypeScript. Each feature is filled per platform; stubs return a clear error.

## Compile times

- Default features: CPU renderer + desktop shell only.
- No wgpu, Vello, or QuickJS in the default graph.
- Dev profile: `debug = 1`, incremental on. A `dev-fast` profile exists if you want a bit of opt-level for demos.
- Later we can look at dylib / hot-patch for Rust, similar to Subsecond.

## Look and feel

Default widgets use a warm off-white page, charcoal ink, 24px page padding, 16px stack gap, and 20–22px text. No bright chrome. Animation, scroll feel, and themes are planned; the style hooks should not block them.
