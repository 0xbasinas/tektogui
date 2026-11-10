# macOS handoff

This is a checklist for a Mac owner, not a claim that Notes runs on macOS. The macOS clipboard path and the desktop shell have not been run on a Mac. Record the macOS version, CPU architecture, Rust version, commands, and results when testing. Apple command line developer tools and Rust are needed.

On Linux, an optional compile-only check is `cargo check -p tekto-native --target aarch64-apple-darwin` and `cargo check -p notes --target aarch64-apple-darwin`. These need the Rust target and cached or downloadable Apple-target dependencies; they do not link or run a Mac app. On this Linux host the target is installed, but dependency downloads failed because DNS/network access was unavailable.

## Build and CPU window

From the repository root on a Mac, run:

```sh
rustup target list --installed
cargo check -p tekto-native
cargo check -p notes
cargo build -p notes
cargo run -p notes -- --headless --out /tmp/tekto-notes.png
cargo run -p notes
```

Inspect the PNG for missing or clipped text, colors and bounds. In the window check startup, redraw, resizing, high-DPI scaling, mouse clicks, typing and closing. The default window uses the CPU renderer. Note any crash or visual defect; a successful `cargo check` does not link or exercise a window. Also run `cargo test -p tekto --test headless_snapshot`; report any platform-dependent snapshot mismatch rather than blessing it automatically.

## Accessibility

Build and run with OS accessibility enabled (off by default):

```sh
cargo check -p tekto --features a11y-os
cargo run -p notes --features tekto/a11y-os
```

With VoiceOver or Accessibility Inspector, inspect the Notes window and check names, roles, bounds, focus, and navigation as the UI changes. The in-process AccessKit tree alone does not prove OS accessibility works.

## Native plugins

Rust uses `tekto::native()` and the existing traits; TypeScript uses the same `NativeOp` bridge. No new public API is needed. Run `cargo test -p tekto-native` and use a small caller to check both Rust and `apply_native_op` results. For clipboard, save any important clipboard contents first; this opt-in test overwrites them:

```sh
TEKTO_CLIPBOARD_LIVE=1 cargo test -p tekto-native macos::tests::live_clipboard_roundtrip -- --exact
```

Also check empty and Unicode text, non-text clipboard contents, and an unavailable session. `arboard` is MIT OR Apache-2.0 and uses its text-only configuration. Clipboard behavior has not been tested on macOS.

Current gaps, including with `tray` or `shortcuts` enabled:

- Files: sandboxed text and byte reads/writes return `Unsupported`; no macOS root or symlink-safe confinement is implemented.
- Open/save dialogs: `Unsupported`; cancellation, selected paths, and overwrite confirmation are untested.
- Notifications: `Unsupported`; authorization and delivery are unimplemented.
- Tray: `Unsupported`; icon, menu, and click handling are unimplemented.
- Global shortcuts: `Unsupported`; registration, permissions, and activation are unimplemented.
- Clipboard: text only; the implementation needs real-session checks, including error behavior and the TypeScript bridge.

Do not mark runtime acceptance complete until the Mac checks have been performed and their results recorded. No macOS CI job is added by this change.
