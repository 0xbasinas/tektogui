---
name: add-native-feature
description: Implement a tekto-native plugin (files, open/save dialogs, clipboard, notifications, system tray, or global shortcuts) for one platform at a time, with the same Rust and TS API surface. Use when filling in or extending crates/tekto-native.
---

# Add a native feature

`crates/tekto-native/src/lib.rs` holds small traits: `Files`, `Clipboard`, `Notifications`, `SystemTray`, `GlobalShortcuts`. `StubNative` is the current implementation. Linux text clipboard (`get_text` / `set_text`) uses `arboard` (MIT OR Apache-2.0, `default-features = false`, `wayland-data-control`) in `src/linux.rs`. Linux file reads and writes are sandboxed in the same file (`libc` 0.2, MIT OR Apache-2.0, Linux-only). Linux open and save dialogs call the installed `zenity` helper from `src/linux_dialog.rs` (no extra crate). Linux notifications (`Notifications::show`) talk to the freedesktop notification service in `src/linux_notify.rs` (session bus only; no extra crate). Android notifications use `NotificationManager` in `src/android_notify.rs` (`jni` and `ndk-context`, both MIT OR Apache-2.0, Android-only). Android text clipboard uses `ClipboardManager` in `src/android_clipboard.rs` (same two crates, no new dependency). Android app-private file reads and writes use `Context.getFilesDir()` in `src/android_files.rs` (`jni` and `ndk-context` as above, plus `libc` 0.2, MIT OR Apache-2.0, a new Android-only direct dependency). Windows text clipboard uses Win32 `CF_UNICODETEXT` directly in `src/windows.rs`, without a Windows arboard dependency. Windows file reads and writes use the same trait methods, confined with `NtCreateFile` (no extra crate). Windows notifications use `Shell_NotifyIconW` in `src/windows_notify.rs` (no extra crate). Windows open and save dialogs use `GetOpenFileNameW` and `GetSaveFileNameW` in `src/windows_dialog.rs` (no extra crate). The system tray is the `tray` feature: `src/linux_tray.rs` on Linux and `src/windows_tray.rs` on Windows (no extra crate). Global shortcuts are the `shortcuts` feature: `src/linux_shortcuts.rs` (`x11rb` 0.13.2, MIT OR Apache-2.0) and `src/windows_shortcuts.rs` (`RegisterHotKey`, no extra crate). A live Windows hotkey check is still issue #49. macOS text clipboard uses `arboard` in `src/macos.rs`; it has not been compiled or run on a Mac (see `docs/macos-handoff.md`). Any other platform and capability combination that is not implemented returns `NativeError::Unsupported { capability, platform }`; on macOS that includes every plugin except text clipboard. Without `tray` or `shortcuts`, those methods also return `Unsupported`.

A cross-check does not run Windows APIs. Notifications and file dialogs have not been checked on Windows.

The handoff probe tested a clipboard and file round trip on Windows. Cross-application clipboard use, contention, file edge cases, notifications and dialogs still need runtime checks.

Windows code is compile-checked on Linux with `cargo check --target x86_64-pc-windows-gnu -p tekto-native` and `cargo check --target x86_64-pc-windows-msvc -p tekto-native` (no linker, no runtime). Do not treat those checks as a runtime test. Runtime steps are the [windows-test](../windows-test/SKILL.md) skill and [docs/windows-handoff.md](../../../docs/windows-handoff.md). Do not close issues #3, #26, or #28 until that Windows record exists. PRs go to `nightly` only.

Windows clipboard limits: one `OpenClipboard` try (busy clipboard is `NativeError::Unavailable`, no retry); an interior NUL is rejected; line endings are not converted; the owner `HWND` is destroyed before `set_text` returns. New files inherit the app-data ACL (not Unix `0600`).

