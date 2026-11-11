# Round 3 research roadmap

Status: 2026-11-11. This is a research roadmap, not a release promise or a list of GitHub issues. The original research was checked against `nightly` at `89726d8`; the progress below describes the **current working tree**, including uncommitted changes. It does not imply that changes have been merged or released.

Tekto keeps its own scene tree and stable ids, first-class Rust and TSX APIs, agent-first tools, lean default builds, and a plain default look. Sizes are rough: **S** is a few days, **M** one to two weeks, and **L** larger or cross-platform.

## Status of the last three items

1. **Text selection and accessibility.** Multiline selection, composition, and text-run geometry are covered by headless tests, a real Windows window was exercised at 125% scale, and the Notes APK ran on an x86_64 emulator. What is still unproven is a real IME candidate window: the desktop route handles `WindowEvent::Ime`, but no test drives an actual IME layout. OS accessibility smoke tests remain opt-in.
2. **Context menus, tabs, and typed agent actions.** WASM tests cover menu reopening, retained tab callbacks, and conditional panel lifecycles with their owners. `js_owner::tests::typed_agent_actions_reach_retained_tsx_and_reject_unrealized_nodes` now drives `SemanticAction` against a retained QuickJS app and checks that an unattached node is neither found nor actionable. Browser and Safari interaction is still manual.
3. **Generated-app builds.** `scripts/test-generated-apps.js` scaffolds an app and runs install, typecheck, and both builds with npm, pnpm, and bun; all three pass on Windows, and the nightly/manual CI job runs it. It has not been run against a published package.

Navigation, tokens, right-to-left support, lazy lists, tables, charts and native extras are later work.

## Implemented foundations

The current tree has CPU and Vello rounded rectangles, borders and lines; an overlay layer; menus, select and dialogs; switch, radio, slider and progress controls; web callbacks; keyed reload; and initial image support. Headless tests cover these slices. See the entries below for remaining work and validation limits.

