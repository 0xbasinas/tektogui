---
name: add-native-feature
description: Implement a tekto-native plugin (files, open/save dialogs, clipboard, notifications, system tray, or global shortcuts) for one platform at a time, with the same Rust and TS API surface. Use when filling in or extending crates/tekto-native.
---

# Add a native feature

`crates/tekto-native/src/lib.rs` holds small traits: `Files`, `Clipboard`, `Notifications`, `SystemTray`, `GlobalShortcuts`. `StubNative` is the current implementation. Linux text clipboard (`get_text` / `set_text`) uses `arboard` (MIT OR Apache-2.0, `default-features = false`, `wayland-data-control`) in `src/linux.rs`. Linux file reads and writes are sandboxed in the same file (`libc` 0.2, MIT OR Apache-2.0, Linux-only). Windows text clipboard uses the same `arboard` crate without the Wayland feature (`src/windows.rs`). Windows file reads and writes use the same trait methods, confined with `NtCreateFile` (no extra crate). Other platforms, dialogs, notifications, tray, and shortcuts return `NativeError::Unsupported { capability, platform }`.

Windows code is compile-checked on Linux with `cargo check --target x86_64-pc-windows-gnu -p tekto-native` (no linker, no runtime). Clipboard and file behavior still need a Windows machine. Do not treat that check as a runtime test.

On Windows, `arboard` links `clipboard-win` 5.4.3 and `error-code` 3.4.0, both Boost Software License 1.0. That is outside the MIT / Apache-2.0 / BSD / zlib set used for crates this repo adds directly. `windows-sys` and `windows-link` (pulled by `clipboard-win`) are MIT OR Apache-2.0. The file sandbox does not add a crate.

A present platform implementation that cannot reach the OS service (headless Linux, no `DISPLAY` and no `WAYLAND_DISPLAY`) returns `NativeError::Unavailable { capability, platform, reason }`, not `Unsupported`. Do not open displays `:1` or `:2`. The opt-in live check is `TEKTO_CLIPBOARD_LIVE=1`.

JS calls clipboard and files through `NativeOp` and `apply_native_op`. `tekto-js::apply_native_host_op` wraps that in `JsError`. It is not a scene-tree `HostOp` (get returns text, not a node id). There is still no QuickJS binding for it; do not enable the `quickjs` feature by default.

`NativeOp` is internally tagged (`#[serde(tag = "op", rename_all = "camelCase")]`). Clipboard ops:

- `{"op":"clipboardGetText"}`
- `{"op":"clipboardSetText","text":"..."}`

File ops (Linux and Windows; other platforms return `Unsupported` for `files.read` / `files.write`):

- `{"op":"filesReadText","path":"notes/a.txt"}` — `apply_native_op` returns `Some(text)`
- `{"op":"filesWriteText","path":"notes/a.txt","text":"..."}` — returns `None`
- `{"op":"filesReadBytes","path":"blob"}` — returns `Some` of a JSON byte array encoded as a string, for example `"[0,255]"`. The op result type is still `Option<String>`.
- `{"op":"filesWriteBytes","path":"blob","bytes":[0,255]}` — `bytes` is a JSON array of numbers 0–255 (`Vec<u8>`). Returns `None`.

## Linux file sandbox

`Files::read_to_string`, `write_string`, `read_bytes`, and `write_bytes` on Linux go through `linux::read_file` / `write_file`. The root is `$XDG_DATA_HOME/tekto` when `XDG_DATA_HOME` is absolute, otherwise `$HOME/.local/share/tekto`. The root is created if needed. Paths must be relative and made only of normal components: no absolute paths, empty segments, `.`, `..`, or a NUL. Each component is opened with `openat` and `O_NOFOLLOW` (and `O_DIRECTORY` for parents), so a symlink or a rename out of the tree fails. Parent directories must already exist; the plugin does not create them. New files are mode `0o600`. `read_to_string` is UTF-8 over `read_bytes`. Dialogs and paths outside this directory stay unsupported. Headless confinement tests set `XDG_DATA_HOME` in a child process (`TEKTO_FILES_TEST_CHILD=1`) so they do not write the developer’s real app-data directory.

