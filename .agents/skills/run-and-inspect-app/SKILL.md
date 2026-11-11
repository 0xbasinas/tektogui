---
name: run-and-inspect-app
description: Run a Tekto app in dev mode and inspect it with the CLI, the browser inspector, or MCP (tree, click, type, screenshot, live patch). Use when checking a running UI, debugging a widget, or trying a live edit. There is no `tekto type` CLI; typing is MCP or POST /type only.
---

# Run and inspect an app

`tekto dev` currently always runs the Notes example (`examples/notes`). A window starts a dev server automatically (`WindowOptions { dev_server: true }`).

On this shared box, do **not** use `DISPLAY=:1` or `DISPLAY=:2`. `Xvfb :99` is fine. Leave `~/.beddcode` and Tailscale alone.

## Start the app

```bash
cargo build -p tekto-cli -p notes
# Window (Linux):
cargo run -p notes
# or: cargo run -p tekto-cli -- dev

# Headless PNG plus a dev server that stays up until you press Enter:
cargo run -p tekto-cli -- dev --headless --out /tmp/notes-headless.png
```

To also publish the AccessKit tree to the OS (screen readers), run with `--features a11y-os` (off by default so builds stay fast), for example `cargo run -p notes --features tekto/a11y-os`.

The process prints a URL like `Tekto dev server at http://127.0.0.1:PORT`. The endpoint file is `$TEKTO_DEV_FILE`, or `$XDG_RUNTIME_DIR/tekto-dev.json`, or `$TMPDIR/tekto-dev.json` (often `/tmp/tekto-dev.json`).

## CLI (needs a running dev server, except screenshot)

`tekto click <id>` records a click (focus + `last_click`, and a `clicked #id` log). A real left-click in the window does the same, using logical pixels (`physical / scale_factor`) and the tree hit test. With a retained desktop QuickJS session, a button `onClick` also runs. A completed pointer click and Enter or Space on a focused button use the same delivery. There is **no** `tekto type` subcommand. `tekto patch` takes a positional id, not `--id`. Colors are `#rrggbb` or `#rrggbbaa`.

```bash
cargo run -p tekto-cli -- inspect   # prints http://127.0.0.1:PORT/inspector
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- click 1
cargo run -p tekto-cli -- key Tab
cargo run -p tekto-cli -- scroll 1 --by 48
cargo run -p tekto-cli -- screenshot /tmp/shot.png
cargo run -p tekto-cli -- patch 4 --text "Hello from a patch"
cargo run -p tekto-cli -- patch 4 --color "#2c333a" --fill "#f4f3f0" --font-size 20 --padding 24
# If no server is running, `screenshot` falls back to a headless Notes PNG.
```

## Browser inspector

`tekto inspect` reads the same endpoint file as `tekto tree` and prints `GET /inspector`. Open that URL in a browser. The page is served by the dev server (`crates/tekto-mcp/src/inspector.html`). It is not an MCP tool.

The page loads `GET /tree` and shows the live tree. Click a node. Check only the properties you want to change (text, name, fill, color, font size, padding, gap). Text and name may be empty. Then press **Apply patch**. That sends `POST /patch` with `{ "id": <selected>, ...fields }` and refreshes the tree. The status box shows the JSON result (`node` and `writeback`) or the error text.

Press **Refresh tree** after a change made elsewhere (CLI, MCP, or a hot reload). If the selected id is gone, the page clears the selection.

Same patch from the shell, once you know the id from the tree:

```bash
curl -s -X POST http://127.0.0.1:PORT/patch \
  -H 'Content-Type: application/json' \
  -d '{"id":4,"name":"Inspector test"}'
```

`tree` should include `window#1` and `Hello from Tekto` (unless you patched the text). A button built with `tekto::button` shows up as `button#N` with the label as its name. `tekto click N` (or a left-click on its bounds) sets `last_click` to that id. `GET /accesskit` reports role `Button` and the label. JSX `<button>` only creates the kind and text; fill, color, font size, and padding stay unset until you patch them or set the props. A `tekto::text_field` shows up as `text_field#N`. Its value is node `text` (patchable); its label is `name`. AccessKit role is `TextInput`. Focused desktop keys (printable characters and Backspace) edit the value. `POST /type` does the same for plain characters. Full IME is not implemented.

## HTTP and MCP

- `GET /inspector` — HTML page (see Browser inspector). `GET /health` returns `{"ok":true}`.
- `GET /tree`, `GET /tree.txt`, `GET /accesskit`, `GET /screenshot`, `GET /state`, `GET /states`, `GET /logs`, `GET /components`
- `POST /states/restore` `{"index":N}` jumps an opted-in store to a recorded frame from `GET /states`
- `POST /patch` JSON: `{"id":4,"text":"…","name":"…","fill":"#rrggbb","color":"#rrggbb","fontSize":20,"padding":24,"gap":20}` — response is `{ "ok": true, "node": { ... }, "writeback": { ... } }` or an error
- `POST /click` `{"id":1}`
- `POST /key` `{"name":"Tab"}` presses `Tab`, `Shift+Tab`, `Enter`, `Space`, or `Backspace` on the focused node.
- `POST /scroll` `{"id":N,"delta":N}` scrolls a scroll container by logical pixels and returns the clamped `offset`.
- `POST /type` `{"id":N,"text":"typed"}` inserts plain characters into a `text_field` (errors if the node is not one). Use `POST /key` for Backspace. No IME.
- `POST /reload` raw JS bundle, or `{"js":"…"}`

