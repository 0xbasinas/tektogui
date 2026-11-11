---
name: add-mcp-tool
description: Add or change a Tekto dev-server HTTP endpoint and the matching MCP tool (and CLI if useful). Use when agents need a new inspect or live-edit action, or when fixing ui.tree, ui.patch, ui.click, ui.key, ui.scroll, ui.type, ui.screenshot, app.state, app.logs, or app.components.
---

# Add an MCP tool

The stdio MCP server (`tekto mcp`) is a proxy for the `ui.*` and other `app.*` tools (`app.components` is answered locally). Real work happens on the localhost dev HTTP server started by `tekto dev` or a window with `dev_server: true`.

## Files to touch

- `crates/tekto-mcp/src/lib.rs`
  - HTTP `match` in the request handler (`GET`/`POST` paths)
  - `tool_list()` — name, description, JSON Schema
  - `call_tool()` — map `ui.*` / `app.*` to the HTTP call
  - `AppHandle` trait if the tool needs new behavior on the tree
- `crates/tekto-cli/src/main.rs` — add a subcommand only if humans will want the same action (`tree`, `click`, `screenshot`, `patch` already exist; there is **no** `type` subcommand)
- `docs/agent-guide.md` and this skill, if the tool list or flags change

## Current tools and routes

| MCP tool | HTTP | CLI |
|---|---|---|
| `ui.tree` | `GET /tree` (JSON) and `GET /tree.txt` | `tekto tree` uses `/tree.txt` |
| `ui.accesskit` | `GET /accesskit` | none |
| `ui.screenshot` | `GET /screenshot` | `tekto screenshot [out]` |
| `ui.click` | `POST /click` `{"id":N}` | `tekto click N` (sets focus and `last_click`; also logged). Retained desktop JS button callbacks run; callback errors are returned and logged. Each callback has a fixed one-second QuickJS budget (no setting); a runaway loop is interrupted and returns a budget error. Native Rust and lock waits are not preemptible, and changes made before an interrupt are kept |
| `ui.key` | `POST /key` `{"name":"Tab"}` | `tekto key <name>`; names: `Tab`, `Shift+Tab`, `Enter`, `Space`, `Backspace` |
| `ui.scroll` | `POST /scroll` `{"id":N,"delta":N}` | `tekto scroll ID --by N`; returns the clamped `offset` |
| `ui.type` | `POST /type` `{"id":N,"text":"…"}` inserts plain characters into a text field at the caret and focuses it. Not an IME; use `ui.key` for Backspace | none |
| `ui.patch` | `POST /patch` | `tekto patch ID [--text] [--fill] [--color] [--font-size] [--padding] [--gap]` (positional id; HTTP/MCP also accept `name` and `fontSize`). Response is `{ ok, node, writeback }`. |
| `app.state` | `GET /state` | none |
| `app.states` | `GET /states` | `tekto states` |
| `app.restore` | `POST /states/restore` `{"index":N}` | `tekto restore N` |
| `app.logs` | `GET /logs` | none |
| `app.components` | `GET /components` | none. Returns `packages/jsx-runtime/src/components.json`. The stdio tool works with no running app (it does not call the dev server); `GET /components` needs a running dev server. |

Protocol version in `initialize` is `2024-11-05`. Stdio uses newline-delimited JSON-RPC, with legacy `Content-Length` frames accepted and answered in kind. Stdout is protocol-only. The advertised initialization version remains `2024-11-05`. Tool results use `{ "content": [{ "type": "text", "text": "…" }] }` (screenshots use `type: "image"` + base64 + `mimeType: "image/png"`).

## HTTP limits (dev server)

Constants live in `crates/tekto-mcp/src/lib.rs` (`serve`, `handle_client_locked`):

- `MAX_CLIENTS = 8`: at most eight connections in flight. The accept loop closes any extra connection at once, with no response and no dispatch. Capacity returns when a worker finishes.
- `IO_DEADLINE = 5s`, absolute, not per read or write. The request (headers and body) must arrive within 5 seconds of accept, even if a client trickles one byte at a time. The response gets a separate 5 seconds to write. On a miss the connection is dropped with no HTTP error body, so a slow reader cannot hold a slot.
- Requests: headers up to 64 KiB, whole buffered request up to 2 MiB. A body shorter than `Content-Length` is an error, not a partial request.
- Most handlers run one at a time under a dispatch lock; click and key delivery run outside that lock and use the handle action gate, which also orders supported patches and JS mounts. Keep new routes short. Do not reenter gated handle actions from callbacks or change hooks. A click that runs JS holds the gate for at most about one second of JS time (periodic interrupt); native or lock waits in a new route are not covered, so keep them short.
- IPv6: `DevEndpoint::base_url()` brackets the host (`http://[::1]:PORT`).

Clients should retry after a closed or timed-out connection. Keep `docs/agent-guide.md` in sync if these numbers change.

## Steps

1. Add the HTTP route first and try it against `tekto dev --headless`.
2. Add the MCP tool with a tight schema (`required` only for what is required).
3. Add a CLI subcommand only if it stays consistent with existing flags (positional id for click/patch).
4. Live edits should keep working without a reload. Unique text / color / fontSize literals in the app project are written back to source; if the match is unsafe, leave the runtime change and report why in the `writeback` field. Never write outside the app project.

## Verify

```bash
cargo test -p tekto-mcp --all-targets
# HTTP socket regressions (limits, deadlines, overload, IPv6):
cargo test -p tekto-mcp --lib -- request_limits stalled_clients trickling_headers slow_reader endpoint_urls http_socket
cargo test -p tekto-mcp --test http_trust
cargo test -p tekto-cli --test live_cli ipv6_dev_server_cli_flow
cargo clippy -p tekto-mcp -p tekto-cli --all-targets -- -D warnings
# Framing smoke check (newline framing; expect one JSON line with "result":{}):
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"ping"}' | cargo run -q -p tekto-cli -- mcp
# Manual: start `tekto dev --headless`, then call the CLI or curl the new route.
```
