# Round 3 research roadmap

Status: 2026-11-11. This is a research roadmap, not a release promise or a list of GitHub issues. The original research was checked against `nightly` before round 3 began. The progress below describes `nightly` after PR #189 was merged (`05e6c67`), with the nightly CI run on that commit passing all five jobs. The repository history was rewritten later the same day, so any old commit hashes in earlier notes no longer resolve.

Tekto keeps its own scene tree and stable ids, first-class Rust and TSX APIs, agent-first tools, lean default builds, and a plain default look. Sizes are rough: **S** is a few days, **M** one to two weeks, and **L** larger or cross-platform.

## Round 3 shipped

All eight extension slices below are merged into `nightly`, each with headless tests and a nightly CI run:

| Item | PR |
| --- | --- |
| Animation: colour tweens, named easings, repeat | #191 |
| Native permission allow-list | #192 |
| Quiet design tokens | #193 |
| Responsive axis and collapsing split pane | #195 |
| Pointer capture in the tree | #196 |
| Keyed lazy rows | #197 |
| Generated native typings and drift test | #198 |
| Versioned QuickJS bytecode | #199 |
| Browser parity harness (manual) | #200, #201 |
| MCP overload flake fix | #194 |

Try the desktop demo: `cargo run -p round3-demo`. It sets tinted tokens before the widgets exist, runs a repeating colour tween on a bar, and has a split pane that collapses when the window is narrower than 760 px plus a slider whose drag survives leaving its bounds.

## Status of the last three items

1. **Text selection and accessibility.** Multiline selection, composition, and text-run geometry are covered by headless tests, a real Windows window was exercised at 125% scale, and the Notes APK ran on an x86_64 emulator. What is still unproven is a real IME candidate window: the desktop route handles `WindowEvent::Ime`, but no test drives an actual IME layout. OS accessibility smoke tests remain opt-in.
2. **Context menus, tabs, and typed agent actions.** WASM tests cover menu reopening, retained tab callbacks, and conditional panel lifecycles with their owners. `js_owner::tests::typed_agent_actions_reach_retained_tsx_and_reject_unrealized_nodes` now drives `SemanticAction` against a retained QuickJS app and checks that an unattached node is neither found nor actionable. Browser and Safari interaction is still manual.
3. **Generated-app builds.** `scripts/test-generated-apps.js` scaffolds an app and runs install, typecheck, and both builds with npm, pnpm, and bun; all three pass on Windows, and the nightly/manual CI job (`Generated app builds`) has run it successfully on the merge commit. It has not been run against a published package.

Navigation, tokens, right-to-left support, lazy lists, tables, charts and native extras are later work.

## Implemented foundations

The current tree has CPU and Vello rounded rectangles, borders and lines; an overlay layer; menus, select, dialogs and context menus; switch, radio, slider, progress and tabs with bound panels; multiline text selection with IME composition; images with a per-image cache; colour and named-easing transitions with repeat; shared design tokens; a responsive split pane; keyed lazy rows; a JS native permission allow-list; generated native typings; versioned QuickJS bytecode; web callbacks and keyed reload; the `tekto-test` helper; and a shared bundler with generated-app checks. Headless tests cover these slices. `tekto-tree` keeps the scene and core actions in `lib.rs` and the overlay, tab, field, animation, store and scroll logic in their own modules; `lib.rs` is 2,543 lines after the split (it was 3,235 before). See the entries below for remaining work and validation limits.