A present platform implementation that cannot reach the OS service (headless Linux, no `DISPLAY` and no `WAYLAND_DISPLAY`, no session bus / notification daemon, no status notifier watcher, or an X11 grab the server refuses) returns `NativeError::Unavailable { capability, platform, reason }`, not `Unsupported`. Do not open displays `:1` or `:2`. Opt-in live checks are `TEKTO_CLIPBOARD_LIVE=1`, `TEKTO_NOTIFY_LIVE=1`, `TEKTO_TRAY_LIVE=1`, and `TEKTO_SHORTCUTS_LIVE=1`.

JS calls clipboard, files, notifications, tray, and shortcuts through `NativeOp` and `apply_native_op`. `tekto-js::apply_native_host_op` wraps that in `JsError`. It is not a scene-tree `HostOp` (get returns text, not a node id). With `--features quickjs`, a script calls `tekto.applyNativeOp(json)` where `json` is one `NativeOp` object. `@tektogui/core` `nativeOpJson(op)` builds that string. Do not enable the `quickjs` feature by default. A headless or unsupported call throws (`not available` or `not implemented`) and does not panic.

`NativeOp` is internally tagged (`#[serde(tag = "op", rename_all = "camelCase")]`). Clipboard ops:

- `{"op":"clipboardGetText"}`
- `{"op":"clipboardSetText","text":"..."}`

File ops (Linux, Windows, and Android; other platforms return `Unsupported` for `files.read` / `files.write`):

- `{"op":"filesReadText","path":"notes/a.txt"}` — `apply_native_op` returns `Some(text)`
- `{"op":"filesWriteText","path":"notes/a.txt","text":"..."}` — returns `None`
- `{"op":"filesReadBytes","path":"blob"}` — returns `Some` of a JSON byte array encoded as a string, for example `"[0,255]"`. The op result type is still `Option<String>`.
- `{"op":"filesWriteBytes","path":"blob","bytes":[0,255]}` — `bytes` is a JSON array of numbers 0–255 (`Vec<u8>`). Returns `None`.

Dialog ops (Linux and Windows; Android is explicitly `Unsupported` because it has no app-private file picker in this plugin, and so are other platforms, for `files.open_dialog` / `files.save_dialog`):

- `{"op":"filesOpenDialog"}` — `Ok(Some(absolute path))`, or `Ok(None)` on cancel
- `{"op":"filesSaveDialog"}` — same result shape. Save asks the user to confirm overwrite.

Notification op (Linux, Windows, and Android; other platforms return `Unsupported` for `notifications.show`):

- `{"op":"notificationsShow","title":"...","body":"..."}` — returns `None`. Title and body are plain text.

Tray ops (Linux and Windows, and only when the `tray` feature is on; otherwise `Unsupported` for `tray.show`, `tray.hide`, `tray.set_label`, `tray.take_click`, and `tray.on_click`):

- `{"op":"trayShow","label":"Tekto","menu":[{"id":"quit","label":"Quit"}]}` — returns `None`
- `{"op":"trayHide"}` — returns `None`
- `{"op":"traySetLabel","label":"Notes"}` — returns `None`
- `{"op":"trayTakeClick"}` — `Some(id)` for the oldest menu click, or `None` when the queue is empty

Shortcut ops (Linux and Windows, and only when the `shortcuts` feature is on; otherwise `Unsupported` for `shortcuts.register`, `shortcuts.unregister`, `shortcuts.take_activation`, and `shortcuts.on_activation`):

- `{"op":"shortcutsRegister","accelerator":"Ctrl+K","id":"search"}` — returns `None`
- `{"op":"shortcutsUnregister","id":"search"}` — returns `None`. A missing id is success.
- `{"op":"shortcutsTakeActivation"}` — `Some(id)` for the oldest chord, or `None` when the queue is empty

`set_on_activation` is Rust-only (same idea as `set_on_click`). TypeScript polls with `tekto.applyNativeOp(nativeOpJson({ op: "shortcutsTakeActivation" }))`. Register and unregister use `shortcutsRegister` and `shortcutsUnregister` the same way.

## Linux global shortcuts

