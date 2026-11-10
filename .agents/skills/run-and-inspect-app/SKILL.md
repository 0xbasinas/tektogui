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

The process prints a URL like `Tekto dev server at http://127.0.0.1:PORT`. The endpoint file is `$TEKTO_DEV_FILE`, or `$XDG_RUNTIME_DIR/tekto-dev.json`, or `$TMPDIR/tekto-dev.json` (often `/tmp/tekto-dev.json`).

## CLI (needs a running dev server, except screenshot)

There is **no** `tekto type` subcommand. `tekto patch` takes a positional id, not `--id`. Colors are `#rrggbb` or `#rrggbbaa`.

```bash
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- click 1
cargo run -p tekto-cli -- screenshot /tmp/shot.png
cargo run -p tekto-cli -- patch 4 --text "Hello from a patch"
cargo run -p tekto-cli -- patch 4 --color "#2c333a" --fill "#f4f3f0"
# If no server is running, `screenshot` falls back to a headless Notes PNG.
```

`tree` should include `window#1` and `Hello from Tekto` (unless you patched the text).

## HTTP and MCP

- `GET /tree`, `GET /tree.txt`, `GET /screenshot`, `GET /state`, `GET /logs`
- `POST /patch` JSON: `{"id":4,"text":"…","name":"…","fill":"#rrggbb","color":"#rrggbb"}`
- `POST /click` `{"id":1}`
- `POST /type` `{"id":4,"text":"typed"}`

Stdio MCP (`cargo run -p tekto-cli -- mcp`) proxies those to the dev server. Tools: `ui.tree`, `ui.screenshot`, `ui.click`, `ui.type`, `ui.patch`, `app.state`, `app.logs`.

Live patches change the running tree only. Writing the change back to source is not implemented yet (issue on Month 2). Hot reload of TS is also not implemented yet; use patch plus a restart for Rust edits.

## Verify

After `dev --headless`, `tekto tree` prints a tree and `tekto screenshot` writes a PNG starting with the bytes `89 50 4E 47`. Stop the dev process when you are done so the endpoint file is cleared (`dev --headless` clears it after Enter).
