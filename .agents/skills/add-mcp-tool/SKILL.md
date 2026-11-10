---
name: add-mcp-tool
description: Add or change a Tekto dev-server HTTP endpoint and the matching MCP tool (and CLI if useful). Use when agents need a new inspect or live-edit action, or when fixing ui.tree, ui.patch, ui.click, ui.type, ui.screenshot, app.state, or app.logs.
---

# Add an MCP tool

The stdio MCP server (`tekto mcp`) is a proxy. Real work happens on the localhost dev HTTP server started by `tekto dev` or a window with `dev_server: true`.

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
| `ui.screenshot` | `GET /screenshot` | `tekto screenshot [out]` |
| `ui.click` | `POST /click` `{"id":N}` | `tekto click N` |
| `ui.type` | `POST /type` `{"id":N,"text":"…"}` | none |
| `ui.patch` | `POST /patch` | `tekto patch ID [--text] [--fill] [--color]` (positional id; no `--name` on the CLI, but HTTP/MCP accept `name`) |
| `app.state` | `GET /state` | none |
| `app.logs` | `GET /logs` | none |

Protocol version in `initialize` is `2024-11-05`. Tool results use `{ "content": [{ "type": "text", "text": "…" }] }` (screenshots use `type: "image"` + base64 + `mimeType: "image/png"`).

## Steps

1. Add the HTTP route first and try it against `tekto dev --headless`.
2. Add the MCP tool with a tight schema (`required` only for what is required).
3. Add a CLI subcommand only if it stays consistent with existing flags (positional id for click/patch).
4. Live edits should keep working without a reload. Source write-back is a separate, later step.

## Verify

```bash
cargo test -p tekto-mcp --all-targets
cargo clippy -p tekto-mcp -p tekto-cli --all-targets -- -D warnings
# Manual: start `tekto dev --headless`, then call the CLI or curl the new route.
```