`GlobalShortcuts` on Linux is feature `shortcuts` (`tekto` and `tekto-js` forward it to `tekto-native`). Default builds leave it out. The implementation is `src/linux_shortcuts.rs`. It uses `x11rb` 0.13.2 (MIT OR Apache-2.0, default features off, no libxcb and no GTK).

`register(accelerator, id)` grabs the chord on the X11 root window (`XGrabKey`, async). Also grabs the CapsLock and NumLock variants. `unregister(id)` releases that grab. A press queues `id` for `take_activation` and calls `set_on_activation` (`Option<fn(&str)>`) on the shortcut thread. The hook must return quickly. It must not call `register` or `unregister` (those wait on the same thread).

Accelerators look like `Ctrl+K`, `Alt+F4`, or `Ctrl+Shift+Space`. Modifiers are `Ctrl` / `Control`, `Shift`, `Alt` / `Mod1`, and `Super` / `Meta` / `Mod4`. The key is a letter, a digit, F1–F24, or Space, Tab, Enter, Escape, or an arrow. Alt is X11 Mod1 and Super is Mod4.

Permission and compositor limits:

- No `DISPLAY` and no `WAYLAND_DISPLAY`: `NativeError::Unavailable`, reason `no X11 display`. The call does not open a display. Headless tests clear both variables (`TEKTO_SHORTCUTS_HEADLESS_CHILD=1`).
- Wayland with no `DISPLAY`: there is no client key grab. The freedesktop GlobalShortcuts portal would show a permission dialog and let the compositor bind the chord. This feature does not open that portal. Reason: `Wayland compositor does not allow an X11 key grab; the global shortcuts portal is not used`.
- X11, including XWayland: if the display name is set but the server refuses the connection, the reason is `X11 display refused the connection`. If the server is up but refuses the grab (`BadAccess`, another client, or a compositor that blocks global grabs), the reason is `X11 key grab was denied`.
- Displays `:1` and `:2` are reserved. The client refuses them with `refusing reserved display :1 or :2` and does not connect. Do not point tests at those displays.

`TEKTO_SHORTCUTS_LIVE=1` grabs `Ctrl+Shift+F24` and releases it. Do not set it when `DISPLAY` is `:1` or `:2`. Run the feature tests with `cargo test -p tekto-native --features shortcuts` and `cargo test -p tekto-js --features shortcuts`. Do not add `shortcuts` to default PR CI.

## Windows global shortcuts

`GlobalShortcuts` on Windows is the same `shortcuts` feature and the same methods (`register`, `unregister`, `take_activation`, `set_on_activation`). The implementation is `src/windows_shortcuts.rs`. No extra crate. It calls `RegisterHotKey` and `UnregisterHotKey` (`user32`) with a null window, on a thread that pumps `GetMessageW`. `WM_HOTKEY` queues the id for `take_activation` and calls `set_on_activation` on that thread, including when the app window is in the background. The hook must return quickly. It must not call `register` or `unregister` (those wait on the same thread).

Chord text matches Linux: `Ctrl+K`, `Alt+F4`, `Ctrl+Shift+Space`, `Win+K` (`Super` / `Meta` / `Win` / `Mod4` are `MOD_WIN`). The key is a letter, a digit, F1–F24, or Space, Tab, Enter, Escape, or an arrow. `MOD_NOREPEAT` is always set, so a held key fires once. A bad chord or id is `NativeError::Message` and does not start the thread. The same id may replace its chord. A second id on the same virtual key and modifiers is `shortcuts.register: chord is already registered`. A missing unregister is success.

OS limits:

- The hotkey belongs to the interactive session and window station of the registering thread. A service in session 0 does not see the signed-in user's keys. If the thread or its message queue cannot start, the reason is `Windows hotkey thread is unavailable`.
- Another process in that session already owns the chord: `NativeError::Unavailable`, reason `Windows hotkey is already registered`.
- Windows reserves some chords. `Ctrl+Alt+Delete` cannot be registered. `Win+L` and other shell `Win` chords often fail too. Those, and any other denial, use reason `Windows hotkey registration was denied`.
- An elevated (high integrity) process and a normal process do not share registrations. The higher integrity owner keeps the chord. This API cannot unregister a hotkey it did not register.
- Headless and non-Windows builds do not call this API. Without the feature the calls return `Unsupported`. Do not open displays `:1` or `:2` on the Linux path.

