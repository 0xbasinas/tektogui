# Architecture

Tekto is a stack of small crates around one scene tree.

```
TypeScript + JSX (signals)     Rust app API (crate `tekto`)
        |                                |
        +----------- Host ops -----------+
                         |
                    tekto-tree
                         |
         +---------------+---------------+
         |               |               |
   tekto-layout    tekto-text      tekto-a11y
      (Taffy)       (Parley)      (AccessKit)
         |               |               |
         +-------+-------+               |
                 |                       |
           tekto-render                  |
        (trait + CPU now,                |
         GPU / web later)                |
                 |                       |
           tekto-platform                |
     desktop window | Android | web      |
                 |                       |
              dev server <---------------+
           (tekto-mcp)   agent MCP, CLI,
                         live patches
```

## Scene tree

`tekto-tree` owns nodes and **stable, monotonic ids**. Ids are never reused. That is what AccessKit, MCP tools, and live patches all use.

A `NodePatch` can change text, name, fill, or color at runtime. That is the live-edit path.

## Layout and text

`tekto-layout` maps nodes to Taffy (flex column or row, padding, gap, sizes).

`tekto-text` uses Parley and a bundled **Roboto Regular** (Apache-2.0) so headless screenshots look the same everywhere.

## Renderer

`tekto-render` is a trait:

- `CpuRenderer` (default): tiny-skia, glyph outlines via Skrifa. Used for windows (through softbuffer), headless PNGs, tests, and later a canvas2d-style web fallback.
- `GpuRenderer`: stub. wgpu / Vello (and WebGPU on the web) will live behind the `gpu` feature so default builds stay fast.

Apps never call tiny-skia or wgpu directly.

## Platform

`tekto-platform` hides the host:

- **Desktop (Windows, Linux, macOS):** winit window, softbuffer presents CPU frames.
- **Android:** `examples/notes-android` is a `NativeActivity` APK (`cargo-apk2`, `winit` + `android-activity`). `android_main` calls `tekto::run_android`. The window / softbuffer surface is created on `resumed` and dropped on `suspended`. The CPU core also still builds headless for host tests and for `cargo check --target aarch64-linux-android`.
- **Web:** planned. The Rust core builds to WASM. Painting uses WebGPU / WebGL2 (GPU backend) or the CPU renderer to a `<canvas>`. TypeScript runs in the **browser JS engine**, not QuickJS. The public API stays the same; no DOM in app code.

## Agents

`tekto-a11y` builds an AccessKit tree from the scene (same stable ids, plus roles, labels, bounds, and focus). `GET /accesskit` and MCP `ui.accesskit` return that dump. The desktop window rebuilds it after every layout so a live patch can stay in sync. Publishing to the OS (AT-SPI / UIA / VoiceOver) is behind the optional `a11y-os` feature, which pulls in `accesskit_winit` and is off by default so compile times stay short.


`tekto-mcp` starts a localhost HTTP server and writes `tekto-dev.json` (or `$TEKTO_DEV_FILE`). The `tekto` CLI and the stdio MCP adapter both use that server.

Dev-mode tools (first slice): `ui.tree`, `ui.screenshot`, `ui.click`, `ui.type`, `ui.patch`, `app.state`, `app.logs`. Unique live patches can write back to source. Tree actions can be recorded and replayed with `tekto::Recording` (see the agent guide); that helper does not capture a live session by itself.

`NodeKind::TextField` is a single-line field. The scene `text` is the committed value agents read and patch; `name` is the label. AccessKit maps it to `Role::TextInput` on the same id. Its value is the committed text with the current IME preedit inserted at the caret, so a composition shows up before commit. Desktop input is printable keys, Backspace, and winit `WindowEvent::Ime` (preedit and commit) on the shared Linux/Windows window path. IME is enabled only while a text field is focused. Printable keys are ignored during a preedit. Changing focus clears the previous field's preedit. A commit echo is ignored only until the event queue goes idle. We pass the caret rectangle to the OS (`set_ime_cursor_area`) and do not draw a candidate window. Still open: selection and marked-range highlight, arrow keys, copy and paste, a real Windows IME session (the event path is compiled, not run here), and the Android soft keyboard.

Hot reload (rebuild or reload TypeScript) and live patches (change a running node) are both intended in dev mode.

## TypeScript

Packages under `packages/` (`@tektogui/core`, `@tektogui/jsx-runtime`, `create-tektogui`) must work with **pnpm, npm, and bun**. We use a standard `workspaces` field. Do not use pnpm-only fields in published `package.json` files. `@tektogui/jsx-runtime` `mount` / `renderToOps` lower a general-purpose JSX tree to the same `HostOp` protocol QuickJS and the web target use. There are no Notes-only APIs in the runtime.

