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

- `CpuRenderer` (default): tiny-skia, glyph outlines via Skrifa. Used for windows (through softbuffer), headless PNGs, tests, and the web canvas fallback. `render_tree_png` always uses this path. The browser glue decodes that PNG and calls `putImageData`.
- `GpuRenderer` (`--features gpu`): wgpu 29 (Vulkan, GLES, Metal; DX12 is off) + Vello 0.10. The desktop window records the same `paint_tree` calls (fills, text, clips; opacity is already in the color) and blits the GPU texture through softbuffer. If no adapter exists, the window falls back to `CpuRenderer`. WebGPU on the web is still later. The feature is off by default so everyday builds do not compile wgpu or Vello.

Apps never call tiny-skia or wgpu directly.

## Platform

`tekto-platform` hides the host:

- **Desktop (Windows, Linux, macOS):** winit window, softbuffer presents frames. Default paint is the CPU renderer. `--features gpu` paints with wgpu / Vello and still presents through softbuffer.
- **Android:** `examples/notes-android` is a `NativeActivity` APK (`cargo-apk2`, `winit` + `android-activity`). `android_main` calls `tekto::run_android`. The window / softbuffer surface is created on `resumed` and dropped on `suspended`. The CPU core also still builds headless for host tests and for `cargo check --target aarch64-linux-android`.
- **Web:** `crates/tekto-wasm` builds the core (tree, layout, text, CPU renderer, `HostOp`) for `wasm32-unknown-unknown`. `@tektogui/wasm` sends the same op JSON the desktop host uses. The browser JS engine runs the app, not QuickJS. The Notes TSX entry (`examples/notes/src/app.tsx`) uses the same JSX and signals API. `node packages/wasm/dev.js` bundles it with esbuild and serves a host page. App code has no DOM types. The host module `@tektogui/wasm/canvas` blits the CPU PNG onto a `<canvas>` (`paintCanvas`, `resizeCanvas`). `@tektogui/wasm/patch` (`applyUiPatch`) applies a live `ui.patch` on that host. WebGPU is still later. See [the web-build skill](../.agents/skills/web-build/SKILL.md).

## Agents

`tekto-a11y` builds an AccessKit tree from the scene (same stable ids, plus roles, labels, bounds, and focus). `GET /accesskit` and MCP `ui.accesskit` return that dump. The desktop window rebuilds it after every layout so a live patch can stay in sync. Publishing to the OS (AT-SPI / UIA / VoiceOver) is behind the optional `a11y-os` feature, which pulls in `accesskit_winit` and is off by default so compile times stay short.


`tekto-mcp` starts a localhost HTTP server and writes `tekto-dev.json` (or `$TEKTO_DEV_FILE`). The `tekto` CLI and the stdio MCP adapter both use that server.

Dev-mode tools (first slice): `ui.tree`, `ui.screenshot`, `ui.click`, `ui.type`, `ui.patch`, `app.state`, `app.logs`. Unique live patches can write back to source. Tree actions can be recorded and replayed with `tekto::Recording` (see the agent guide); that helper does not capture a live session by itself.

`NodeKind::TextField` is a single-line field. The scene `text` is the committed value agents read and patch; `name` is the label. AccessKit maps it to `Role::TextInput` on the same id. Its value is the committed text with the current IME preedit inserted at the caret, so a composition shows up before commit. Desktop input is printable keys, Backspace, and winit `WindowEvent::Ime` (preedit and commit) on the shared Linux/Windows window path. IME is enabled only while a text field is focused. Printable keys are ignored during a preedit. Changing focus clears the previous field's preedit. A commit echo is ignored only until the event queue goes idle. We pass the caret rectangle to the OS (`set_ime_cursor_area`) and do not draw a candidate window. Still open: selection and marked-range highlight, arrow keys, copy and paste, a real Windows IME session (the event path is compiled, not run here), and the Android soft keyboard.

Hot reload (rebuild or reload TypeScript) and live patches (change a running node) are both intended in dev mode.

## TypeScript

Packages under `packages/` (`@tektogui/core`, `@tektogui/jsx-runtime`, `create-tektogui`) must work with **pnpm, npm, and bun**. We use a standard `workspaces` field. Do not use pnpm-only fields in published `package.json` files. `@tektogui/jsx-runtime` `mount` / `renderToOps` lower a general-purpose JSX tree to the same `HostOp` protocol QuickJS and the web target use. There are no Notes-only APIs in the runtime.

`tekto-js` embeds QuickJS-ng (via `rquickjs`, both MIT) on desktop behind the `quickjs` feature. It is off by default because it is slow to compile. A JS script talks to the Rust tree through the `HostOp` protocol (`tekto.create` / `setText` / `setProp` / `append` / `remove`). The same global has `tekto.applyNativeOp(json)` for one `NativeOp` (clipboard, files, notifications, tray, shortcuts). `@tektogui/core` exports `nativeOpJson` to build that JSON. The web glue (`@tektogui/wasm`) calls the tree ops on the WASM module. Do not enable `quickjs` on PR CI or on the wasm build.

