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
cargo run -p tekto-cli -- patch 4 --text "New title"
cargo run -p tekto-cli -- patch 4 --color "#2c333a" --font-size 20
cargo run -p tekto-cli -- click 4
```

`tree` prints the scene / AccessKit tree with stable ids, roles, names, and bounds. The id is positional (`tekto patch 4`, not `--id`).

## MCP

```bash
cargo run -p tekto-cli -- mcp
```

This speaks the Model Context Protocol on stdio and proxies to the same HTTP server.

Tools in the first slice:

| Tool | What it does |
| --- | --- |
| `ui.tree` | Dump the tree (pretty text + JSON) |
| `ui.accesskit` | Dump AccessKit roles, labels, bounds, and focus (same ids) |
| `ui.screenshot` | CPU-rendered PNG |
| `ui.click` | Click a node id (focus + last_click; a real window click uses the same hit test) |
| `ui.type` | Type into a node id (stubs a field until IME lands) |
| `ui.patch` | Live-edit text, name, fill, color, or fontSize on a running node. Returns `{ ok, node }`. |
| `app.state` | Opt-in JSON state (empty for now) |
| `app.logs` | Recent log lines |

Live patches change the running tree immediately and do not require a reload. `POST /patch` and `ui.patch` accept JSON `{ "id", "text?", "name?", "fill?", "color?", "fontSize?" }` (`#rrggbb` or `#rrggbbaa`) and return the updated node, or a clear error (unknown id, bad color). Padding and other layout styles are not in this slice.

Writing those edits back into `.rs` or `.tsx` source is a separate step (issue #19).

## Hot reload TypeScript

Rebuilds only the JS bundle and swaps it into the running QuickJS runtime. The Rust core stays up. Needs `esbuild` on `PATH` and a CLI built with `--features quickjs` (do not enable that feature on PR CI).

```bash
cargo run -p tekto-cli --features quickjs -- dev --js examples/notes/src/app.tsx --headless --out /tmp/notes-js.png
```

Saving the `.tsx` prints `js reload Nms`. JS remounts from source, so signal state resets. Failures go to stderr and `app.logs`. `POST /reload` accepts a JS bundle (or JSON `{"js":"..."}`) so a host can push over `adb forward`; the current Android APK has no QuickJS, so that hook waits for a later APK.

## Headless tests

Prefer the CPU renderer, a fixed size (640×400 is what Notes uses), and the bundled Roboto font. That keeps screenshots deterministic.

```bash
TEKTO_SHOT_DIR=/tmp/shots cargo test -p tekto --test headless_snapshot
```

## What not to do

- Do not drive the app through a browser DOM. There is no DOM in the app API.
- Do not invent a second id scheme. Use `NodeId`.
- Do not add a heavy crate just to make a demo nicer. Keep default builds lean.
