# Notebook

A small Rust notebook for short lists and notes. Edit the title and up to eight single-line entries, scroll to reach the lower entries, and press **Save notebook**. The file is JSON, so it can also be edited with a text editor while the app is closed. Saving overwrites the selected file; the app reports save errors in the window. Changes are not saved until you press Save.

```sh
cargo run -p notebook -- --file notebook.json
cargo build -p notebook --release
./target/release/notebook --file notebook.json
```

The default file is `notebook.json` in Tekto’s app data directory (`$XDG_DATA_HOME/tekto`, or `~/.local/share/tekto` on Linux). Paths must be relative to that directory and contain only normal path components. Parent directories are created on save. Invalid or unreadable files, including files with more than eight lines, open in recovery mode with an error and saving disabled. Fix the named file and restart; the original is preserved. Missing JSON fields default to empty values. The status shows unsaved changes after editing. Saves write and sync a temporary file before replacing the document. Fields are single-line; full multiline editing is not available here.

The window starts the Tekto dev server and prints its address. With the CLI built (`cargo build -p tekto-cli`), use `cargo run -p tekto-cli -- tree` to find the field and button IDs, `cargo run -p tekto-cli -- click ID` to save, or `cargo run -p tekto-cli -- screenshot /tmp/notebook.png` to inspect. The MCP server (`cargo run -p tekto-cli -- mcp`) exposes `ui.tree`, `ui.type` and `ui.click`; `ui.type` inserts at the field's caret. The HTTP `/type` endpoint works too. Changes made with these tools are saved by the same button. Runtime patches and their source writeback are intended for development, not document persistence.

To render without a window, use `cargo run -p notebook -- --file notebook.json --headless /tmp/notebook.png`. This renders the initial document and exits; it does not start a dev server. `tekto dev` itself currently starts Notes, not Notebook.