The final tree was checked on Windows with rustc 1.99.1: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo check -p tekto-wasm --target wasm32-unknown-unknown`, and `npm test` with esbuild (76 passed, 0 skipped) all passed. `cargo test -p tekto --features gpu --lib gpu_ -- --include-ignored` and the ignored GPU image test passed with a real adapter. The Notes window ran at 125% scale, the CLI tree, screenshot and live patch worked against it, the Windows clipboard and file probe passed, the Notes APK ran on an x86_64 emulator with `tekto tree` over `adb forward`, and the browser host painted Notes in headless Chrome. These are one-off manual checks, not PR CI. Node/WASM tests do not replace every browser, and a headless GPU check does not prove every driver or device.

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

**Remaining:** Desktop dev and the JavaScript helper share wrapper templates and compiler flags; desktop still invokes esbuild directly (the shared flag list is already used by both). The generated-app script runs on the nightly/manual CI job, not on PRs, and it has not been run against a published package.

### Keyed JSX helpers (`<For>`, `<Show>`)

**Implemented:** `<For>` retains node ids and callbacks across keyed reorder, inserts and removals; `<Show>` retains its active branch until truthiness changes. Types and WASM host tests are present. Keys are application keys, not node ids.

**Remaining:** Exercise realistic TSX bundles and the retained QuickJS path, including update and cleanup failures. A `<Switch>` helper remains optional. Lazy data lists are a separate task.

### Later packages

- **`create-tektogui` typecheck template:** Generated apps now include `tsconfig.json`, local package links and a `typecheck` script. The generated template typechecked locally using npm, pnpm and bun. JSX intrinsic declarations are generated from `components.json` and checked for drift. The scaffold test typechecks a generated project when workspace TypeScript is installed. TypeScript is Apache-2.0.
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

**Implemented:** Single-line selection, replacement and grapheme-aware movement/deletion; desktop mouse and keyboard selection and native clipboard; browser mouse and keyboard selection and clipboard events. IME composition text has an underline and selected-subrange highlight. Single-line fields expose AccessKit text runs and grapheme-aware selection metadata when their text and IDs can be represented; multiline fields expose one run per line, cross-line selection, and per-line bounds with per-character positions and widths (right-to-left lines report their direction and offset from the right edge). Layout stores one box per grapheme for text up to 4000 graphemes; longer text falls back to the field bounds. This uses the existing MIT OR Apache-2.0 `unicode-segmentation` dependency. A plain multiline field supports wrapping, vertical movement and caret scrolling. Headless tests cover selection, graphemes, emoji, IME commit and cancellation, disabled fields and paint.

**Remaining — high priority:** Browser touch selection. Browser IME has host wiring and needs verification in a real browser. Desktop IME, real clipboard and Android overlay behavior still need live device checks. Bidirectional lines are covered by a headless Hebrew test only; mixed-direction lines are reported as one run, so per-run direction for mixed text and visual (not logical) caret movement still need work. IME composition is now tested to cancel on focus change, field disable and tab-panel hide.

### Web TSX callbacks and keyed reload

**Implemented:** WASM host delivers button clicks and field input, removes stale callbacks, and supports keyed signal reload state with reset reporting. Node/WASM tests exercise the retained paths; QuickJS remains optional.

**Remaining:** Verify these flows in a real browser and expand parity coverage for controls and reload edge cases. Unkeyed state still resets; this is not whole-heap preservation.

### Test helpers, snapshots, and inspect (related slices)

These three work well together, and can share the `tekto-test` crate above.

**Semantic snapshots, actions and inspect**

**Implemented:** Scene and AccessKit dumps expose ids and state; `tekto-test` finds by role/name or unique test key. CLI `inspect` and MCP `ui.inspect` report ancestry, bounds, clipping and semantics. An inspector page and headless diagnostics tests are present. A versioned semantic snapshot and exact role/name search use the realized AccessKit tree and return duplicate matches. Typed focus, activation, toggle-state and text/slider value actions use the shared tree and retained callback hooks; existing click, key, type and scroll actions remain available.

**Remaining:** Verify typed actions against retained TSX apps and unrealized rows, including callback failures and overlay restrictions. A debug overlay is optional; the current inspector page is not evidence of an in-window overlay.

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

- **Name:** One small token set, not a theme engine.
- **What it does:** Put today's quiet defaults (surface, ink, border, focus, spacing, font sizes) in one typed structure, with the same values for Rust and JSX, and optional app-level override. A later slice can honour a host text scale (for example 150%).
- **Why:** MAUI, Uno, and Flutter expose semantic colours and text scale. Shared tokens keep new widgets consistent without a visual redesign.
- **Priority:** medium. **Size:** S (text scale may make it M).
- **Crates it touches:** `tekto-widgets`, `tekto-text`, `tekto-js`, and maybe `tekto-platform` for host scale.
- **Design notes:** Default appearance must not change. No dark-mode engine, no CSS cascade, no theme editor, no automatic OS high-contrast on every platform in the first slice.
- **DX fit:** Keeps the plain look. Rust and TSX stay in sync. No new deps.
- **Acceptance:** Existing snapshots stay the same at default tokens. An override is applied consistently. If text scale is included, 100% and 150% are tested, including focus and disabled contrast.
- **Licence:** Own MIT code. The Design Tokens Community Group spec is a reference, not a runtime dependency.

### Keyed lazy lists

- **Name:** Build only visible rows from application keys.
- **What it does:** Today's list already skips layout and paint for off-screen rows, but the row nodes still exist. Add an opt-in path backed by item count, stable app keys, and a visible-range builder, so off-screen nodes can be dropped. An agent request for an off-screen key should scroll, materialize, and then return a fresh snapshot.
- **Why:** Flutter and Compose lazy lists keep memory bounded on large data. This is separate from JSX `<For>` (which is about keeping ids when items move).
- **Priority:** medium. **Size:** L.
- **Crates it touches:** `tekto-layout`, `tekto-widgets`, `tekto-tree`, `tekto-a11y`, MCP scroll/realize.
- **Design notes:** New materializations get new monotonic NodeIds. Keys are not NodeIds. Define focus rules when a focused row leaves the viewport. First slice: no variable-height estimate, no grid, no network paging, no fake accessibility tree for unrealized rows.
- **DX fit:** Same agent tree, honest errors for unrealized rows. No new deps.
- **Acceptance:** A large synthetic list keeps a bounded node count. Reorder, focus, and semantic inspection are tested. Scroll-then-materialize works for agents.
- **Licence:** Own MIT code.

### Responsive layout helpers

- **Name:** Viewport breakpoints and a collapsing split pane.
- **What it does:** Expose viewport size and a small helper to choose row or column, plus a split pane that can collapse to one pane on a narrow width. Inactive content must have clear focus and accessibility behaviour.
- **Why:** Avalonia's responsive queries and SwiftUI split views show why one fixed layout is not enough on desktop and web.
- **Priority:** medium. **Size:** M.
- **Crates it touches:** `tekto-layout`, `tekto-widgets`.
- **Design notes:** Stay on Taffy. No CSS media-query language, no DOM API, no automatic navigation policy, no multi-window management in this slice.
- **DX fit:** Original code, no new DSL, no new deps. Agents can still see both panes' ids when they exist, and a clear state when one is hidden.
- **Acceptance:** Headless resize tests, minimum sizes, and focus when a pane collapses.
- **Licence:** Own MIT code. Avalonia and SwiftUI are ideas only.

### Pointer capture and in-app drag

- **Name:** Pointer capture, then drag.
- **What it does:** Pointer down / move / up / cancel with capture by node id, released if the node is removed or focus is lost. Then a small in-app drag (for sliders and list reorder), plus an agent action such as `ui.drag { from, to }`. OS file drop can be a later slice (`DroppedFile`).
- **Why:** Flutter, Compose, and Qt have gesture and capture systems. Without capture, a drag breaks when the pointer leaves the control.
- **Priority:** medium. **Size:** M.
- **Crates it touches:** `tekto-platform`, `tekto-tree`, later MCP/CLI, and widgets that need drag.
- **Design notes:** First slice is one pointer, inside the tree. No pinch, no cross-window drag, no gesture DSL.
- **DX fit:** Agents get a deterministic drag, not a pointer-only control. No new deps.
- **Acceptance:** Tests for cancel, nesting, and not fighting scroll. A slider or reorder demo works with pointer and with the agent action.
- **Licence:** Own MIT code.

### JS native permission list

- **Name:** Deny-by-default capabilities for `tekto.applyNativeOp`.
- **What it does:** An app declares which native calls TSX may use (files, clipboard, tray, shortcuts, and so on). Anything else returns a clear error. A good default for agent-generated apps is narrow.
- **Why:** Tauri v2 capabilities are the model (ideas only). Agents write code; an open native bridge is a real risk.
- **Priority:** medium–high. **Size:** S–M.
- **Crates it touches:** `tekto-js`, `tekto-native`, maybe a `tekto.permissions.json` read by the CLI or host.
- **Design notes:** First slice is a flat allow-list, not per-path scopes or permission prompt dialogs.
- **DX fit:** Improves the agent story without new deps. Default builds unchanged. Errors are easy for agents to read.
- **Acceptance:** An allowed clipboard call works; a denied file call fails clearly. Missing config means a documented default (narrow or "all documented ops", but the choice must be explicit).
- **Licence:** Own MIT code. Tauri is MIT OR Apache-2.0 and is a reference only; do not copy its permission crate.

### Typed native API and template types

- **Name:** One command list, generated typings.
- **What it does:** A single list of native commands (name, args, result) generates Rust stubs and TypeScript `.d.ts`. The scaffolder template also gets a `typecheck` script and typings from `components.json`.
- **Why:** Tauri `invoke` typings and Wails bindings reduce drift. Docs already note that Rust and TS APIs are not fully equal. Agents write fewer wrong props with types.
- **Priority:** medium. **Size:** M for generated native typings, S for the template typecheck.
- **Crates / packages it touches:** `tekto-native`, `@tektogui/jsx-runtime` typings, `create-tektogui`.
- **Design notes:** Build-time script only; no runtime cost. No third-party plugin ecosystem in this slice.
- **DX fit:** Helps both humans and agents. Works with pnpm, bun, and npm. TypeScript stays a dev dependency of the template.
- **Acceptance:** A missing or mistyped native call is a type error. `create-tektogui` output typechecks. The generated list matches the Rust handlers.
- **Licence:** Own MIT code. TypeScript is Apache-2.0.

### QuickJS bytecode in release builds

- **Name:** Precompile TSX to QuickJS bytecode for release.
- **Why / what:** Faster cold start and no plain source shipped in release. Kiln lists this approach. Dev mode still uses source TSX.
- **Priority:** medium. **Size:** S–M.
- **Crates it touches:** `tekto-cli`, `tekto-js` (behind the existing `quickjs` feature).
- **Design notes:** Store the QuickJS version next to the bytecode and fall back to source if it does not match. Verify the exact rquickjs / qjsc API before promising this. No obfuscation.
- **DX fit:** Opt-in release path only. Default and PR CI stay clear of QuickJS.
- **Acceptance:** A release build starts from bytecode; a version mismatch falls back safely. Dev reload is unchanged.
- **Licence:** rquickjs and QuickJS-ng are already used (both MIT). Confirm the bytecode API in the version we ship.

### Animation upgrades (small)

- **Name:** Colour, a few easings, and repeat — only when a widget needs them.
- **What it does:** Tekto already tweens opacity, x, y, width, and height (180 ms, smoothstep, reduced-motion respected). Add colour (fill/color), a few named easings, and `repeat` (for a spinner) if a control needs it. Expose reduced-motion and a deterministic clock to tests.
- **Why:** Masonry, Iced, and Makepad have richer motion. The research sessions did not fully agree (see below), so this stays small.
- **Priority:** medium. **Size:** S–M.
- **Crates it touches:** `tekto-tree` animation code first; split a crate only if it grows a lot. Test helpers in `tekto-test`.
- **Design notes:** Do not build a new timeline, keyframe, or spring system unless a later widget truly needs it. Keep reduced-motion behaviour.
- **DX fit:** No new deps. Quiet motion, no bounce. Agents can still patch style; a `transition` field is optional later.
- **Acceptance:** A colour fade and a repeating spinner honour reduced-motion and the test clock. Existing tweens still work.
- **Licence:** Own MIT code. Easing formulas are ordinary maths, not copied framework code.

### Native extras

Each of these is a separate slice in `tekto-native`, with `Unsupported` / `Unavailable` / cancel results, mocks for tests, and no silent external actions.

- **Android shared-document picker (SAF).** Medium, M. Return an opaque document handle; do not pretend a content URI is a normal file path. Expo's file API is the idea, not the code. First slice: open and read only, no write, no persistent grants. Needs an emulator or device after the contract tests. Own MIT code plus Android platform APIs. No new crate and no GTK.
- **Packaging manifest.** Medium, M. Extend `tekto-cli` with app name, identifier, version, and icon path. Validate it and generate a Linux desktop entry and Android metadata for the workflows we already have. Do not claim an unsigned build is store-ready. Own MIT code. See [packaging](../packaging.md) and [signing](../signing.md).
- **Deep links and a share sheet.** Medium, S–M per platform. One host at a time, with injectable mocks so agents can observe the result. Native window menus, OS drag-and-drop, and multi-window are later and larger.
- **Read-only rich text spans.** Medium, later, M. After the multi-line plain field works, a `Text` node may hold bold, colour, size, and links. Parley already supports spans. This is not a rich editor. An extra bold font file should be opt-in because of size.

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
