---
name: run-and-inspect-app
description: Run a Tekto app in dev mode and inspect it with the CLI or MCP (tree, click, type, screenshot, live patch). Use when checking a running UI, debugging a widget, or trying a live edit. There is no `tekto type` CLI; typing is MCP or POST /type only.
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

`tekto click <id>` records a click (focus + `last_click`, and a `clicked #id` log). A real left-click in the window does the same, using logical pixels (`physical / scale_factor`) and the tree hit test. There is **no** `tekto type` subcommand. `tekto patch` takes a positional id, not `--id`. Colors are `#rrggbb` or `#rrggbbaa`.

```bash
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- click 1
cargo run -p tekto-cli -- screenshot /tmp/shot.png
cargo run -p tekto-cli -- patch 4 --text "Hello from a patch"
cargo run -p tekto-cli -- patch 4 --color "#2c333a" --fill "#f4f3f0" --font-size 20 --padding 24
# If no server is running, `screenshot` falls back to a headless Notes PNG.
```

`tree` should include `window#1` and `Hello from Tekto` (unless you patched the text).

## HTTP and MCP

- `GET /tree`, `GET /tree.txt`, `GET /accesskit`, `GET /screenshot`, `GET /state`, `GET /logs`
- `POST /patch` JSON: `{"id":4,"text":"…","name":"…","fill":"#rrggbb","color":"#rrggbb","fontSize":20,"padding":24,"gap":20}` — response is `{ "ok": true, "node": { ... }, "writeback": { ... } }` or an error
- `POST /click` `{"id":1}`
- `POST /type` `{"id":4,"text":"typed"}`
- `POST /reload` raw JS bundle, or `{"js":"…"}`

Stdio MCP (`cargo run -p tekto-cli -- mcp`) proxies those to the dev server. Tools: `ui.tree`, `ui.accesskit`, `ui.screenshot`, `ui.click`, `ui.type`, `ui.patch`, `app.state`, `app.logs`.

Live patches change the running tree immediately (no reload). If the old text or a simple style literal is unique in the app project, the same edit is written back to source; the response includes a `writeback` field (`status`, `files`, `skipped`). If the match is ambiguous, or the source uses a named constant, the runtime change stays and `writeback` explains why. Never writes outside the app project (`TEKTO_APP_ROOT`, or the folder of `tekto dev --js`). Hot reload of TS is not a substitute for `ui.patch`.

## Hot reload TypeScript (no Rust rebuild)

Needs the MIT `esbuild` binary on `PATH` (`npm install -g esbuild`, or set `ESBUILD`) and a CLI built with `--features quickjs`. Do not enable `quickjs` on PR CI.

```bash
# Headless (PNG + dev server, stays up until Enter). Saving the .tsx reloads JS:
cargo run -p tekto-cli --features quickjs -- dev --js examples/notes/src/app.tsx --headless --out /tmp/notes-js.png

# Desktop window (use a safe display on the shared box, e.g. Xvfb :99):
cargo run -p tekto-cli --features quickjs -- dev --js examples/notes/src/app.tsx
```

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