## Windows file sandbox

Same shape as Linux: `read_to_string`, `write_string`, `read_bytes`, and `write_bytes` under an app-data root. The root is `%LOCALAPPDATA%\tekto` when `LOCALAPPDATA` is absolute, otherwise `%USERPROFILE%\AppData\Local\tekto`. The root is created if needed. Paths must be relative and made only of normal components (`/` or `\`). Rejected: absolute paths, drive prefixes, UNC, empty segments, `.`, `..`, NUL, `:` (alternate data streams), `* ? " < > |`, and a component that ends in a space or a dot.

Each component is opened with `NtCreateFile` relative to the previous directory handle, with `FILE_OPEN_REPARSE_POINT`. If the opened object has `FILE_ATTRIBUTE_REPARSE_POINT` (junction, symlink, mount point, or cloud placeholder), the call fails and does not follow it. Parent directories must already exist. New files are created only when the name is absent; an existing reparse point is not overwritten. `read_to_string` is UTF-8 over `read_bytes`.

Residual risk: ancestors of the app-data root are opened by Win32 path (`create_dir_all` and the root `NtCreateFile` on `\??\...`), so a junction above the root is followed the same way any app opens `%LOCALAPPDATA%`. Inside the root, a junction cannot retarget an open that starts from the root handle. Hard links inside the tree still name the same file data (same as Linux `openat`). There is no Windows runtime test in CI.

Fill **one platform at a time**. Keep the trait API the same for Rust and for TypeScript.

## Files to touch

- `crates/tekto-native/src/lib.rs` (and a new `src/<platform>.rs` if the impl grows)
- `crates/tekto/src/lib.rs` — re-exports `native`, `Files`, `Clipboard`, and so on
- `crates/tekto-js/src/lib.rs` — re-export `NativeOp` / `apply_native_host_op` for plugin calls. Add a scene-tree `HostOp` only for tree mutations. Do not enable the `quickjs` feature by default
- A unit test in `tekto-native` (success on the target platform, or a clear `Unsupported` error elsewhere)
- This skill, if the trait names or error shape change

## Steps

1. Use the existing trait methods. Add a method only if the current ones cannot express the feature.
2. Implement it behind `cfg` (`target_os = "linux"`, `"windows"`, `"android"`, `"macos"`, `"ios"`, or `target_arch = "wasm32"` for web). Leave other platforms on the stub.
3. Return `NativeError::Unsupported { capability, platform }` (via `unsupported("clipboard.get")`) when that platform has no implementation yet. Return `NativeError::Unavailable { capability, platform, reason }` when the implementation exists but the session cannot use it (headless CI clipboard). `Option` is for cancel (open/save dialogs already return `Result<Option<String>, NativeError>`).
4. Do not pull a heavy crate into the default graph. If a dialog/clipboard crate is large, feature-gate it and keep default builds compiling without it.
5. License must be MIT, Apache-2.0, BSD, or zlib. Mention it in the PR.
6. Tray and global shortcuts are desktop-only; do not fake them on Android, iOS, or web.
7. Web (`wasm32`): no `std::fs`. Browser downloads/pickers can wait; a clear `Unsupported` is fine until the web milestone. Do not widen the Linux file root or follow symlinks to reach the rest of the filesystem.

## Verify

```bash
cargo test -p tekto-native
cargo clippy --workspace --all-targets -- -D warnings
# If you touched Windows or Android paths (compile only is enough; no linker, no Windows runtime):
cargo check --target x86_64-pc-windows-gnu -p tekto-native
# Android (do not launch the beddcode30 emulator):
#   source ~/.android-env.sh   # if that file exists
cargo check -p tekto --target aarch64-linux-android --no-default-features --features cpu
# or: cargo ndk -t arm64-v8a check -p tekto --no-default-features --features cpu
```

On a real machine for that OS, call `tekto::native()` and the trait method, and confirm success or a clear error.