## Native plugins

`tekto-native` is a small plugin API: files (read/write, open/save dialogs), clipboard, notifications, system tray, global shortcuts. The same API is meant to be called from Rust and from TypeScript. Each feature is filled per platform; stubs return a clear error.

Linux system tray (`SystemTray`) is optional. Enable it with the `tray` feature on `tekto` or `tekto-native` (`cargo test -p tekto-native --features tray`). The default build does not compile the client, so apps that do not need a tray stay smaller. There is no extra crate and no GTK: the feature speaks StatusNotifierItem and a D-Bus menu on the session bus. `show_icon(label, menu)` shows the icon, `hide_icon` removes it, and `set_label` updates the tooltip. A menu click is queued for `take_click` (`Some(id)`) and is also passed to `set_on_click`. The same calls are `NativeOp` values `trayShow`, `trayHide`, `traySetLabel`, and `trayTakeClick`. No session bus, or a bus with no status notifier watcher, returns `NativeError::Unavailable` (the call does not open a display). Without the feature, and on Windows (issue #47), tray calls return `Unsupported`. A live check is off unless `TEKTO_TRAY_LIVE=1`.

Linux global shortcuts (`GlobalShortcuts`) are optional. Enable them with the `shortcuts` feature on `tekto`, `tekto-js`, or `tekto-native` (`cargo test -p tekto-native --features shortcuts`). The default build does not compile the client. The feature uses `x11rb` 0.13.2 (MIT OR Apache-2.0) to grab a key on the X11 root window. There is no GTK. `register(accelerator, id)` binds a chord such as `Ctrl+K`, `unregister(id)` releases it, and a press is queued for `take_activation` (`Some(id)`) and also passed to `set_on_activation`. The same calls are `NativeOp` values `shortcutsRegister`, `shortcutsUnregister`, and `shortcutsTakeActivation` through `apply_native_op`, `tekto-js::apply_native_host_op`, and QuickJS `tekto.applyNativeOp`. The activation hook is Rust-only; TypeScript polls `shortcutsTakeActivation`.

A missing display returns `NativeError::Unavailable` with reason `no X11 display`. Wayland without `DISPLAY` cannot use an X11 grab. The global shortcuts portal would ask the user to bind the chord, and this feature does not open that portal, so the error names the compositor and the portal. A display name that is set, but whose server refuses the connection, returns reason `X11 display refused the connection`. An X11 or XWayland server that refuses the grab (another client, or a compositor that blocks grabs) returns reason `X11 key grab was denied`. Any display whose number is 1 or 2 is refused and never opened, including `host:display.screen` and Unix sockets such as `/tmp/.X11-unix/X1`. Registering the same keycode and modifiers under a second id is rejected so one ungrab cannot drop the other id. The same id may replace its own chord. Without the feature, and on Windows (issue #49), shortcut calls return `Unsupported`. A live grab is off unless `TEKTO_SHORTCUTS_LIVE=1`.

Linux notifications are plain text (`Notifications::show(title, body)`). The call uses the freedesktop notification service on the session bus (`org.freedesktop.Notifications.Notify`) with a small in-tree D-Bus client, so no extra crate is added. The same call is `NativeOp` `notificationsShow` (`{"op":"notificationsShow","title":"...","body":"..."}`) through `apply_native_op` / `tekto-js::apply_native_host_op`. If there is no session bus, or the bus has no notification daemon, the call returns `NativeError::Unavailable` (it does not open a display). Windows uses the Windows Vista and later `Shell_NotifyIconW` balloon API with a temporary notification-area icon. The shell can suppress or delay notifications; success only means the shell accepted the request. Title and body are limited to 63 and 255 UTF-16 units. An unavailable shell returns `Unavailable`; other platforms return `Unsupported`. The Windows path is compile-checked with `cargo check --target x86_64-pc-windows-gnu -p tekto-native`, without a linker or Windows runtime. Display, session failure handling, and icon lifetime still need testing on Windows. A live check is off unless `TEKTO_NOTIFY_LIVE=1`.

Linux text clipboard is implemented with `arboard` (MIT OR Apache-2.0): `Clipboard::get_text` / `set_text` on `native()`, and the same calls through `NativeOp` (`clipboardGetText`, `clipboardSetText`) via `apply_native_op` / `tekto-js::apply_native_host_op`. X11 is used by default; Wayland is used when `WAYLAND_DISPLAY` is set. One `arboard::Clipboard` is kept for the process so the X11 selection survives `set_text` when no clipboard manager is running. If there is no clipboard session the call returns `NativeError::Unavailable` (no panic). Windows text clipboard uses the same crate without Wayland. Android stays a stub. An empty clipboard reads as an empty string. A live round-trip test is off unless `TEKTO_CLIPBOARD_LIVE=1`. Windows is compile-checked from Linux (`x86_64-pc-windows-gnu`); running the clipboard still needs a Windows machine.

Linux file reads and writes are confined to `$XDG_DATA_HOME/tekto` (or `$HOME/.local/share/tekto`) with `openat` and `O_NOFOLLOW`. Windows uses the same methods under `%LOCALAPPDATA%\tekto`, opening each path component with `NtCreateFile` and refusing reparse points (junctions, symlinks, mount points). A junction above that root can still be followed when the root itself is opened. Dialogs stay unsupported on every platform.

## Compile times

- Default features: CPU renderer + desktop shell only.
- No wgpu, Vello, or QuickJS in the default graph. Opt into the GPU backend with `--features gpu` (not on PR CI).
- Dev profile: `debug = 1`, incremental on. A `dev-fast` profile exists if you want a bit of opt-level for demos.
- Later we can look at dylib / hot-patch for Rust, similar to Subsecond.

## Look and feel

Default widgets use a warm off-white page, charcoal ink, 24px page padding, 16px stack gap, and 20–22px text. No bright chrome. A `list` is a column of variable-height `list_item` rows (stable ids, AccessKit `List` / `ListItem`). It uses a `scroll` parent only when the rows are taller than the viewport. Layout then keeps the visible rows plus a 64px overscan, and spacers hold the rest of the scroll height. A `scroll` container clips overflowing children and shifts them by `scroll_y`. A mouse wheel notch moves 48px and a pointer drag pans by the pointer delta. The offset clamps to the content, with no rubber-band. Touch scrolling and themes come later.

## Motion

A node can tween opacity, x, y, width, and height. Call `Tree::transition` with a `StyleTransition`. The default is 180 ms and a smoothstep curve (no bounce, no overshoot). `Tree::advance(time_ms)` writes the in-between values. The desktop loop does that after layout, about every 16 ms, while a tween is running. Headless tests pass their own clock.

A new channel starts on the first `advance` after `transition`, not on the last clock sample. The desktop loop stops ticking while nothing is moving, then advances with elapsed wall time. Anchoring at request time would skip a tween that starts after a long idle.

Several channels can run on one node. A later `transition` replaces only the props it names. Opacity stays on the node. When a position or size channel finishes, including a zero-duration snap or reduced motion, the target is stored on `LayoutStyle` (`x`, `y`, `width`, `height`). The next layout keeps those values. `x` and `y` are `NaN` until a transition sets them.

Opacity multiplies the node and its children. `0` skips that subtree.

Each tick is a full CPU paint. There are no dirty rectangles. `animation_frames_differ_and_stay_cheap` in `tekto-render` paints 12 frames of three rects at 200×120 and expects that batch to finish in under two seconds in a debug build.

`TEKTO_REDUCE_MOTION=1` (or `true` / `yes` / `on`) snaps tweens to the end. The desktop loop reads that when the window starts. AccessKit does not expose a reduced-motion flag, and this crate does not read GTK, GNOME, or Windows animation settings. That OS check is a follow-up.

From JavaScript, `transitionOp(id, { opacity, x, y, width, height, durationMs })` in `@tektogui/core` builds a `HostOp`. QuickJS `tekto.transition` takes that same props object, or `(id, key, value, durationMs)` for one prop. Each call merges. The host starts the tween; it does not advance time.

On Linux, `Files` and `NativeOp` provide text and byte reads and writes under `$XDG_DATA_HOME/tekto` (or `$HOME/.local/share/tekto`). Paths must be relative, contain no traversal or symlinks, and intermediate directories must already exist. On Linux, `Files::open_dialog` and `save_dialog` use the installed `zenity` helper. A chosen file is `Ok(Some(absolute path))`. Cancel is `Ok(None)`. The matching native ops are `filesOpenDialog` and `filesSaveDialog`. A missing display or a missing `zenity` is `Unavailable`. On Windows the same methods call `GetOpenFileNameW` and `GetSaveFileNameW` (`comdlg32`). Cancel (`CommDlgExtendedError` is 0) is `Ok(None)`. A session that cannot show the dialog is `Unavailable`. That Windows path is compile-checked only; a live dialog still needs a Windows machine (issue #45). Other platforms stay `Unsupported`. Rust apps can use the selected path with standard filesystem APIs; the sandboxed `Files` read and write methods do not accept it. The TypeScript bridge returns the same path but does not yet provide unrestricted file access. `filesReadBytes` returns a JSON array encoded in the existing string result; `filesWriteBytes` accepts a `bytes` array.
