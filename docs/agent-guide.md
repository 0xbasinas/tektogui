# Agent guide

Tekto is meant to be driven by coding agents as well as by humans.

## Start an app you can inspect

```bash
# Headless, keeps the dev server up until Enter
cargo run -p tekto-cli -- dev --headless --out /tmp/notes.png
```

Or run the windowed Notes example. The process writes a small file (default: `$XDG_RUNTIME_DIR/tekto-dev.json`, or `/tmp/tekto-dev.json`) with the dev server port. You can override the path with `TEKTO_DEV_FILE`.

## CLI tools (same server as MCP)

```bash
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- screenshot /tmp/shot.png
cargo run -p tekto-cli -- patch --id 4 --text "New title"
cargo run -p tekto-cli -- click 4
```

`tree` prints the scene / AccessKit tree with stable ids, roles, names, and bounds.

## MCP

```bash
cargo run -p tekto-cli -- mcp
```

This speaks the Model Context Protocol on stdio and proxies to the same HTTP server.

Tools in the first slice:

| Tool | What it does |
| --- | --- |
| `ui.tree` | Dump the tree (pretty text + JSON) |
| `ui.screenshot` | CPU-rendered PNG |
| `ui.click` | Click a node id (stub until input is wired) |
| `ui.type` | Type into a node id (stub until IME / fields land) |
| `ui.patch` | Live-edit text, name, fill, or color on a running node |
| `app.state` | Opt-in JSON state (empty for now) |
| `app.logs` | Recent log lines |

Live patches change the running tree immediately. Writing those edits back into `.rs` or `.tsx` source is planned; do not pretend it already works.

Hot reload (reload app code) and live patches (change a live node) should both be available in dev mode as they land.

## Headless tests

Prefer the CPU renderer, a fixed size (640×400 is what Notes uses), and the bundled Roboto font. That keeps screenshots deterministic.

```bash
TEKTO_SHOT_DIR=/tmp/shots cargo test -p tekto --test headless_snapshot
```

## What not to do

- Do not drive the app through a browser DOM. There is no DOM in the app API.
- Do not invent a second id scheme. Use `NodeId`.
- Do not add a heavy crate just to make a demo nicer. Keep default builds lean.
