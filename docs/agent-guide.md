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
cargo run -p tekto-cli -- inspect  # print the browser inspector URL
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- screenshot /tmp/shot.png
cargo run -p tekto-cli -- patch 4 --text "New title"
cargo run -p tekto-cli -- patch 4 --color "#2c333a" --font-size 20 --padding 24
cargo run -p tekto-cli -- click 4
```

`tree` prints the scene / AccessKit tree with stable ids, roles, names, and bounds. The id is positional (`tekto patch 4`, not `--id`).

The inspector at `GET /inspector` shows the live tree. Select a node, choose properties, and apply a patch. Refresh the tree after changes from elsewhere. `tekto inspect` discovers the server through the same endpoint file as `tekto tree`.

Agents can use the same HTTP server directly: `GET /health` checks availability, `GET /tree` returns JSON with `pretty` and `dump` (including node ids and children), and `GET /tree.txt` returns the readable tree. `POST /patch` accepts a JSON object with the node `id` and properties to change, and returns the updated node and any writeback result. Errors return a JSON `error` and an HTTP error status.

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
| `ui.type` | Insert plain characters at the caret of a text field and focus it. Not an IME |
| `ui.patch` | Live-edit text, name, fill, color, or fontSize on a running node. Returns `{ ok, node }`. |
| `app.state` | Opt-in JSON state (empty for now) |
| `app.logs` | Recent log lines |

Live patches change the running tree immediately and do not require a reload. `POST /patch` and `ui.patch` accept JSON `{ "id", "text?", "name?", "fill?", "color?", "fontSize?", "padding?", "gap?" }` (`#rrggbb` or `#rrggbbaa`) and return the updated node, or a clear error (unknown id, bad color).

A list (`kind` `list`) is a column of `list_item` rows. Each row's id is stable and is the AccessKit `ListItem` id. The list id is the AccessKit `List`. A tall list from `tekto::list` has a `scroll` parent; a short one does not. Rows are not virtualized.

A text field (`kind` `text_field`) stores its committed value in `text` and its accessible label in `name`. `ui.patch` with `text` replaces the value, clears any preedit, and leaves `name` alone. `ui.type` inserts plain characters at the caret (control characters are skipped), drops a preedit without committing it, and focuses that field. Backspace is a desktop key on the focused field, not part of `ui.type`. AccessKit role is `TextInput`, with the same stable id and label `name`. The AccessKit value is the committed text plus the current preedit, if the desktop IME is composing.

Desktop IME (Linux and Windows, same winit path): when a text field is focused the window allows IME. `Preedit` shows composing text at the caret. `Commit` writes it into `text` (accents and CJK strings). A click moves the caret to the end and clears a preedit. Moving focus also clears the previous field's preedit, so AccessKit does not keep a stale composition. Printable keys use the same guard as Backspace and are ignored while a preedit is showing. A Windows commit echo is dropped only until the event queue goes idle; a later press of that same string is kept.

Still open: we do not draw a candidate window (the OS may, using the caret spot we set), no marked-range highlight, no selection, arrow keys, copy, or paste. Android has no soft keyboard. This environment does not run a Windows IME, so that path is compile-shared and not exercised on a Windows desktop. `ui.type` is still plain text, not a composition.

When the old text or a simple style literal (`"#rrggbb"`, `fontSize={N}`) appears once in the app project, `ui.patch` / `tekto patch` also writes that change back to source. The JSON includes a `writeback` object (`written`, `skipped`, or `partial`) and the reason if it did not write. Ambiguous literals, named colors such as `Color::BANNER`, and files outside the app project (`TEKTO_APP_ROOT`, or the `--js` entry's app folder) are left as runtime-only changes. `crates/`, `target/`, `node_modules/`, and `docs/` are never written.

## Hot reload TypeScript

Rebuilds only the JS bundle and swaps it into the running QuickJS runtime. The Rust core stays up. Needs `esbuild` on `PATH` and a CLI built with `--features quickjs` (do not enable that feature on PR CI).

```bash
cargo run -p tekto-cli --features quickjs -- dev --js examples/notes/src/app.tsx --headless --out /tmp/notes-js.png
```

Saving the `.tsx` prints `js reload Nms`. JS remounts from source, so signal state resets. Failures go to stderr and `app.logs`. `POST /reload` accepts a JS bundle (or JSON `{"js":"..."}`) so a host can push over `adb forward`. On Android, `tekto dev --js examples/notes/src/app.tsx --android` builds the APK with `-F quickjs` and posts the bundle (the host CLI does not need `--features quickjs`). The default APK stays without QuickJS so nightly Android CI stays lean.

## Headless tests

Prefer the CPU renderer, a fixed size (640×400 is what Notes uses), and the bundled Roboto font. That keeps screenshots deterministic.

```bash
TEKTO_SHOT_DIR=/tmp/shots cargo test -p tekto --test headless_snapshot
```

## What not to do

- Do not drive the app through a browser DOM. There is no DOM in the app API.
- Do not invent a second id scheme. Use `NodeId`.
- Do not add a heavy crate just to make a demo nicer. Keep default builds lean.

### Record and replay tree actions

`tekto::Recording` stores versioned, serde-compatible actions for pointer clicks,
field keys, and the tree operations behind `ui.click`, `ui.type`, and `ui.patch`.
Use `record(&mut tree, action)` to apply an action and append it to a log, then
serialize the recording with `serde_json`. Replay it on the same initial tree
with `replay(&mut tree, layout)`. Lay out the tree before recording or replaying;
use the callback to lay it out after each action so pointer coordinates see the
current bounds. The helper records actions passed to it; it does not automatically
capture a live window or dev-server session, run app callbacks, or write patches
back to source. See `crates/tekto/tests/record_replay.rs` and its JSON fixture.