There is no Windows runtime test. Compile-check only:

```bash
cargo check -p tekto-native --target x86_64-pc-windows-gnu --features shortcuts
cargo check -p tekto --target x86_64-pc-windows-gnu --features shortcuts
cargo check -p tekto-js --target x86_64-pc-windows-gnu --features shortcuts
```

Those commands do not link or run on Windows. Register, unregister, and a callback while the app is in the background still need a Windows machine. Issue #49 stays open until that runtime check exists.

## Linux tray

`SystemTray` on Linux is feature `tray` (`tekto` forwards it to `tekto-native`). Default builds leave it out. The implementation is `src/linux_tray.rs`: StatusNotifierItem plus a D-Bus menu on the session bus. No GTK, no libappindicator, no extra crate.

`show_icon(label, menu)` registers with `org.kde.StatusNotifierWatcher`. `hide_icon` drops that connection. `set_label` stores the tooltip and, when the icon is visible, emits `NewTitle`. Menu rows are `TrayMenuItem { id, label }` (at most 16, unique non-empty ids, no NUL). A click queues `id` for `take_click` and calls the `set_on_click` hook (`Option<fn(&str)>`) on the tray thread. The hook must return quickly. It may call `hide_icon`. It must not block on tray work from that same thread.

No session bus, a refused connection, or a missing watcher is `NativeError::Unavailable` with reason `no session bus or status notifier watcher`. The call does not open `DISPLAY`. Headless tests clear `DBUS_SESSION_BUS_ADDRESS` and point `XDG_RUNTIME_DIR` at an empty directory (`TEKTO_TRAY_HEADLESS_CHILD=1`). `TEKTO_TRAY_LIVE=1` registers a real icon and hides it again. Do not set it when `DISPLAY` is `:1` or `:2`. Run the feature tests with `cargo test -p tekto-native --features tray`. Do not add `tray` to default PR CI.

## Windows tray

`SystemTray` on Windows is the same `tray` feature and the same methods (`show_icon`, `hide_icon`, `set_label`, `take_click`, `set_on_click`). The implementation is `src/windows_tray.rs`. No extra crate. It uses `Shell_NotifyIconW` (`shell32`) on a hidden zero-size `WS_POPUP` window. `NIM_ADD` shows the icon (`NIF_MESSAGE | NIF_ICON | NIF_TIP`, `IDI_APPLICATION`). `NIM_MODIFY` updates the tooltip. `NIM_DELETE` removes the icon. A left click, right click, or double click calls `TrackPopupMenu` (`user32`) with the menu rows. The chosen row is queued for `take_click` and passed to `set_on_click` on the tray thread. The hook must return quickly. It may call `hide_icon`. It must not block on tray work from that same thread.

Menu rules match Linux: at most 16 rows, unique non-empty ids, no NUL. The tooltip must also fit in 127 UTF-16 units (`szTip` is 128 units including NUL). A longer label or a bad menu is `NativeError::Message`. If the window cannot be created, or `Shell_NotifyIconW` rejects the icon, the error is `NativeError::Unavailable` with reason `Windows shell notification area is unavailable`. Headless and non-Windows builds do not call this API. Without the feature the calls return `Unsupported`.

There is no Windows runtime test. Compile-check only:

```bash
cargo check -p tekto-native --target x86_64-pc-windows-gnu --features tray
cargo check -p tekto --target x86_64-pc-windows-gnu --features tray
```

Those commands do not link or run on Windows. Icon lifetime, the popup menu, and a session with no notification area still need a Windows machine. Issue #47 stays open until that runtime check exists.

## Linux notifications

