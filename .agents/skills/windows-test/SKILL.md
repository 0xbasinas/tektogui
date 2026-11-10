---
name: windows-test
description: Run the real Windows Notes window, text clipboard, and sandboxed app-data files, and record what happened. Use on a Windows machine before closing issues #3, #26, or #28. Linux cargo check is not this test.
---

# Windows runtime test

Linux can only typecheck Windows code. `cargo check --target x86_64-pc-windows-gnu` and `--target x86_64-pc-windows-msvc` do not link, do not open a window, and do not touch the clipboard or NTFS. Do not close #3, #26, or #28 from a Linux log. PRs go to `nightly`, never `main`. Do not force-push.

Full command list and report template: [docs/windows-handoff.md](../../../docs/windows-handoff.md). Read it before running anything. The probe overwrites the clipboard and does not restore it.

## Toolchain (MSVC, not MinGW)

- Rust host `x86_64-pc-windows-msvc` (`rustc -vV` must show that host).
- Visual Studio 2022 Build Tools, workload “Desktop development with C++”, including MSVC and a Windows SDK. The clipboard and file code link `user32`, `kernel32`, and `ntdll` (`ntdll.lib` comes from the SDK).
- `cargo check` does not prove those libs resolve. The first real link is `cargo test` or `cargo run`.
- If the linker is not found, use “Developer PowerShell for VS 2022”. Do not use `x86_64-pc-windows-gnu` for this runtime pass.

## What to run

From the repo root in PowerShell, follow `docs/windows-handoff.md`:

- Notes window: resize, move across DPI, click, type, close, dev server. Prefer `cargo run -p tekto-cli -- dev` when source writeback must be exercised. `cargo run -p notes` from the repo root does not set `TEKTO_APP_ROOT`, and writeback will not find `examples/notes`.
- Dev-server file on Windows is `%TEMP%\tekto-dev.json` unless `TEKTO_DEV_FILE` or `XDG_RUNTIME_DIR` is set. If `tekto screenshot` prints `no dev server, used headless`, it did not capture the live window.
- Headless dev server, tree, screenshot, patch, `TEKTO_SHOT_DIR` snapshot.
- Optional QuickJS path only with `--features quickjs` and esbuild (`ESBUILD` or `PATH`).
- `cargo run -p tekto-native --example windows_probe` on Windows only. On any other OS the example must fail.
- Before printing Unicode, set the console to UTF-8. `Get-Content` of `windows-probe.txt` and a Notepad paste are not evidence if the console code page cannot show `λ`, `漢字`, or the emoji.
- File edges the probe does not cover: missing parents, shorter overwrite, invalid UTF-8, absolute and traversal paths, junctions and symlinks inside the root (including a junction parent), non-ASCII names. Cloud placeholders are reparse points and are refused. Hard links are not refused. Ancestors of the app-data root are opened by Win32 path and can be junctions.

## Limits to state in the report

- One `OpenClipboard` attempt. A busy clipboard returns `NativeError::Unavailable`. There is no retry.
- Clipboard text is stored as given. Lone `\n` is not rewritten to CRLF.
- `set_text` puts by-value `CF_UNICODETEXT` on the clipboard, then closes the clipboard and destroys the owner window before it returns. Confirm the text is still readable after the probe exits.
- The probe leaves `windows-probe.txt` and `windows-probe.bin` under `%LOCALAPPDATA%\tekto` (or `%USERPROFILE%\AppData\Local\tekto`).
- Dialogs, notifications, tray, and shortcuts stay unsupported.
- `cargo test` on Windows runs `windows::tests::file_roundtrip_in_isolated_app_data`. Linux CI does not compile that module.

## Verify

On Linux, compile only (no runtime claim):

```bash
cargo check --target x86_64-pc-windows-gnu -p tekto-native --all-targets
cargo check --target x86_64-pc-windows-msvc -p tekto-native --all-targets
```

On Windows, `cargo test --workspace` and the handoff steps. Record OS, SDK, `rustc -vV`, commands, and failures. Keep the three issues open until that record exists.
