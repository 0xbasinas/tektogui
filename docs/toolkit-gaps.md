# Tekto and established GUI toolkits

Status: 2026-11-11. This is a snapshot of this worktree, not a release promise. Flutter and native toolkits have much broader production use, controls, platform integration, and testing. Tekto is early; a shared tree and a successful compile do not establish equivalent behavior on a device.

The integrated nightly includes Android app-private files and the Rust Notebook. macOS text clipboard and a testing handoff are also integrated, but [macOS catch-up (#58)](https://github.com/0xbasinas/tektogui/issues/58) remains open: the clipboard and desktop shell need Mac runtime checks, and other plugins remain unsupported. See the [macOS handoff](macos-handoff.md).

## Input and text

Tekto has Parley text, bundled Roboto, and a single-line field. On the shared Linux/Windows desktop path, IME preedit and commit are handled and the OS gets a caret position. Desktop pointer clicks place the caret at the nearest painted character. Agent `ui.click` and `Tree::click(id)` place it at the end. The WASM/browser `click_at` path is not covered by this change. Selection, copy and paste in the field, and marked-range highlighting are missing. A Windows IME session still needs a runtime check. `ui.type` inserts plain text; it does not simulate an IME.

Android uses a native `EditText` overlay and copies its string into the tree. That string can include unconfirmed composition; it is not a separate Tekto preedit. Live typing and portrait keyboard-inset checks still need a device or free emulator. This is narrower than the editing, international input, and text controls expected from Flutter or platform toolkits. See [agent guide](agent-guide.md) for the current input paths.

## Accessibility

The AccessKit tree shares stable ids with the agent tree and carries roles, names, bounds, focus, and field values. OS publication is optional (`a11y-os`), off by default. A tree dump alone does not prove screen reader behavior, focus navigation, text selection, or platform accessibility quality. Those need real assistive-technology checks on each supported host. See [architecture](architecture.md).

## Rendering and mobile feel

CPU painting is the default. The optional wgpu/Vello renderer paints offscreen and desktop presentation still uses softbuffer; it can fall back to CPU. Android GPU use also needs an explicit runtime opt-in and its drivers have not been proven here. WebGPU is optional and falls back to the CPU canvas blit; WebGL2 also uses that blit. This is not evidence of Flutter-level GPU coverage or performance across devices. See [architecture](architecture.md) and [build notes](dev-builds.md).

Touch scrolling tracks the finger and can coast, and Android layout accounts for reported safe areas and keyboard insets. There is no rubber-band bounce, and the live inset check remains open. Tekto has a small set of controls and quiet defaults, not the broad native control behavior, platform conventions, motion, and device coverage of established toolkits. Themes are later work.

## Packaging and platforms

Notes has an unsigned Linux tarball script and a documented unsigned Windows folder layout. Android has a Notes NativeActivity APK path; nightly CI checks Android and WASM compilation rather than packaging or running an emulator. [PR #129](https://github.com/0xbasinas/tektogui/pull/129) restored that compile matrix, and [nightly run 42919939722](https://github.com/0xbasinas/tektogui/actions/runs/42919939722) was green on Linux, Windows, Android, and WASM. Compilation does not establish device or WASM runtime behavior. There is no store-ready signing pipeline or macOS package, and there is no iOS target. See [packaging](packaging.md) and [signing](signing.md).

The iOS work in [#61](https://github.com/0xbasinas/tektogui/issues/61) and [#62](https://github.com/0xbasinas/tektogui/issues/62) needs a Mac and Xcode for implementation and validation. Distribution also needs the appropriate Apple signing and provisioning setup. This Linux worktree cannot validate iOS bindings or device behavior. Windows compile checks likewise do not replace a live window check ([#3](https://github.com/0xbasinas/tektogui/issues/3)) or a live file dialog check ([#45](https://github.com/0xbasinas/tektogui/issues/45)).

## Web

The WASM core and browser HostOp bridge run the Notes TSX sketch, with CPU canvas paint and optional WebGPU. This is a useful path, not a claim of browser polish equal to Flutter web or native HTML controls. Input, accessibility, browser integration, performance, and production deployment need broader real-browser validation. The web watcher fix in [PR #131](https://github.com/0xbasinas/tektogui/pull/131) is merged into nightly, and [#127](https://github.com/0xbasinas/tektogui/issues/127) is closed. The green nightly run does not establish WASM runtime coverage or coverage of all npm WASM tests. See [architecture](architecture.md) and [getting started](getting-started.md).

## Agent and app developer experience

Stable ids, the shared AccessKit/agent tree, screenshots, click and type actions, live patches, component schemas, and opt-in state recording give agents useful inspection tools. TSX hot reload remounts the app and resets signal state. Rust hot patching remains experimental; see [Rust hot reload](rust-hot-reload.md). Patches and headless checks do not replace testing a real user session; see the [agent guide](agent-guide.md).

Rust and TypeScript/JSX share the tree and HostOp direction, but their API and platform behavior are not yet fully equivalent. For example, Rust has a list helper that decides when to add scrolling; JSX composes a scroll explicitly. Native operations are implemented per platform and unsupported paths return errors. Android file reads and writes are confined to app-private storage; the Storage Access Framework and shared-document picking are unsupported. A selected dialog path does not give the TypeScript bridge unrestricted file access. The [Rust Notebook](../examples/notebook/README.md) uses synchronous tree handlers, reports dirty and save status, saves through a synced temporary file, and preserves invalid documents in recovery mode. It has single-line fields and is not evidence of equivalent TypeScript event handling or broad widget parity. Repeated platform checks are still needed to expose gaps.

The [final project audit (#104)](https://github.com/0xbasinas/tektogui/issues/104) will identify focused follow-up issues after this documentation work.

For comparison, see the official [Flutter accessibility overview](https://docs.flutter.dev/ui/accessibility), [Flutter deployment guides](https://docs.flutter.dev/deployment), and [Android text input guidance](https://developer.android.com/develop/ui/views/touch-and-input/keyboard-input). These links are references, not claims that every platform behaves alike.