`Notifications::show` on Linux sends one freedesktop notification: app name `tekto`, summary = title, body = body, no icon, no actions, default timeout. It connects to `DBUS_SESSION_BUS_ADDRESS`, or to `$XDG_RUNTIME_DIR/bus` when that socket exists. No session bus, a refused connection, or a bus error from `org.freedesktop.Notifications` (no daemon) is `NativeError::Unavailable` with reason `no session bus or notification daemon`. The call does not open `DISPLAY`. A NUL in the title or body is `NativeError::Message`. Headless tests clear `DBUS_SESSION_BUS_ADDRESS` and point `XDG_RUNTIME_DIR` at an empty directory (`TEKTO_NOTIFY_HEADLESS_CHILD=1`). `TEKTO_NOTIFY_LIVE=1` shows a real notification and is off by default. Do not set it when `DISPLAY` is `:1` or `:2`.

## Windows notifications

`Notifications::show` on Windows uses the Vista-and-later balloon API `Shell_NotifyIconW` (`shell32`), plus `CreateWindowExW`, `DestroyWindow`, and `LoadIconW` (`user32`). No extra crate. The call adds a temporary notification-area icon (`NIM_ADD`, `NIF_ICON | NIF_INFO`, `NIIF_INFO`, `IDI_APPLICATION`) on a hidden `STATIC` window, waits 30 seconds on that thread, then deletes the icon (`NIM_DELETE`) and destroys the window. Success means the shell accepted `NIM_ADD`. The shell can still hide or delay the balloon. There is no Windows runtime test; this host did not show a toast.

Title must fit in 63 UTF-16 units and body in 255 (the API buffers are 64 and 256, including the NUL). A NUL or a longer string is `NativeError::Message`. If the window, icon, or `Shell_NotifyIconW` call fails, the error is `NativeError::Unavailable` with reason `Windows shell notification area is unavailable`. Compile-check only:

```bash
cargo check --target x86_64-pc-windows-gnu -p tekto-native
```

That command does not link or run on Windows. Icon lifetime, session failures, and whether a balloon is visible still need a Windows machine. Issue #37 stays open until that runtime check exists.

## Android notifications

`Notifications::show` on Android posts one status-bar notification through `NotificationManager` (`src/android_notify.rs`). No GPL library. Android-only dependencies are `jni` 0.22.4 and `ndk-context` 0.1.1, both MIT OR Apache-2.0. `jni` uses default features off, so it does not launch a JVM. The activity or application context is the handle `android-activity` stores in `ndk-context` before `android_main`. If that handle is missing, the error is `NativeError::Unavailable` with reason `Android activity context is not ready`.

The call creates the channel `tekto` on API 26+, then reads it back with `NotificationManager.getNotificationChannel`. `createNotificationChannel` does not change the importance of a channel the user already blocked. If `getImportance` is `IMPORTANCE_NONE` (0), the error is `NativeError::Unavailable` with reason `the tekto notification channel is blocked`. A missing channel after create uses reason `the tekto notification channel is missing`. Below API 26 there is no channel check. The builder uses title, body, and `android.R.drawable.ic_dialog_info`. The notification id is `1`, so a later show replaces the previous one. A NUL in the title or body is `NativeError::Message`.

Manifest, for every APK that calls this (cargo-apk2 metadata):

```toml
[[package.metadata.android.uses_permission]]
name = "android.permission.POST_NOTIFICATIONS"
```

`POST_NOTIFICATIONS` is a runtime permission on API 33+ when `targetSdkVersion` is 33 or higher. Below that target the system grants it at install. The plugin calls `checkSelfPermission`. If it is not granted and the context is an `Activity`, it calls `requestPermissions` so the system dialog can appear, then returns `NativeError::Unavailable` with reason `POST_NOTIFICATIONS was denied`. The grant arrives later. Call `show` again after the user allows it. If notifications are turned off for the app, the reason is `notifications are disabled for this app`. That check is `areNotificationsEnabled` and does not see a blocked channel. The channel check above covers that case. A `SecurityException` from `notify` uses the same denial error. Other JNI failures are `NativeError::Message`. None of these panic.

