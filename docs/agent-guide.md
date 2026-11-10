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
cargo run -p tekto-cli -- states
cargo run -p tekto-cli -- restore 0
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
| `app.state` | Focus and last click as JSON |
| `app.states` | List time-travel frames. `enabled` is false unless the app opted in |
| `app.restore` | Jump that store to a recorded `index` |
| `app.logs` | Recent log lines |
| `app.components` | JSON schema of widgets, props, and events. No running app required |

`app.components` (and `GET /components`) returns `packages/jsx-runtime/src/components.json`. That file is the list `@tektogui/jsx-runtime` mounts. Kinds match `NodeKind` (`window`, `group`, `rect`, `text`, `button`, `text_field`, `scroll`, `list`, `list_item`). Each kind lists every prop the host `setProp` accepts (`width`, `height`, `column`, `justifyCenter`, `text`, `name`, `fill`, `color`, `fontSize`, `padding`, `gap`), because JSX forwards that set on every kind. `title` is an alias of `name` (`font_size` and `justify_center` are the snake_case aliases). Events are host actions, not `onClick` props: `click` on every kind, plus `type`, `key`, and `ime` on `text_field`, and `scroll` on `scroll`. The Rust `banner()` helper is a `rect`, not its own kind.

Live patches change the running tree immediately and do not require a reload. `POST /patch` and `ui.patch` accept JSON `{ "id", "text?", "name?", "fill?", "color?", "fontSize?", "padding?", "gap?" }` (`#rrggbb` or `#rrggbbaa`) and return the updated node, or a clear error (unknown id, bad color).

A list (`kind` `list`) is a column of `list_item` rows. Each row's id is stable and is the AccessKit `ListItem` id. The list id is the AccessKit `List`. A tall list from `tekto::list` has a `scroll` parent; a short one does not. After layout, a scrolling list keeps only the visible rows plus 64px overscan as children. The window is the list's own position in the scroll, so padding and earlier siblings count. Rows outside that window are not painted. Their ids still exist until the list is removed or the row is moved with `append`. Scroll height stays the full stack.

A text field (`kind` `text_field`) stores its committed value in `text` and its accessible label in `name`. `ui.patch` with `text` replaces the value, clears any preedit, and leaves `name` alone. `ui.type` inserts plain characters at the caret (control characters are skipped), drops a preedit without committing it, and focuses that field. Backspace is a desktop key on the focused field, not part of `ui.type`. AccessKit role is `TextInput`, with the same stable id and label `name`. The AccessKit value is the committed text plus the current preedit, if the desktop IME is composing.

Desktop IME (Linux and Windows, same winit path): when a text field is focused the window allows IME. `Preedit` shows composing text at the caret. `Commit` writes it into `text` (accents and CJK strings). A click moves the caret to the end and clears a preedit. Moving focus also clears the previous field's preedit, so AccessKit does not keep a stale composition. Printable keys use the same guard as Backspace and are ignored while a preedit is showing. A Windows commit echo is dropped only until the event queue goes idle; a later press of that same string is kept.

Android does not use winit IME events. winit only shows or hides the soft keyboard, and `NativeActivity` text state is a no-op. When a text field is focused, the APK shows a native `EditText` (`com.tektogui.ime.FieldOverlay`, compiled from `examples/notes-android/java` by cargo-apk2) over the field. The loop polls that string into the focused field's `text` and caret, so the Tekto tree, AccessKit, and MCP see the value. The overlay applies that push on the UI thread. A poll that still shows the previous string is not treated as typing. Carets in the tree are Unicode scalar indexes. The `EditText` caret is UTF-16, and the boundary converts both ways. An agent patch of `text` or of the caret is pushed back into the `EditText`. Keyboard events are ignored only while that overlay is focused on a text field. The label `name` stays put. The Android IME's own candidate UI is used. Tekto does not store a preedit on this path: the overlay string is the committed value, including text the IME has not confirmed yet. A live type-into-field check still needs a free emulator or device. This host must not start the emulator next to a heavy build. If the Java class is missing, the window falls back to Android key events only. `FieldOverlay.insets` reads system bars, the display cutout, and the IME in physical pixels. The shell converts them with the window scale and lays the tree out in the remaining box (`layout_with_insets`). The bottom pad is the larger of the safe bottom and the keyboard, so the nav bar is not counted twice. A focused `text_field` that would still sit under the keyboard is scrolled into that box when it has a scroll parent (portrait). There is no bounce. `adjustResize` can shrink the window; an IME inset of 0 then means the window no longer overlaps the keyboard. A live portrait check on a device is still open. Host tests inject insets with no emulator: `cargo test -p tekto-platform --lib android_insets`.

Still open: we do not draw a candidate window (the OS may, using the caret spot we set), no marked-range highlight, no selection, arrow keys, copy, or paste. This environment does not run a Windows IME, so that path is compile-shared and not exercised on a Windows desktop. `ui.type` is still plain text, not a composition.

When the old text or a simple style literal (`"#rrggbb"`, `fontSize={N}`) appears once in the app project, `ui.patch` / `tekto patch` also writes that change back to source. The JSON includes a `writeback` object (`written`, `skipped`, or `partial`) and the reason if it did not write. Ambiguous literals, named colors such as `Color::BANNER`, and files outside the app project (`TEKTO_APP_ROOT`, or the `--js` entry's app folder) are left as runtime-only changes. `crates/`, `target/`, `node_modules/`, and `docs/` are never written.

## Hot reload TypeScript

On the web, hot reload is the esbuild loop from `node packages/wasm/dev.js` (no QuickJS). `POST /patch` on that server pushes the same `ui.patch` JSON to the open page (`applyUiPatch`). Set `TEKTO_DEV_URL` to this desktop dev server if you also want source writeback.

Desktop reload rebuilds only the JS bundle and swaps it into the running QuickJS runtime. The Rust core stays up. Needs `esbuild` on `PATH` and a CLI built with `--features quickjs` (do not enable that feature on PR CI).

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
back to source. See `crates/tekto/tests/record_replay.rs` and its JSON fixture. Linux CI runs that test and the Notes PNG compare as part of `cargo test --workspace --all-targets`.

### Opt-in store time-travel

`tekto::Store::new` keeps one value and does not grow. `Store::recording(initial, limit)`
keeps that many frames. `set` records the new value and drops the redo branch.
`undo`, `redo`, and `jump(index)` move the cursor. The scene tree is unchanged
until the app paints from `get()`.

Attach the same `Arc<Mutex<Store<serde_json::Value>>>` to `SharedTreeHandle.history`
if agents should see it. `GET /states` and `app.states` list `{ enabled, cursor, states: [{ index, label, current }] }`.
`POST /states/restore` with `{ "index": N }` (MCP `app.restore`, CLI `tekto restore N`)
jumps there and returns the JSON value. With no store attached, `enabled` is false
and restore returns an error. `examples/time-travel` records a counter, steps back,
and jumps forward. It does not open a window.