Stdio MCP (`cargo run -p tekto-cli -- mcp`) proxies these tools to the running dev server: `ui.tree`, `ui.accesskit`, `ui.screenshot`, `ui.click`, `ui.key`, `ui.scroll`, `ui.type`, `ui.patch`, `app.state`, `app.states`, `app.restore`, `app.logs`. `app.components` is the exception: the MCP server answers it locally from the bundled `components.json` and needs no running app. `GET /components` is the dev-server route for the same file, so it needs a running dev server.

Live patches change the running tree immediately (no reload). If the old text or a simple style literal is unique in the app project, the same edit is written back to source; the response includes a `writeback` field (`status`, `files`, `skipped`). If the match is ambiguous, or the source uses a named constant, the runtime change stays and `writeback` explains why. Never writes outside the app project (`TEKTO_APP_ROOT`, or the folder of `tekto dev --js`). Hot reload of TS is not a substitute for `ui.patch`.

## Hot reload TypeScript (no Rust rebuild)

Needs the MIT `esbuild` binary on `PATH` (`npm install -g esbuild`, or set `ESBUILD`) and a CLI built with `--features quickjs`. Do not enable `quickjs` on PR CI.

```bash
# Headless (PNG + dev server, stays up until Enter). Saving the .tsx reloads JS:
cargo run -p tekto-cli --features quickjs -- dev --js examples/notes/src/app.tsx --headless --out /tmp/notes-js.png

# Desktop window (use a safe display on the shared box, e.g. Xvfb :99):
cargo run -p tekto-cli --features quickjs -- dev --js examples/notes/src/app.tsx
```

### Interactive counter (desktop only)

`examples/counter/app.tsx` is a separate interactive example. Notes stays the default and is unchanged. Run it with:

```bash
cargo run -p tekto-cli --features quickjs -- dev --js examples/counter/app.tsx
```

In another terminal, look up ids in the tree. Do not hard-code them. Find the `button` named `Increment` and the `text` named `Count`, then use the button's real id:

```bash
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- click ID
cargo run -p tekto-cli -- tree
```

The Count text should read 0, then 1 after one click, then 2 after another. Its id and the button id stay stable during the session. A window click on the button, then Enter or Space while it has focus, also activates it. `ui.click` (MCP) and `POST /click` do the same. Callback errors are logged (`app.logs`), and agent clicks also return the error. Reload (saving the file) remounts from source and resets Count to 0.

Limits: desktop QuickJS only, not the web host; each JS call has the fixed one-second budget below; run trusted code only. See `docs/tsx-events.md`.

In TSX, reactive text is `text={() => signal()}` and button callbacks are `onClick={() => ...}`. This is desktop QuickJS only. See `docs/tsx-events.md` for syntax and limits (no raw concurrent tree mutation, no reentry of gated handle actions).

Each host-backed JS call (evaluation, click callback, forget, disposal) has a fixed one-second budget in `crates/tekto-js/src/engine.rs`. There is no setting to change it. QuickJS checks the deadline periodically and interrupts JS, so a runaway loop returns a budget error (`QuickJS execution budget exceeded (1 second)`); click failures are also logged (`GET /logs` / `app.logs`). Native Rust calls and lock waits are not generally preemptible, so this is not a strict wall-clock limit. A failed reload keeps the old session. An interrupted callback keeps the session, but changes it already made are not rolled back. Interrupted cleanup can leave cleanups unfinished, and a retired session drops the rest.

The process prints `js bundle Nms` on the first load and `js reload Nms` after a save (bundle + eval + swap). The Rust window and dev server stay up; the JS side remounts from source, so in-memory signal state resets. A failed rebuild prints a short error to stderr and `GET /logs` / `app.logs`. `--no-watch` loads once.

`POST /reload` queues a JS bundle (raw IIFE, or JSON `{"js":"..."}`) for the app thread. If `--js` was not used, the server returns a clear error (`no JS runtime attached`).

On Android, `tekto dev --js examples/notes/src/app.tsx --android` builds the APK with `-F quickjs`, installs it, `adb forward`s port 8474, and POSTs the bundle. The host CLI does not need `--features quickjs`. Set `TEKTO_ANDROID_TARGET=x86_64-linux-android` for AVD beddcode30 so only one ABI is compiled. See the android-build-and-run skill.

You can also POST a bundle by hand after `adb forward`:

```bash
adb forward tcp:8474 tcp:8474
curl -X POST --data-binary @bundle.js http://127.0.0.1:8474/reload
```

HTTP endpoints also include `POST /reload`.

## Verify

After `dev --headless`, `tekto tree` prints a tree and `tekto screenshot` writes a PNG starting with the bytes `89 50 4E 47`. Stop the dev process when you are done so the endpoint file is cleared (`dev --headless` clears it after Enter).