`examples/notes-android` declares `POST_NOTIFICATIONS`. Its `target_sdk_version` is still 30, so the runtime dialog is not required until that target is raised.

Host tests cover the denial and disabled decisions (`allow_post`) and a blocked or missing channel (`allow_channel`) without a device. Compile-check:

```bash
cargo check -p tekto-native --target aarch64-linux-android
```

That does not install an APK and does not post a notification. A live notice still needs a device or an emulator. Do not start an emulator for this check.

## Android clipboard

`Clipboard::get_text` and `set_text` on Android use `ClipboardManager` (`src/android_clipboard.rs`). The `NativeOp` names stay `clipboardGetText` and `clipboardSetText`. No new crate. The same Android-only `jni` 0.22.4 and `ndk-context` 0.1.1 dependencies as notifications apply (MIT OR Apache-2.0). `jni` keeps default features off.

The context is the `ndk-context` handle. If it is missing, both calls return `NativeError::Unavailable` with reason `Android activity context is not ready`. A null `getSystemService("clipboard")` uses reason `Android clipboard service is unavailable` (`clipboard.get` or `clipboard.set`). A `SecurityException` uses reason `clipboard access was denied`. Other JNI failures are `NativeError::Message`. None of these panic.

On API 29+ the OS only returns clipboard text to the focused app or the default IME. This plugin is not an IME. It reads `Activity.hasWindowFocus`. If that is false, `get_text` returns `Unavailable` with reason `the app is in the background` and does not call `getPrimaryClip` (so Android 12 does not toast a background paste). Below API 29 a background read is allowed. A context that is not an `Activity` counts as unfocused. `set_text` calls `ClipData.newPlainText` and `setPrimaryClip` and does not need focus. Android 12+ may toast when the app copies or reads. That toast is not an error.

A NUL in `set_text` is `NativeError::Message` (`clipboard.set: text must not contain NUL`). X11 and Windows clipboard owners stop at the first NUL, so the Android path rejects it instead of writing a truncated string. An empty string is a valid write. `get_text` returns `""` when there is no clip, when `coerceToText` is null, or when the clip is not text.

Host tests use an in-memory clipboard for the success path, plus `allow_read`, `require_context`, and `require_service`. Compile-check:

```bash
cargo check -p tekto-native --target aarch64-linux-android
```

That does not install an APK and does not touch the device clipboard. A live round trip still needs a device or an emulator. Do not start an emulator for this check.

## Linux file sandbox

`Files::read_to_string`, `write_string`, `read_bytes`, and `write_bytes` on Linux go through `linux::read_file` / `write_file`. The root is `$XDG_DATA_HOME/tekto` when `XDG_DATA_HOME` is absolute, otherwise `$HOME/.local/share/tekto`. The root is created if needed. Paths must be relative and made only of normal components: no absolute paths, empty segments, `.`, `..`, or a NUL. Each component is opened with `openat` and `O_NOFOLLOW` (and `O_DIRECTORY` for parents), so a symlink or a rename out of the tree fails. Parent directories must already exist; the plugin does not create them. New files are mode `0o600`. `read_to_string` is UTF-8 over `read_bytes`. The sandboxed read and write methods do not accept a dialog path. Headless confinement tests set `XDG_DATA_HOME` in a child process (`TEKTO_FILES_TEST_CHILD=1`) so they do not write the developer’s real app-data directory.

## Android file sandbox

`Files::read_to_string`, `write_string`, `read_bytes`, and `write_bytes` on Android go through `src/android_files.rs`. The root is `Context.getFilesDir()`, read through `jni` and `ndk-context`. Path rules match Linux: relative, normal components only. The `openat` walk uses `O_NOFOLLOW`, with `O_DIRECTORY` on parent components, so a symlink fails. Parent directories must already exist. New files are mode `0o600`. If the context or the files dir is missing, the error is `NativeError::Unavailable`. There is no live device test; host tests cover the path walk only. Compile-check with `cargo check -p tekto-native --target aarch64-linux-android`. Do not start an emulator.

