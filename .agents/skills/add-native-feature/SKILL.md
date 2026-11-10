---
name: add-native-feature
description: Implement a tekto-native plugin (files, open/save dialogs, clipboard, notifications, system tray, or global shortcuts) for one platform at a time, with the same Rust and TS API surface. Use when filling in or extending crates/tekto-native.
---

# Add a native feature

`crates/tekto-native/src/lib.rs` holds small traits: `Files`, `Clipboard`, `Notifications`, `SystemTray`, `GlobalShortcuts`. `StubNative` is the current implementation. Desktop Linux/Windows already do plain `std::fs` read/write. Linux text clipboard (`get_text` / `set_text`) uses `arboard` (MIT OR Apache-2.0, `default-features = false`, `wayland-data-control`) in `src/linux.rs`. Other clipboard platforms, dialogs, notifications, tray, and shortcuts return `NativeError::Unsupported { capability, platform }`.

A present platform implementation that cannot reach the OS service (headless Linux, no `DISPLAY` and no `WAYLAND_DISPLAY`) returns `NativeError::Unavailable { capability, platform, reason }`, not `Unsupported`. Do not open displays `:1` or `:2`. The opt-in live check is `TEKTO_CLIPBOARD_LIVE=1`.

JS calls clipboard through `NativeOp` (`clipboardGetText`, `clipboardSetText`) and `apply_native_op`. `tekto-js::apply_native_host_op` wraps that in `JsError`. It is not a scene-tree `HostOp` (get returns text, not a node id). There is still no QuickJS binding for it; do not enable the `quickjs` feature by default.

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
7. Web (`wasm32`): no `std::fs`. Browser downloads/pickers can wait; a clear `Unsupported` is fine until the web milestone.

## Verify

```bash
cargo test -p tekto-native
cargo clippy --workspace --all-targets -- -D warnings
# If you touched Windows or Android paths (compile only is enough):
cargo check --target x86_64-pc-windows-msvc --workspace
# Android (do not launch the beddcode30 emulator):
#   source ~/.android-env.sh   # if that file exists
cargo check -p tekto --target aarch64-linux-android --no-default-features --features cpu
# or: cargo ndk -t arm64-v8a check -p tekto --no-default-features --features cpu
```

On a real machine for that OS, call `tekto::native()` and the trait method, and confirm success or a clear error.
