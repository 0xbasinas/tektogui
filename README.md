# Tekto

Tekto is a small GUI framework for **desktop, mobile, and web**, from one codebase.

You write apps in **Rust** or in **TypeScript + JSX**. Both talk to the same scene tree, the same layout, and the same renderer. The runtime is ours: we do not wrap Electron, Tauri, or Flutter.

It is built so **coding agents** can make apps quickly: a dev-mode MCP server exposes the UI tree, screenshots, clicks, typed text, live patches, state, and logs. Hot reload and live edits work together in dev mode.

It is also built for **fast compile times**, **small apps**, and a **quiet, comfortable look** (plain typography and spacing first; themes later).

## Status

This is very early. The first slice can:

- Build a scene tree with **stable node ids**
- Lay out nodes with **Taffy**
- Draw a rectangle and text with the **tiny-skia CPU renderer** (Roboto is bundled)
- Fill an **AccessKit** tree (this is also the agent UI tree)
- Run **headless**, write a PNG, and pass a snapshot test
- Open a **window on Linux** (and it should compile for Windows too)
- Serve `tekto tree`, `tekto screenshot`, `tekto patch`, and MCP `ui.tree` from the same dev server

Android and Windows are compile-checked from day one. The WASM core, HostOp bridge, and CPU canvas blit are in. WebGPU is still later. See [ROADMAP.md](ROADMAP.md).

## Repo branches

- **`main`** is the default, stable-ish line.
- **`nightly`** is where work lands. **Open pull requests against `nightly`.**
- We merge `nightly` into `main` when a slice is ready.

## How to run

You need a recent Rust (1.88 or newer, 1.99 is fine).

```bash
# Headless PNG + print the tree
cargo run -p notes -- --headless --out notes.png

# Window (Linux / Windows / macOS), starts the dev server
cargo run -p notes

# In another terminal, while the window (or `tekto dev --headless`) is running:
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- screenshot shot.png
```

Run tests (includes the headless snapshot):

```bash
cargo test --workspace
```

JavaScript packages (signals and the JSX runtime) work with pnpm, npm, or bun:

```bash
npm test
# or: pnpm test
# or: bun test
```

## Workspace

Rust crates live in `crates/`. JavaScript packages live in `packages/`. The first example is `examples/notes` (Rust and a TypeScript sketch).

| Crate | Role |
| --- | --- |
| `tekto-tree` | Scene tree, stable ids, live `NodePatch` |
| `tekto-layout` | Taffy layout |
| `tekto-text` | Parley + bundled Roboto |
| `tekto-render` | Renderer trait, tiny-skia CPU backend, GPU stub |
| `tekto-platform` | winit / softbuffer on desktop; Android and web hooks |
| `tekto-a11y` | AccessKit mapping (agent tree) |
| `tekto-mcp` | Dev HTTP server + MCP stdio proxy |
| `tekto-js` | TS host protocol (QuickJS later; browser JS on web) |
| `tekto-native` | Files, clipboard, notifications, tray, shortcuts |
| `tekto-widgets` | Plain default widgets |
| `tekto` | Facade / Rust app API |
| `tekto-cli` | The `tekto` binary |

## License

MIT. New direct dependencies stay MIT, Apache-2.0, BSD, or zlib. Roboto Regular is bundled under Apache-2.0 (`crates/tekto-text/fonts/`). The text stack already brings Unicode License v3 transitives (ICU crates such as `icu_segmenter` and `zerovec`, plus `unicode-ident`). That license is permissive, not GPL or LGPL. On Windows, `arboard` also links Boost Software License 1.0 crates (`clipboard-win`, `error-code`). See the web-build and add-native-feature skills.

## Docs

- [Architecture](docs/architecture.md)
- [Getting started](docs/getting-started.md)
- [Agent guide](docs/agent-guide.md)
- [Notes for coding agents](AGENTS.md)
- [Roadmap](ROADMAP.md)