## Linux open and save dialogs

`Files::open_dialog` and `Files::save_dialog` on Linux run `zenity --file-selection` from `src/linux_dialog.rs`. Save also passes `--save --confirm-overwrite`. No new crate. A chosen file is `Ok(Some(path))` only when the path is absolute. Cancel (zenity exit code 1 and empty stderr) is `Ok(None)`. If both `DISPLAY` and `WAYLAND_DISPLAY` are missing or empty, if `zenity` cannot be started, or if the helper fails another way, the error is `NativeError::Unavailable` with reason `no usable display or zenity file dialog`. A non-UTF-8 path is `NativeError::Message`. Do not open displays `:1` or `:2`. Rust can open the selected path with normal filesystem APIs. The TypeScript bridge returns the same path string and does not yet read that file.

## Windows open and save dialogs

`Files::open_dialog` and `Files::save_dialog` on Windows call `GetOpenFileNameW` and `GetSaveFileNameW` from `comdlg32` (`src/windows_dialog.rs`). No extra crate. The struct is `OPENFILENAMEW`. Flags are `OFN_EXPLORER`, `OFN_HIDEREADONLY`, `OFN_NOCHANGEDIR`, and `OFN_PATHMUSTEXIST`. Open also sets `OFN_FILEMUSTEXIST`. Save also sets `OFN_OVERWRITEPROMPT` (same idea as zenity `--confirm-overwrite`). The filter is all files. The path buffer is 32768 UTF-16 units. A chosen file is `Ok(Some(path))` only when the path is a drive path (`C:\...`) or a UNC path (`\\server\share\...`). Cancel is `Get*FileNameW` returning false and `CommDlgExtendedError` returning 0, which is `Ok(None)`. Any other dialog error is `NativeError::Unavailable` with reason `Windows common file dialog is unavailable` (no interactive desktop, initialization failure, or a buffer the API rejects). A path that is not absolute uses reason `file dialog did not return an absolute path`. A UTF-16 sequence that is not Unicode is `NativeError::Message`. The call blocks on the thread that invokes it. It does not change the process directory. Headless and non-Windows builds do not call this API. There is no Windows runtime test; this host did not open a dialog. Compile-check only:

```bash
cargo check --target x86_64-pc-windows-gnu -p tekto-native
```

That command does not link or run on Windows. Issue #45 stays open until a Windows machine shows the dialog, confirms cancel, and confirms overwrite.

## Windows file sandbox

Same shape as Linux: `read_to_string`, `write_string`, `read_bytes`, and `write_bytes` under an app-data root. The root is `%LOCALAPPDATA%\tekto` when `LOCALAPPDATA` is absolute, otherwise `%USERPROFILE%\AppData\Local\tekto`. The root is created if needed. Paths must be relative and made only of normal components (`/` or `\`). Rejected: absolute paths, drive prefixes, UNC, empty segments, `.`, `..`, NUL, `:` (alternate data streams), `* ? " < > |`, and a component that ends in a space or a dot.

Each component is opened with `NtCreateFile` relative to the previous directory handle, with `FILE_OPEN_REPARSE_POINT`. If the opened object has `FILE_ATTRIBUTE_REPARSE_POINT` (junction, symlink, mount point, or cloud placeholder), the call fails and does not follow it. Reserved DOS device names are rejected, including names with extensions. Parent directories must already exist. New files are created only when the name is absent; an existing reparse point is not overwritten. `read_to_string` is UTF-8 over `read_bytes`.

Residual risk: ancestors of the app-data root are opened by Win32 path (`create_dir_all` and the root `NtCreateFile` on `\??\...`), so a junction above the root is followed the same way any app opens `%LOCALAPPDATA%`. Inside the root, a junction cannot retarget an open that starts from the root handle. Hard links inside the tree still name the same file data (same as Linux `openat`). OneDrive and other cloud placeholders are reparse points and fail the same check. There is no Windows runtime test in CI.

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