The final tree was checked on Windows with rustc 1.99.1: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo check -p tekto-wasm --target wasm32-unknown-unknown`, and `npm test` with esbuild (76 passed, 0 skipped) all passed. `cargo test -p tekto --features gpu --lib gpu_ -- --include-ignored` and the ignored GPU image test passed with a real adapter. The Notes window ran at 125% scale, the CLI tree, screenshot and live patch worked against it, the TSX showcase accepted typed text and a select interaction, the Windows clipboard and file probe passed, the Notes APK ran on an x86_64 emulator with `tekto tree` over `adb forward`, and the browser host painted Notes in headless Chrome. Nightly CI on the merge commit then passed Linux check, Windows compile, Android compile, WASM and QuickJS checks, and Generated app builds. The manual checks are one-off, not PR CI; Node/WASM tests do not replace every browser, and a headless GPU check does not prove every driver or device.

## New crates

### `tekto-test`

**Implemented:** Semantic lookup and actions against the real AccessKit/scene tree, deterministic waiting, optional unique test keys, CPU PNG comparison and diff artifacts. An optional QuickJS host retains callbacks. See [headless tests](../headless-tests.md) and `crates/tekto-test`.

**Remaining:** Keep Rust and TSX coverage current as widgets and actions grow; verify the optional QuickJS path when it changes. The helper does not replace real OS accessibility tests.

### `tekto-image`

**Implemented:** Bounded app-supplied RGBA pixels, contain/cover, clipping, Rust and TSX APIs, image semantics and decorative state, CPU and Vello paint, and opt-in static PNG decoding. Tests cover CPU pixels, WASM and QuickJS host paths, accessibility and a headless GPU pixel. See [getting started](../getting-started.md) and `crates/tekto/tests/image.rs`.

**Remaining:** Pixel data is a runtime resource and is not restored by scene JSON. Test a wider range of PNG variants and browser/WebGPU drivers. The CPU and GPU paths now cache one premultiplied or converted image keyed by `Image::id`, so a static image is not copied every frame; alternating many images still converts each one again. JPEG, WebP, network loading, animated images and SVG remain separate later decisions. PNG uses the existing MIT OR Apache-2.0 `png` crate behind `image-png`.

### `tekto-navigation` and `@tektogui/navigation`

- **What it does:** A small push/pop screen stack with explicit route keys. The current route is visible to agents. Back returns focus to the previous screen. Node ids stay monotonic; they are not reused when a screen is rebuilt.
- **Why:** SwiftUI and Compose use state-driven navigation without forcing a large router DSL. Tekto has no stack or back model yet.
- **Priority:** medium. **Size:** M.
- **Crates it touches:** new `tekto-navigation`, a small JS package `@tektogui/navigation`, plus light hooks in the tree, platform back action, and JSX.
- **Design notes:** One documented model, two language faces (Rust and TSX). Route keys name destinations; they are not NodeIds. No URL sync, no deep links, and no transition animations in the first slice.
- **DX fit:** Agents can inspect the current route and trigger back. Optional crate, not in the default app graph. Works with pnpm, bun, and npm. Same quiet widgets as the rest of the app.
- **Acceptance:** Tests cover push, pop, back at the root, a removed route, and focus restore. Rust and TSX use the same operations.
- **Licence:** Own MIT code. No framework code is copied. SwiftUI and Compose are ideas only.

### `tekto-i18n` and `@tektogui/i18n` (or a first slice in existing crates)

- **What it does:** Explicit locale, string lookup with fallback, interpolation, app-supplied plural variants, and logical start/end layout so right-to-left screens work. Accessible names stay in the selected language.
- **Why:** Flutter and Compose have resource and locale APIs. Parley can shape text, but that is not a translation system. Hard-coded left/right will get in the way as more widgets appear.
- **Priority:** medium. **Size:** M.
- **Crates it touches:** Start in `tekto-tree`, `tekto-layout`, `tekto-text`, `tekto-a11y`, and `tekto-js`. Split out `tekto-i18n` only if the catalog grows.
- **Design notes:** No bundled CLDR database in the first slice. Do not claim complete plural rules from hand-written data. Test RTL with an app-supplied fallback font. Full bidi editing polish is later.
- **DX fit:** Rust and TSX take ordinary data, not a new DSL. Packages work with pnpm, bun, and npm. No extra default dependencies.
- **Acceptance:** Missing keys fall back clearly. An RTL layout, mixed numbers, and focus order are tested headlessly. AccessKit language and direction metadata are present.
- **Licence:** Own MIT code. Any later locale data pack needs its own licence review (ICU transitives already exist under the text stack; do not add a large new database without checking).

### `tekto-data-grid`

- **What it does:** A read-only table with labelled headers, rows and cells, a bounded viewport, keyboard row movement, and optional single-row selection. Agents can scroll to a row and then inspect the realized cells.
- **Why:** Flutter, MAUI, Avalonia, and Qt all have data tables. Admin, file, and business apps need this. A table is also a good accessible fallback for a chart.
- **Priority:** medium. **Size:** L.
- **Crates it touches:** new optional `tekto-data-grid`, plus `tekto-tree`, `tekto-layout`, `tekto-widgets`, `tekto-a11y`, and HostOps.
- **Design notes:** Reuse list virtualization ideas. Application row keys are not NodeIds. Ids are never recycled. Do not invent clickable ids for rows that are not realized. No spreadsheet editing, frozen columns, or server query in the first slice.
- **DX fit:** Opt-in crate, no extra default deps. Same ids and AccessKit roles for Rust, TSX, and MCP. Quiet, plain cells.
- **Acceptance:** Empty state, a large row count with a bounded realized set, keyboard movement, scroll-to-row, selection, and semantic roles are tested headlessly.
- **Licence:** Own MIT code. Do not take a third-party grid framework.

### `tekto-charts`

- **What it does:** Simple bar and line charts with a title, axis labels, units, a short text summary, and a separately navigable list or table of values. Default paint stays quiet and monochrome.
- **Why:** Dashboards in Flutter, Avalonia, and many web kits hide data in pixels. Agents and screen readers need the numbers, not only a picture.
- **Priority:** medium. **Size:** M.
- **Crates it touches:** new optional `tekto-charts`, existing render primitives, and `tekto-a11y`. A table fallback may use `tekto-data-grid` once that exists.
- **Design notes:** Interactive point selection only if real nodes and actions exist. No live streaming, zoom, 3D, or maps.
- **DX fit:** Optional crate, no chart library. Agents get semantic values and can still take a screenshot. Quiet default look.
- **Acceptance:** A bar and a line chart expose title, summary, and named values. Headless CPU snapshots exist. A decorative-only chart is rejected: data must be reachable in the tree.
- **Licence:** Own MIT code. No external chart crate.

### Later or optional crates

These are real ideas, but they should wait.

- **`tekto-svg`:** Simple SVG icons, rasterized once per size and cached. Medium, M. Opt-in only. Licence of `resvg` / `usvg` must be confirmed before any dependency; if it is not MIT, Apache, BSD, or zlib, write a tiny path-only subset on tiny-skia (BSD-3) instead. No filters, SVG text, or animation in the first slice.
- **`tekto-update`:** Check a signed JSON manifest (version, URL, SHA-256, signature), download, verify, and hand the file to the user. Low–medium, M. Modelled on Tauri and Electrobun updaters (ideas only). Confirm `ed25519-dalek` (BSD-3) and `sha2` (MIT OR Apache-2.0) before adding them. No in-place patching, delta updates, or store submission.
- **`tekto-webview`:** An opt-in widget for rare cases such as sign-in pages. Low, L, **deferred**. System web views on Windows and macOS may be possible later, but wry/WebKitGTK on Linux pulls extra GTK and is against the "no extra GTK" rule. Never make this the default renderer, never bundle Chromium, and never claim the page's DOM shares Tekto ids.
- **`tekto-inspect`:** Only if the in-window overlay grows past a small platform feature. Prefer extending `tekto-mcp` and `tekto-platform` first (see below).

## New packages

### `@tektogui/build`

**Implemented:** The helper and `tekto-build` CLI generate a web ES module or desktop QuickJS IIFE from a TSX entry. Generated apps have `build:web` and `build:desktop` scripts plus `esbuild` as a dev dependency, so both formats build after one install. `scripts/test-generated-apps.js` scaffolds an app and runs install, typecheck and both builds for every available package manager; npm, pnpm and bun each passed on Windows. Generated apps carry a `pnpm-workspace.yaml` that lets pnpm 10+ run esbuild's build script, and Windows shim discovery now handles both global and local `.bin` layouts, including pnpm's nested store. The web dev server uses the helper; Node tests cover build, reload and watcher isolation.

**Remaining:** Desktop dev and the JavaScript helper share wrapper templates and compiler flags; desktop still invokes esbuild directly (the shared flag list is already used by both). The generated-app script runs on the nightly/manual CI job and passed there on the merge commit, but not on PRs, and it has not been run against a published package.

### Keyed JSX helpers (`<For>`, `<Show>`)

**Implemented:** `<For>` retains node ids and callbacks across keyed reorder, inserts and removals; `<Show>` retains its active branch until truthiness changes. Types and WASM host tests are present. Keys are application keys, not node ids.

**Remaining:** Exercise realistic TSX bundles and the retained QuickJS path, including update and cleanup failures. A `<Switch>` helper remains optional. Lazy data lists are a separate task.

### Later packages

- **`create-tektogui` typecheck template:** Generated apps now include `tsconfig.json`, local package links and a `typecheck` script. `scripts/test-generated-apps.js` runs that typecheck with npm, pnpm and bun, and the nightly CI job does the same. JSX intrinsic declarations are generated from `components.json` and checked for drift. The scaffold test typechecks a generated project when workspace TypeScript is installed, and its full `cargo check` is opt-in with `TEKTO_SCAFFOLD_CHECK=1`. TypeScript is Apache-2.0.
- **`@tektogui/solid`:** An experimental Solid renderer on HostOp, as a peer dependency, not a replacement for `@tektogui/core`. Low, M–L. Solid is MIT. Do not copy code from Wabou or similar projects until their licences are checked. Optional only.
- **`@tektogui/mcp-apps` or `@tektogui/a2ui`:** Adapters for [MCP Apps](https://apps.extensions.modelcontextprotocol.io/api/) or [A2UI](https://github.com/a2ui-project/a2ui) (Apache-2.0), only if a real consumer exists. Low, M. Do not embed a browser in the native renderer. Check the MCP Apps SDK licence before adding it.

## Extensions to existing crates and packages

### Paint primitives and overlay layer

**Implemented:** Rounded fills, borders and lines in the renderer trait and CPU/Vello backends; style fields in scene dumps; and an anchored overlay that paints and hit-tests above the base tree. Headless tests cover pixels, stacking, dismissal, blocked background clicks and accessibility. Existing renderer trait defaults preserve custom backends.

**GPU/CPU parity:** With the `gpu` feature, tests paint the same tree on both renderers and fail if more than 3% of pixels differ noticeably. They cover each primitive (fills, rounded fills, borders, lines, nested clips, text width, images) at scales 1.0 and 1.25, plus tabs, fields, selection, IME composition, controls, a list, an open menu, a modal dialog and an open select. They found two GPU bugs, both fixed: scaled images were smoothed instead of sampled nearest, and borders ignored the display scale. They ran on one NVIDIA adapter and skip when none exists.

**Remaining:** Continue checking GPU output across other drivers, Android and browser WebGPU. Collision flipping, overlay arrows and motion were outside the first slice.

### Menus, select and modal dialog

**Implemented:** Popup menu, single-select and named modal dialog with overlay placement, keyboard actions, focus restoration and background blocking. Reusable context menus bind in Rust and TSX and open on desktop and browser right-click. Headless QuickJS and WASM tests cover reopening and callbacks.

**Remaining:** A headless test covers arrow keys skipping disabled items, a newly disabled focused item and Escape restoring focus; the same checks on the web and Android hosts are still open. Nested submenus, native menu bars and multi-select are later work.

### Radio, switch, slider, progress and tabs

**Implemented:** Radio groups, switch, slider, determinate progress and tab lists have Rust and TSX forms, accessibility roles and headless tests. Tabs use roving focus, arrow keys and a selected indicator. Slider supports pointer dragging and the agent set-value action. Tabs associate panels in Rust and TSX; inactive panel state is retained while it is excluded from layout, focus, hit testing and accessibility. Arrow selection delivers retained callbacks in QuickJS and WASM host tests.

**Remaining:** Conditional panel removal and recreation have a WASM regression test, and headless tests now cover removing the selected tab, adding tabs and panels later, double binding and removing a panel directly. Retained TSX apps with many dynamic panels are still untested. Check remaining disabled-state and callback parity on each host; general pointer capture and agent drag are separate items below.

### Text selection and multi-line field

**Implemented:** Single-line selection, replacement and grapheme-aware movement/deletion; desktop mouse and keyboard selection and native clipboard; browser mouse and keyboard selection and clipboard events. IME composition text has an underline and selected-subrange highlight. Single-line fields expose AccessKit text runs and grapheme-aware selection metadata when their text and IDs can be represented; multiline fields expose one run per line, cross-line selection, and per-line bounds with per-character positions and widths (right-to-left lines report their direction and offset from the right edge). Layout stores one box per grapheme for text up to 4000 graphemes; longer text falls back to the field bounds. This uses the existing MIT OR Apache-2.0 `unicode-segmentation` dependency. A plain multiline field supports wrapping, vertical movement and caret scrolling. Keyboard and IME text now mark layout dirty (so the caret follows the text instead of staying where the last layout put it), the caret blinks at a 530 ms half-period from the tree clock, typing or moving focus turns it solid and restarts the clock, and the desktop loop wakes at the next blink edge instead of every frame. Headless tests cover selection, graphemes, emoji, IME commit and cancellation, disabled fields, paint, caret position after typing/backspace, and blink reset on typing and blur.

**Remaining — high priority:** Browser touch selection. Browser IME has host wiring and needs verification in a real browser. Desktop IME composition has not been driven through a real IME layout: the Windows pass typed committed text through the dev server, and the clipboard/file probe covers the native API rather than field copy and paste. The Android `EditText` overlay has not been exercised because Notes has no text field. Bidirectional lines are covered by a headless Hebrew test only; mixed-direction lines are reported as one run, so per-run direction for mixed text and visual (not logical) caret movement still need work. IME composition is now tested to cancel on focus change, field disable and tab-panel hide.

### Web TSX callbacks and keyed reload

**Implemented:** WASM host delivers button clicks and field input, removes stale callbacks, and supports keyed signal reload state with reset reporting. Node/WASM tests exercise the retained paths; QuickJS remains optional.

**Remaining:** Headless Chrome painted the Notes host page, but clicks, field input and reload have not been exercised in a real browser. Unkeyed state still resets; this is not whole-heap preservation.

### Test helpers, snapshots, and inspect (related slices)

These three work well together, and can share the `tekto-test` crate above.

**Semantic snapshots, actions and inspect**

**Implemented:** Scene and AccessKit dumps expose ids and state; `tekto-test` finds by role/name or unique test key. CLI `inspect` and MCP `ui.inspect` report ancestry, bounds, clipping and semantics. An inspector page and headless diagnostics tests are present. A versioned semantic snapshot and exact role/name search use the realized AccessKit tree and return duplicate matches. Typed focus, activation, toggle-state and text/slider value actions use the shared tree and retained callback hooks; existing click, key, type and scroll actions remain available. `js_owner::tests::typed_agent_actions_reach_retained_tsx_and_reject_unrealized_nodes` drives `SemanticAction` against a retained QuickJS app, checks that activation and text input reach its callbacks, and rejects a node that exists in JS but was never attached.

**Remaining:** Callback failures during a typed action and overlay restrictions are covered by the MCP unit tests but not yet by a retained-TSX end-to-end test. A debug overlay is optional; the current inspector page is not evidence of an in-window overlay.

**Linux AT-SPI smoke test**

- **What it does:** Wire AccessKit actions back through the shared action path, then run a small opt-in Linux test: read a button, field, and checkbox through AT-SPI, activate or toggle, type, and check focus events.
- **Why:** An AccessKit JSON dump is not the same as a real screen reader or OS automation client. Flutter documents platform accessibility testing for this reason.
- **Priority:** high for a Linux first slice, larger if every platform is included. **Size:** M (Linux first).
- **Crates it touches:** `tekto-a11y`, `tekto-platform`. Stays behind the existing `a11y-os` feature, which must remain off by default.
- **Design notes:** Publish a clear support matrix for UIA, macOS, and Android rather than implying the dump is enough. Adapter mocks can run in CI; a live desktop session may stay manual.
- **DX fit:** Fast default builds unchanged. Agent path and OS path share the same actions.
- **Acceptance:** Opt-in Linux smoke covers role, name, value, activate/toggle, type, and focus. CI does not require a desktop session unless it is explicitly marked.
- **Licence:** AccessKit is already used (MIT OR Apache-2.0). No new default dependency.

### Quiet design tokens

**Implemented:** `tekto-tree` has a `Tokens` set for `page`, `ink`, `banner`, `onBanner`, `field`, `border`, `focus`, `gap`, `padding`, `bodySize`, and `titleSize`. `Tokens::default()` reproduces the palette and spacing the widgets already used, so the blessed Notes PNG and every widget snapshot are unchanged. New nodes read the tree's tokens in `Tree::add`, so widgets no longer re-apply hard-coded defaults; `window`, `banner`, `title_on_banner`, and `list_item` read the same set, and the renderer falls back to `tree.tokens().ink` / `page` / `focus` instead of constants. An app overrides with `Tree::set_tokens` (Rust) or the `setTokens` HostOp: `tokensOp({ page, gap, ... })` in `@tektogui/core` and `tekto.setTokens(json)` in QuickJS. `TokenPatch` takes hex colours and finite sizes, accepts snake-case aliases, and rejects unknown fields or bad values without changing the set. Tests cover the default values, patch parsing and rejection, the op applying to later nodes, and a painted window using an overridden `page`.

**Remaining:** Tokens affect nodes created afterwards; there is no live re-theme of an existing tree, and generated apps do not ship a token file. Text scale from the host (150%) and high-contrast are not wired. `Color` stays sRGB; no colour-space conversion.

### Keyed lazy lists

**Implemented (first slice):** `KeyedList` in `tekto-widgets` owns a scroll viewport, a row column, and top/bottom spacers sized from the item count and row height. `sync(tree, keys, overscan, build)` removes rows whose keys left the window, creates rows for keys that entered it, keeps the node id of a key that stayed visible, and rejects duplicate keys. `scroll_to_key` jumps to a key's offset so a host can then `sync` and materialize it. Rows are inserted in key order with the new `Tree::insert_child_at`, and new materializations get fresh monotonic ids. Tests cover a 1000-key list keeping fewer than 20 row nodes, id retention across a small scroll, dropping and recreating rows after a far scroll, scroll-to-key materialization, duplicate keys, and bad geometry.

**Remaining:** This is app-driven: the host must call `sync` after a scroll, and there is no automatic materialize on an agent scroll (an agent can call `scroll_to_key` through the app or set the slider-style scroll then re-sync). No variable-height estimate, no grid, no network paging, no TSX binding, and no special focus rule when a focused row leaves the window. The existing `list` widget still keeps detached row nodes, so `KeyedList` is the opt-in path.

### Responsive layout helpers

**Implemented:** `responsive_axis(viewport_width, breakpoint)` returns `Axis::Row` or `Axis::Column`, and `apply_axis` switches an existing container. `split_pane(tree, parent, collapse_below, gap)` builds a row of two equal panes (`grow` plus a zero width basis), marks the secondary pane with `collapse_below`, and returns both ids. Layout compares `collapse_below` against the viewport width, sets `Node::collapsed`, and `Tree::is_visible` excludes that subtree, so it drops out of layout, paint, hit testing, focus, and AccessKit while keeping its nodes and state. If focus was inside a pane that collapses, layout moves it to the nearest visible ancestor. `grow` and `collapseBelow` are HostOp props too, so TSX groups can build the same layout; `components.json` and the generated JSX declarations include them. Headless tests cover equal pane widths, collapse and restore at a breakpoint, focus rescue, the axis helper, and rejected breakpoints.

**Remaining:** Collapse is driven by the viewport width, not the pane's own content box, so a split pane nested inside a narrow column does not re-evaluate its parent's width. There is no animation on collapse, no draggable divider, and no stored per-breakpoint layout. Only `split_pane` uses `grow` today.

### Pointer capture and in-app drag

**Implemented:** Pointer capture is part of the scene tree: `Tree::capture_pointer` (attached, enabled, visible only), `release_pointer`, and `pointer_capture`. It is dropped automatically when the captured node is removed or when focus moves to a different node, and `ui.inspect` reports `pointer_captured`. The desktop pointer router no longer keeps its own capture field; it captures the slider on press, uses the tree's capture for motion (so leaving the bounds still tracks), and releases it on end or cancel. The existing slider drag, scroll-conflict, and cancelled-touch tests still pass, plus tree-level tests for release on remove, focus change, disabled nodes, and detached nodes.

**Remaining:** No `ui.drag { from, to }` agent route yet; `ui.action` set-value covers sliders programmatically. No list-reorder drag or drag events for app callbacks, no pinch, no cross-window drag, and no gesture DSL. Text selection still uses its own `text_capture` rather than the shared capture.

### JS native permission list

**Implemented:** `tekto-js` has a flat capability set — `clipboard`, `files`, `notifications`, `tray`, `shortcuts` — as `NativeCapability`, with `NativePermissions` storing the allowed set. `TreeHost` carries permissions and honours them in `Host::allow_native`; `tekto.applyNativeOp` checks before the platform call and throws `native capability `<name>` is not allowed for this app; the host must opt in with NativePermissions`. `NativePermissions::from_names(["clipboard", "files"])` parses configuration, `all` expands to every family, an unknown name lists the accepted ones, and `deny_all` / `only` / `allow` / `deny` cover the rest. The default is `allow_all` so existing apps keep working, and an embedding that runs agent-written TSX narrows it explicitly; both choices are documented on the type. Tests cover name parsing and rejection, every op mapping to one family, `TreeHost` gating with and without QuickJS, and the JS error text.

**Remaining:** No `tekto.permissions.json` is read by the CLI or desktop host yet, so narrowing is a Rust API call today. Checks are per family, not per path or per call, and there are no permission prompts. Wiring a generated-app template with a narrow default is a later slice.

### Typed native API and template types

**Implemented:** `crates/tekto-native/native-ops.json` is the one list: each op has a JS name, argument types, an optional result type, and an example object the Rust side can decode. `scripts/generate-native-types.mjs` writes `packages/core/src/native-ops.d.ts` (`NativeOp` union) and `npm run generate:native-types` regenerates it; a `--check` test in `packages/core/native-types.test.js` fails when the file is stale. `@tektogui/core` re-exports that union from its declarations, so `nativeOpJson` rejects a misspelled op or wrong argument type. `NativeOp::name()` is an exhaustive Rust match, and `tekto-native` has a drift test that decodes every JSON example into the real enum, checks the name and serialized tag, and fails if an op disappears from the list. The scaffolder copies the generated declarations into a generated app, and its typecheck test passed. The template already had the `tsconfig`/`typecheck` script and generated JSX prop types. Build-time only, no runtime cost.

**Remaining:** Rust stubs are not generated from the list; the enum and its handlers stay hand-written, and the drift test is what keeps the two sides aligned. The generator has no result-type checking beyond the declared string, and there is no per-platform capability table in the list.

### QuickJS bytecode in release builds

**Implemented:** `tekto-js` wraps rquickjs 0.14 module bytecode in a small envelope: magic `TEKTOQBC`, a format number, and an engine tag (`rquickjs-0.14`). `compile_bytecode(source, name)` compiles with `Module::declare` and writes with `strip_source`, so the source text is not embedded; `JsSession::eval_bytecode` parses the header, loads with `Module::load`, and runs the module. A stale format, foreign engine tag, truncation, or corrupt payload returns an error whose text says to rebuild from source, and the caller falls back to `JsSession::eval` with the original bundle. `tekto bytecode <in.js> <out.qbc>` produces the file from a CLI built with `--features quickjs`. Tests cover the header round trip and every rejection, a compiled script running from bytecode, the source marker being absent from the payload, fallback to source after a foreign tag, and a corrupt payload failing inside QuickJS.

**Remaining:** This is opt-in and crate/CLI level. No Rust app or Android release path loads a `.qbc` yet, dev reload still bundles and evaluates source, and the engine tag is a hand-maintained constant that must be bumped when rquickjs or QuickJS-ng changes. Bytecode is compiled as a module, so a bundle must not depend on script-mode top-level `this`. No obfuscation was added.

### Animation upgrades (small)

**Implemented:** `StyleTransition` now tweens fill and text colour in addition to opacity and bounds, takes `easing` (`smoothstep`, `linear`, `ease-in`, `ease-out`), and supports `repeat`. Repeat ping-pongs: when the last channel finishes it swaps endpoints and runs back, one cycle per `advance`, so a spinner or a breathing pulse needs no timeline. Reduced motion still snaps once and never repeats. The same fields cross the HostOp boundary: `transitionOp(id, { fill, color, easing, repeat })` in `@tektogui/core`, the QuickJS `tekto.transition` bridge, and the Rust API. Headless tests cover colour midpoints, each easing, ping-pong over several cycles, reduced motion, and bad colour or easing strings.

**Remaining:** No rotation channel, so a spinner is a pulse or a back-and-forth slide rather than a rotating arc. No keyframes, springs, or timelines, and existing tweens still use the single 180 ms default. A colour fade and a repeating spinner honour reduced motion and the test clock; the desktop window still advances from its own clock.

### Native extras

Each of these is a separate slice in `tekto-native`, with `Unsupported` / `Unavailable` / cancel results, mocks for tests, and no silent external actions.

- **Android shared-document picker (SAF).** Medium, M. Return an opaque document handle; do not pretend a content URI is a normal file path. Expo's file API is the idea, not the code. First slice: open and read only, no write, no persistent grants. Needs an emulator or device after the contract tests. Own MIT code plus Android platform APIs. No new crate and no GTK.
- **Packaging manifest.** Medium, M. Extend `tekto-cli` with app name, identifier, version, and icon path. Validate it and generate a Linux desktop entry and Android metadata for the workflows we already have. Do not claim an unsigned build is store-ready. Own MIT code. See [packaging](../packaging.md) and [signing](../signing.md).
- **Deep links and a share sheet.** Medium, S–M per platform. One host at a time, with injectable mocks so agents can observe the result. Native window menus, OS drag-and-drop, and multi-window are later and larger.
- **Read-only rich text spans.** Medium, later, M. After the multi-line plain field works, a `Text` node may hold bold, colour, size, and links. Parley already supports spans. This is not a rich editor. An extra bold font file should be opt-in because of size.

## Host parity checks (manual)

- **Browser:** `packages/wasm/parity.html` builds a tab list, a button with a context menu, and a text field on the WASM canvas. `scripts/browser-parity.mjs` serves the page, launches headless Chrome, drives it over the DevTools protocol with real `PointerEvent` (mouse and touch), `MouseEvent("contextmenu")`, `KeyboardEvent`, and a `textarea` input, and checks the callback log. On this machine all six steps pass: button click, context-menu item, Escape closing the overlay, tab selection, a touch tap on a tab, and field input. `npm run test:browser` runs it. It needs Chrome and a wasm build, so it is not in CI.
- **GPU:** the CPU/Vello parity tests and the GPU image test pass on the one adapter this machine exposes. Other drivers, Android GPUs, and browser WebGPU are unverified.
- **Android:** the Notes APK builds, installs, paints on the x86_64 AVD, and answers `tekto tree` over `adb forward`. The NativeActivity text field and IME overlay still need a live type-into-field check because the Notes screen has no field.
- **Desktop IME:** the Windows route handles `WindowEvent::Ime`, but no test drives a real IME layout; the Windows pass typed committed text only.

### Custom app bar

**Implemented:** `app_bar(tree, parent, title)` builds a banner-coloured in-window bar and returns `AppBar { root, leading, title, actions }`: a leading slot, a title that takes the free space, and a right-hand actions group. `app_bar_action` and `app_bar_leading_button` add quiet buttons (no fill, `onBanner` text, focus radius). The bar follows the tree tokens, so a tinted `set_tokens` changes it. TSX has `appBar(title, actions)` and `appBarAction(label, onClick)`; `fill: "none"` (also accepted by a patch) clears a button fill so it sits on the bar. Tests cover structure, leading-versus-actions placement, quiet buttons, focus on click, and token colours.

`WindowOptions.decorations = false` removes the OS title bar. A node with `drag_window` starts `Window::drag_window()` on a left press, and the walk from the hit node stops at an interactive widget, so a button inside the bar still clicks. `app_bar_window_controls` adds minimise, maximise and close buttons that carry `window_control`; the desktop host applies them on mouse click or keyboard activation, and close exits the event loop. The round 3 demo runs undecorated with the bar flush to the window edge (drag it, use the three buttons). Tests cover the drag-region walk, the three control values, and patching `windowControl` /`dragWindow` (including an unknown value).

Edge resizing is manual for the undecorated window: a 6 px band on each edge (corners included) starts a `ResizeSession`, the mouse is captured on Windows so moves keep arriving outside the window, and the target rect is computed from screen-space cursor deltas against the rect at press time. On Windows the screen position comes from `GetCursorPos`, so moving the window under a captured pointer cannot feed back into the delta; the rounded rect is compared with the last one sent so duplicate moves are dropped. On Windows one `SetWindowPos` call applies position and size together and is skipped when the rounded rect did not change, so the window tracks the pointer instead of drifting and a resize frame never paints at a stale rect; other platforms fall back to `set_outer_position` plus `request_inner_size`. Only the redraw path lays out and resizes the painter — the `Resized` event just requests a redraw, so a live resize lays out once per frame instead of twice per size event. The minimum is 240×160 logical and the cursor changes to the matching resize arrow. Double-clicking the drag region toggles maximise. On release after a bar drag, the cursor is checked against the current monitor: the top edge maximises, a side edge snaps to that half, and a corner snaps to that quarter. Measured with real mouse events on Windows: right +100 px grew 900→1000, bottom +80 grew 640→720, left +100 moved x by 100 and shrank 1000→900, double-click set and cleared `IsZoomed`, and a drag to the left edge snapped to a 768-wide half. A screen capture taken while the button was still held showed the tree reflowed at the new width. Unit tests cover the edge and corner direction map.

**Remaining:** The Windows 11 snap-layouts flyout is an OS caption-button behaviour and is not reproduced; snapping uses the monitor rectangle, so a taskbar-aware work area for the quarter/height cases is still approximate. The TSX props for `dragWindow` and `windowControl` are not exposed yet (only the Rust API and the app bar helpers), so a TSX app cannot opt into undecorated chrome by itself. No drop shadow, rounded window corners, or overflow menu.

## Rendering performance

Animations repaint on the window clock (16 ms). A frame used to re-run Taffy and Parley every time even when only a colour or opacity tween was running. `Tree` now carries a `layout_dirty` flag: structure, geometry, text, focus, scroll, overlays and tab binding mark it; `get_mut` marks it for app and widget edits; a layout pass clears it; and a position or size tween marks it from `advance`. The desktop loop lays out only when the viewport changed or the flag is set, so a colour or opacity tween paints without relayout. Android still lays out every frame because insets can change.

The GPU painter now presents **straight to a wgpu window surface** (`GpuRenderer::new_for_window` and `present_window`), sharing the Vello device and queue. The old path rendered Vello to an offscreen texture, copied it back to the CPU, converted it to softbuffer pixels, and blitted through GDI; measured frame time on the round 3 demo was about 33 ms, of which roughly 29 ms was that present. With the surface path it is about 16 ms and the softbuffer readback remains only as a fallback when a surface cannot be created. A window resize applies at most one rect per painted frame (`ResizeSession::pending`), so input events cannot queue up faster than the window can present.

`TEKTO_FRAME_LOG=1` on a debug build prints one line per frame with layout, paint, and present timings.

**Remaining:** Text shaping still runs whenever layout does, and a resize frame that changes the wrap width can spend 40–60 ms in layout, which is the remaining source of spikes. There is no partial damage region: each frame repaints the whole scene.

## Not doing

These were looked at and rejected, or deferred on purpose.

- Do not wrap Flutter, Compose, React Native, Tauri, Electron, Wails, Lynx, Avalonia, MAUI, Uno, Qt, Dioxus Native, Blitz, or Masonry. Keep Tekto's own tree, ids, renderer trait, and MCP surface.
- Do not depend on or copy **Slint**. Its licence is GPL-3.0, or a royalty-free licence that requires visible credit and excludes embedded use, or a paid licence. That does not fit a MIT toolkit.
- Do not copy **Zed the app** (GPL). The `gpui` crate is Apache-2.0, but wrapping it would pull a large tree and a different model.
- Do not bundle Chromium, CEF, or an HTML/CSS engine (including Blitz / Stylo). Do not take Servo (MPL-2.0, not on the allowed list).
- Do not add wry / WebKitGTK on Linux just to get a web view (extra GTK). A web view, if ever added, stays an opt-in escape hatch and is not the default renderer.
- Do not replace the CPU/Vello renderer with Skia or Impeller, and do not put GPU, QuickJS, or OS accessibility in the default graph or on PR CI.
- Do not switch QuickJS for Hermes, V8, or JavaScriptCore unless a later profile shows JS is the bottleneck.
- Do not build a Material, Fluent, or Cupertino clone, or a large theme system. Small tokens are enough.
- Do not start with maps, video, audio, a spreadsheet, or a full rich-text editor.
- Do not replace semantic agent actions with screenshot-only clicking, and do not expose an unrestricted shell through the dev server.
- Do not copy code from experimental projects (Wabou, Kiln, and similar) until their licences are checked. Chili3D and other "ideas only" projects stay ideas only.
- New direct dependencies must be MIT, Apache-2.0, BSD, or zlib, with attribution if code is reused. Several research notes marked licences "from memory"; confirm each licence in the upstream repo before adding a crate.

Windows runtime checks, macOS and iOS work, PR #101, and merging `nightly` into `main` remain separate decisions.