`tekto-js` embeds QuickJS-ng (via `rquickjs`, both MIT) on desktop behind the `quickjs` feature. It is off by default because it is slow to compile. A JS script talks to the Rust tree through the `HostOp` protocol (`tekto.create` / `setText` / `setProp` / `append` / `remove`); the web target will call the same ops from the browser later. Do not enable `quickjs` on PR CI.

## Native plugins

`tekto-native` is a small plugin API: files (read/write, open/save dialogs), clipboard, notifications, system tray, global shortcuts. The same API is meant to be called from Rust and from TypeScript. Each feature is filled per platform; stubs return a clear error.

Linux notifications are plain text (`Notifications::show(title, body)`). The call uses the freedesktop notification service on the session bus (`org.freedesktop.Notifications.Notify`) with a small in-tree D-Bus client, so no extra crate is added. The same call is `NativeOp` `notificationsShow` (`{"op":"notificationsShow","title":"...","body":"..."}`) through `apply_native_op` / `tekto-js::apply_native_host_op`. If there is no session bus, or the bus has no notification daemon, the call returns `NativeError::Unavailable` (it does not open a display). Windows uses the Windows Vista and later `Shell_NotifyIconW` balloon API with a temporary notification-area icon. The shell can suppress or delay notifications; success only means the shell accepted the request. Title and body are limited to 63 and 255 UTF-16 units. An unavailable shell returns `Unavailable`; other platforms return `Unsupported`. The Windows path is compile-checked with `cargo check --target x86_64-pc-windows-gnu -p tekto-native`, without a linker or Windows runtime. Display, session failure handling, and icon lifetime still need testing on Windows. A live check is off unless `TEKTO_NOTIFY_LIVE=1`.

Linux text clipboard is implemented with `arboard` (MIT OR Apache-2.0): `Clipboard::get_text` / `set_text` on `native()`, and the same calls through `NativeOp` (`clipboardGetText`, `clipboardSetText`) via `apply_native_op` / `tekto-js::apply_native_host_op`. X11 is used by default; Wayland is used when `WAYLAND_DISPLAY` is set. One `arboard::Clipboard` is kept for the process so the X11 selection survives `set_text` when no clipboard manager is running. If there is no clipboard session the call returns `NativeError::Unavailable` (no panic). Windows text clipboard uses Win32 `CF_UNICODETEXT` directly. Android stays a stub. An empty clipboard reads as an empty string. The Linux live round-trip test is off unless `TEKTO_CLIPBOARD_LIVE=1`. Cross-compiling from Linux does not test the Windows clipboard at runtime.

Linux file reads and writes are confined to `$XDG_DATA_HOME/tekto` (or `$HOME/.local/share/tekto`) with `openat` and `O_NOFOLLOW`. Windows uses the same methods under `%LOCALAPPDATA%\tekto`, opening each path component with `NtCreateFile` and refusing reparse points (junctions, symlinks, mount points). A junction above that root can still be followed when the root itself is opened. Dialogs stay unsupported on every platform.

## Compile times

- Default features: CPU renderer + desktop shell only.
- No wgpu, Vello, or QuickJS in the default graph.
- Dev profile: `debug = 1`, incremental on. A `dev-fast` profile exists if you want a bit of opt-level for demos.
- Later we can look at dylib / hot-patch for Rust, similar to Subsecond.

## Look and feel

Default widgets use a warm off-white page, charcoal ink, 24px page padding, 16px stack gap, and 20–22px text. No bright chrome. A `list` is a column of variable-height `list_item` rows (stable ids, AccessKit `List` / `ListItem`). It uses a `scroll` parent only when the rows are taller than the viewport. Rows are not virtualized. A `scroll` container clips overflowing children and shifts them by `scroll_y`. A mouse wheel notch moves 48px and a pointer drag pans by the pointer delta. The offset clamps to the content, with no rubber-band. Touch scrolling, animation, and themes come later.

On Linux, `Files` and `NativeOp` provide text and byte reads and writes under `$XDG_DATA_HOME/tekto` (or `$HOME/.local/share/tekto`). Paths must be relative, contain no traversal or symlinks, and intermediate directories must already exist. File dialogs and access outside this directory are not yet supported. `filesReadBytes` returns a JSON array encoded in the existing string result; `filesWriteBytes` accepts a `bytes` array.
