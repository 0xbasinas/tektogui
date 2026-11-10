# Windows runtime handoff

The Windows window, clipboard, and sandboxed file code is in the tree but has not been runtime-proven. Linux cross-checks only compile Windows code; they do not demonstrate that a Windows window or native API works. Issues #3, #26 and #28 remain open pending real Windows runtime checks and review of their issue-specific acceptance criteria and comments. The issue discussions were unavailable to the implementing agent because GitHub could not be reached; compare this checklist with those discussions before closing anything. Send local fixes as focused PRs into `nightly`, never `main`.

## Prerequisites

Use Windows with Rust and `rustup`, with host triple `x86_64-pc-windows-msvc` (verify with `rustc -vV`), Visual Studio 2022 Build Tools, the “Desktop development with C++” workload, and a Windows SDK. Use Developer PowerShell for VS 2022 if `link.exe` is missing. Do not use the GNU target for this runtime pass. `cargo check` does not link; `cargo test` or `cargo run` links `user32`, `kernel32`, and `ntdll`. From PowerShell at the repository root:

```powershell
rustup target add x86_64-pc-windows-msvc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm test
```

Node/npm are needed for the JS tests. For optional TypeScript hot reload, install the MIT esbuild executable on PATH (or set `ESBUILD`), then use the QuickJS feature shown below.

## Window and dev server (#3)

In terminal 1:

```powershell
cargo run -p tekto-cli -- dev
```

Confirm the Notes window paints, resize it, move it between displays with different scaling if available, click controls, type, and close it. Record DPI, display configuration, resize artifacts and shutdown behavior. The process prints the dev-server URL. While it remains open, in terminal 2:

```powershell
cargo run -p tekto-cli -- tree
cargo run -p tekto-cli -- screenshot "$env:TEMP\tekto-window.png"
cargo run -p tekto-cli -- patch 4 --text "Windows patch"
cargo run -p tekto-cli -- tree
```

Use an actual text node id from `tree` if it differs. Check the window and second tree reflect the patch and inspect the PNG. Inspect the patch JSON `writeback` field: source writeback needs this CLI dev command (or `TEKTO_APP_ROOT` set to `examples\notes` when running `cargo run -p notes`) and a uniquely matching source literal. Inspect and revert any unwanted source changes. The endpoint file defaults to `%TEMP%\tekto-dev.json` unless `TEKTO_DEV_FILE` or `XDG_RUNTIME_DIR` is set. If screenshot prints `no dev server, used headless`, it did not capture the live window. Stop the window normally.

## Headless and optional TypeScript (#3)

In terminal 1, leave this running until testing finishes, then press Enter:

```powershell
cargo run -p tekto-cli -- dev --headless --out "$env:TEMP\notes-headless.png"
```

Run the same tree, screenshot, and patch commands in terminal 2. Check the generated PNGs open and the patched tree changes. Also run:

```powershell
$env:TEKTO_SHOT_DIR = "$env:TEMP\tekto-shots"
cargo test -p tekto --test headless_snapshot
```

Optional TypeScript window, with esbuild on PATH:

```powershell
cargo run -p tekto-cli --features quickjs -- dev --js examples/notes/src/app.tsx
```

Edit and restore the TSX file to check hot reload; verify the tree and window. This optional path requires QuickJS tooling and is not covered by default builds.

## Native clipboard and files (#26 / #28)

The opt-in probe overwrites the clipboard and leaves two files under `%LOCALAPPDATA%\tekto` (or `%USERPROFILE%\AppData\Local\tekto` when LOCALAPPDATA is not absolute):

```powershell
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
cargo run -p tekto-native --example windows_probe
```

While it pauses, paste into Notepad and compare `Tekto λ 漢字 😀`. After Enter, confirm its text and byte assertions pass; inspect the saved files independently:

```powershell
Get-Content -Raw -Encoding utf8 "$env:LOCALAPPDATA\tekto\windows-probe.txt"
Format-Hex "$env:LOCALAPPDATA\tekto\windows-probe.bin"
```

Expected bytes are `00 01 7F 80 FF`. If using the USERPROFILE fallback, substitute its path. Test copying Unicode from another application into Tekto as well and clipboard contention. After the probe exits, paste again and confirm the text remains available; the probe itself does not assert this. The probe only checks its own round trip and does not restore previous clipboard contents.

The clipboard makes one `OpenClipboard` attempt; a busy clipboard returns `Unavailable` without a system error code. It does not convert line endings to CRLF and rejects interior NUL. It supplies by-value `CF_UNICODETEXT` and destroys its message-only owner before `set_text` returns.

For files, additionally check missing parents and files, overwriting shorter content, invalid UTF-8, traversal and absolute paths, and rejection of junctions and symlinks inside the root, including a junction as a parent, and a reserved DOS name such as `CON`. Cloud placeholders are reparse points and are refused. New files inherit the app-data ACL. Do not use valuable data: hard links can share data with files outside the root. Ancestors of the app-data root may themselves be junctions and are followed. Confirm this behavior with a disposable test directory. Exercise non-ASCII paths and app-data ancestry. These runtime and security edge cases are not established by a Linux compile check.

## Report template

- Date, Windows version, architecture, Rust version, SDK/Build Tools version:
- Branch and commit, display scale(s), session type:
- #3 window, DPI, resize, shutdown, dev server, headless, tree/screenshot/patch; commands and observed results:
- #26 clipboard Unicode, other-app paste/copy, ownership, contention; commands and observed results:
- #28 text/bytes, invalid paths, junctions/symlinks, hard links and app-data ancestry; commands and observed results:
- Tests run, exact failures, screenshots/logs with secrets removed:
- Remaining risks and links to follow-up PRs into `nightly`:
