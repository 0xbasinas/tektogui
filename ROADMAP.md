# Roadmap

Work is tracked in GitHub milestones (Month 1 … Month 6). Open PRs against `nightly`.

Priorities that apply every month: agents can move fast, compile times stay short, apps stay small, and the default UI looks quiet and feels good.

## Month 1 — engine embryo

Already in progress in the first PRs:

- Scene tree with stable ids, Taffy layout, Parley text, tiny-skia CPU renderer
- AccessKit tree + dev server + `tekto tree` / MCP `ui.tree` + headless PNG test
- Linux window (winit + softbuffer)
- Windows and Android compile of the CPU core
- Bundled Roboto, quiet default spacing and type
- Lean default features; GPU and QuickJS off
- Live patch API on the running tree
- Platform and JS abstractions that can accept a web target later
- `tekto-native` plugin stubs

Still in Month 1:

- Windows window actually run and checked on a real machine
- Hook AccessKit into the live winit window (not only the MCP dump)
- Click handling from the window into the tree
- Dev-profile / compile-time notes and CI caches proven on GitHub
- `tekto new` scaffold that is actually useful

## Month 2 — TypeScript and richer dev

- Solid-style signals + JSX talking to the Rust tree (QuickJS-ng, feature-flagged)
- TS hot reload on desktop
- Live edits write back to source where we can
- Android window / activity, `Resumed` / `Suspended` handled safely
- More widgets (button and a basic text field are in; list is still to do). Full IME stays in Month 3
- Clipboard and basic file read/write on Windows and Linux
- Faster incremental builds; investigate Rust dylib / hot-patch
- Scroll container (basic)

## Month 3 — feel, GPU start, packing

- wgpu / Vello GPU backend behind the `gpu` feature
- Desktop IME for a real text field
- Record / replay of pointer, key, and MCP actions
- Simple inspector UI talking to the same dev server
- Notifications on desktop
- First pass at animation (transitions, no flashy effects)
- Linux / Windows packaging experiments (Linux tarball script and a Windows zip layout are documented in `docs/packaging.md`; no signing)

## Month 4 — web and more native

- WASM build of the core
- Web paint: CPU renderer to canvas, then WebGPU if the GPU backend is ready
- TypeScript apps on the browser engine, same JSX API
- File open/save dialogs on Windows and Linux
- System tray and global shortcuts on desktop
- Better list virtualization and scroll feel

## Month 5 — mobile polish and plugins

- Android IME or a native text-field overlay
- Safe areas and keyboard insets
- GPU path on Android if drivers allow
- Notifications, clipboard, and files on Android
- Animation and scroll feel on touch
- macOS catch-up if we have a Mac

## Month 6 — a real side app

- Time-travel for opted-in stores
- `app.components` schemas for agents
- iOS hello (needs a Mac)
- One real example app people can use
- CI: headless replay + screenshots; documented store signing
- Honest list of what is still weaker than Flutter or a native toolkit

Web, GPU, iOS, and perfect IME may slip. If they do, we cut scope rather than rush a wrapper around someone else's engine.
