# Round 3 research roadmap

Status: 2026-11-11. This is a research list, not a release promise and not a set of open GitHub issues. It was written from a review of other GUI frameworks, then checked against `nightly` at `89726d8`.

Tekto already has a Rust core, TSX on QuickJS, a first-class Rust API, MCP and CLI control, hot reload, and targets for Linux, Windows, Android, headless, and web (WASM). This list is about what to add next so the toolkit is more complete, while keeping the same rules: our own tree and stable ids, agent-first tools, fast builds, small apps, a plain quiet look, and JS packages that work with pnpm, bun, and npm.

Four research sessions produced the source notes (Rust kits, cross-platform app frameworks, web/desktop shells and 2025–2026 experiments, and agent tooling). Their findings were merged and de-duplicated before this document was written.

Sizes are rough: **S** is a few days, **M** is about one to two weeks, **L** is larger or crosses many hosts. Priority is **high**, **medium**, or **low**.

## Suggested order

This is a recommended order only. Hardware work (Windows runtime, macOS, iOS) and merging `nightly` into `main` stay separate decisions.

1. Short foundations: paint primitives (rounded corners, borders, lines), then an overlay layer so menus and dialogs can sit on top.
2. Everyday agent and app DX: web TSX callbacks, a small `tekto-test` crate, richer tree snapshots and actions, an inspect command, and opt-in keyed TSX reload so state is not wiped.
3. Widgets people need for real apps: text selection and copy/paste, menus and a dropdown, a modal dialog, then radio / switch / slider / progress, then images.
4. Start the multi-line text area early, because it is large, but do not block the smaller slices on it.
5. Later: quiet design tokens, a simple navigation stack, right-to-left foundations, JS native permissions, a shared bundler and template types, lazy lists, a read-only table and small charts, and native extras (Android shared documents, share, deep links).

## New crates

### `tekto-test`

- **What it does:** A small headless test helper. Tests can find a control by stable id, role, or accessible name, click or type on it, wait for a condition, assert focus / value / checked / disabled, and optionally compare a CPU PNG to a saved picture (with a diff image and an update mode).
- **Why:** Flutter's widget and semantics tests, Masonry's pixel snapshots, and Playwright's visual comparisons all show that screenshots alone miss broken names, focus, and state. Tekto already has headless PNGs and record/replay; they are not yet a reusable test API.
- **Priority:** high. **Size:** S–M.
- **Crates it touches:** new `tekto-test` (dev-only). Small use of `tekto-tree`, `tekto-a11y`, the CPU renderer, and maybe `tekto-cli`.
- **Design notes:** Use the real scene and AccessKit mapping, never a second tree. Fix the viewport, scale, bundled font, and animation clock so pixels stay stable. Semantic assertions should ship first; PNG compare can be a second slice. An optional developer test key may resolve to a live id, and must fail if it is duplicated.
- **DX fit:** Agent-first: the same ids and actions as MCP/CLI. Fast builds: dev-dependency only, no GPU or QuickJS in the default graph. Tests should cover Rust and, when the `quickjs` feature is on, a TSX callback flow. Quiet look is unchanged.
- **Acceptance:** A small Rust form can be found, activated, and asserted by role and name. A stale or disabled id fails clearly. Optional QuickJS path updates a label after a click. PNG compare (second slice) writes expected / actual / diff and supports an explicit update command. `cargo test` on Linux stays CPU-only.
- **Licence:** Own MIT code. Reuse existing `png` support (MIT OR Apache-2.0, already in the workspace). Check any extra diff crate before adding it. Do not copy Playwright or Flutter test code.

### `tekto-image`

- **What it does:** Let an app show a picture: a new image node, decode (or accept ready pixels), scale with contain/cover, clip, and an accessible name or an explicit "decorative" mark.
- **Why:** Flutter, Iced, egui, GPUI, MAUI, and almost every other toolkit can show an image. Tekto currently cannot show a logo, avatar, or icon.
- **Priority:** high. **Size:** M.
- **Crates it touches:** new `tekto-image` (decode and cache), plus `tekto-render` (`draw_image`), `tekto-tree` (`NodeKind::Image`), `tekto-widgets`, `tekto-a11y`, `tekto-js`, and the JSX runtime.
- **Design notes:** First slice can draw app-supplied RGBA pixels with size limits, so no new decoder is required. Then add an opt-in PNG feature. JPEG or WebP stay behind later features. No network loading, no animated GIF, no SVG in this crate.
- **DX fit:** Same ids for Rust, TSX, AccessKit, and agents. Default build stays small: PNG only when opted in. Agents can read the alt text. Quiet look: no decorative chrome around the image.
- **Acceptance:** Headless tests cover a valid image, invalid or missing data, clipping, contain/cover, alt text, and decorative state. Rust and TSX HostOps both work. CPU paint is checked; GPU may fall back in this slice.
- **Licence:** Own MIT code. If a decoder is added, prefer the existing `png` crate (MIT OR Apache-2.0). Confirm any JPEG crate (for example `zune-jpeg`) before adding it. Do not pull the heavy `image` crate by default.

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

- **What it does:** One small helper or CLI that turns `app.tsx` into a single JS file for both QuickJS (desktop) and the WASM host. Today the web package has its own bundler path; this would share it.
- **Why:** Lynx and Electrobun give one command for dev and release. A single path is easier for agents and for pnpm, bun, and npm.
- **Priority:** medium. **Size:** S.
- **Packages it touches:** new `@tektogui/build`; can reuse logic in `packages/wasm` (`bundle-app.js`). No Rust-side watcher rewrite.
- **Design notes:** Wrapping esbuild is enough. No CSS pipeline and no plugin API in the first slice.
- **DX fit:** Published `package.json` stays manager-neutral (no pnpm-only fields). Fast and small: this is a dev tool, not shipped inside apps.
- **Acceptance:** The same app entry bundles for the desktop QuickJS host and for `packages/wasm`, from npm, pnpm, and bun. Existing wasm dev server still works.
- **Licence:** Own MIT code. esbuild is MIT (confirm in its repo before adding or documenting it as a dependency).

### Keyed JSX helpers (`<For>`, `<Show>`, optional `<Switch>`)

- **What it does:** Control-flow helpers so a list can keep stable node ids when items move, and so conditionals do not remount everything. This can live in `@tektogui/jsx-runtime` or a tiny `@tektogui/flow` package.
- **Why:** Solid's keyed lists and `createRenderer` (MIT, ideas only) are the model. Tekto's never-reused ids are a selling point for agents; a naive remount throws that away. This is different from a lazy data list in layout (see extensions below).
- **Priority:** high. **Size:** M.
- **Packages it touches:** `@tektogui/jsx-runtime` and maybe `@tektogui/core`. HostOp and tree ids must stay monotonic.
- **Design notes:** Write our own helpers. No Solid compiler transform in the first slice. Keys are application keys, not NodeIds.
- **DX fit:** Pure JS, no new runtime deps, works with pnpm, bun, and npm. Agents keep talking to the same ids for items that still exist.
- **Acceptance:** Moving, inserting, and removing items keeps ids for the items that remain. A headless or QuickJS test shows a click still hits the same item after a reorder. Type declarations are updated.
- **Licence:** Own MIT code. Solid is MIT and is ideas only; do not copy its compiler.

### Later packages

- **`create-tektogui` typecheck template:** Add `tsconfig.json`, a `typecheck` script, and typings generated from `components.json`. Medium, S. TypeScript is Apache-2.0 and already a workspace dev dependency. Cheap and helpful for agents.
- **`@tektogui/solid`:** An experimental Solid renderer on HostOp, as a peer dependency, not a replacement for `@tektogui/core`. Low, M–L. Solid is MIT. Do not copy code from Wabou or similar projects until their licences are checked. Optional only.
- **`@tektogui/mcp-apps` or `@tektogui/a2ui`:** Adapters for [MCP Apps](https://apps.extensions.modelcontextprotocol.io/api/) or [A2UI](https://github.com/a2ui-project/a2ui) (Apache-2.0), only if a real consumer exists. Low, M. Do not embed a browser in the native renderer. Check the MCP Apps SDK licence before adding it.

## Extensions to existing crates and packages

### Paint primitives

- **Name:** Rounded rectangles, borders, and lines in the renderer.
- **What it does:** Add `fill_rrect`, `stroke_rrect`, and `draw_line` to the `Renderer` trait, with a fallback to `fill_rect` so older backends still compile. Nodes get `radius`, `border`, and `border_color`.
- **Why:** The current trait only has `fill_rect`, `draw_text`, and clip push/pop. Iced, egui, Masonry, GPUI, Flutter, and Slint all have rounded shapes and strokes. Almost every later widget (switch, slider, tabs, tooltip, dialog) needs this. A quiet look needs a thin 1px border more than it needs new colours.
- **Priority:** high. **Size:** S–M.
- **Crates it touches:** `tekto-render`, `tekto-tree`, and paint paths in the CPU (tiny-skia) and optional GPU (Vello) backends.
- **Design notes:** Trait defaults keep other renderers compiling. No blur shadows, gradients, or per-corner radii in the first slice. Fields should show up in `ui.tree`.
- **DX fit:** No new dependencies. Default build stays CPU-only. Agents can read the new style fields. The default palette stays quiet.
- **Acceptance:** Headless snapshots of a rounded rect, a 1px border, and a line. GPU path implements the same calls or falls back cleanly. Existing button and field snapshots stay valid or are updated on purpose.
- **Licence:** Own MIT code. tiny-skia is BSD-3-Clause. Vello is Apache-2.0 OR MIT. Both are already used.

### Overlay layer

- **Name:** Window-level overlay for menus, tooltips, and dialogs.
- **What it does:** Nodes can be attached to a window overlay, paint last, hit-test first, and sit at an anchor (a node id and a side). Clicking outside closes the overlay; Escape is routed to it.
- **Why:** Masonry, Iced (`stack`), and egui (`Area`) all have a layer above the normal tree. Without this, menus, dropdowns, tooltips, and dialogs cannot work correctly.
- **Priority:** high. **Size:** M.
- **Crates it touches:** `tekto-tree`, `tekto-layout`, `tekto-render`, `tekto-a11y`, and later `tekto-widgets`.
- **Design notes:** In the AccessKit tree the overlay stays a child of the window, with a proper role and stable ids. Agents click it like any other node. First slice: no collision flipping, arrow pointers, or extra motion.
- **DX fit:** Same ids for Rust, TSX, AccessKit, and MCP. No new deps. Fast compile unchanged.
- **Acceptance:** Headless tests: overlay paints above siblings, hits the top node, click-outside closes, Escape closes, and a blocked background click fails. AccessKit contains the overlay nodes.
- **Licence:** Own MIT code.

### Menus, dropdown, and modal dialog

- **Name:** Popup menu, single-select, and modal dialog.
- **What it does:** After the overlay layer exists, add a quiet menu (and a context menu built from it), a single-select control with labelled options, and a modal dialog with a focus trap, accessible name, Escape/cancel, and background hit blocking. Focus returns when the popup or dialog closes.
- **Why:** Flutter, MAUI, SwiftUI, Qt, egui, Iced, and gpui-component all have pickers, menus, and dialogs. Forms and settings are hard without them.
- **Priority:** high. **Size:** M (select and dialog can be separate slices).
- **Crates it touches:** `tekto-widgets`, `tekto-tree`, `tekto-layout`, `tekto-a11y`, `tekto-js`, JSX runtime / `components.json`, and input routing.
- **Design notes:** Every item has a stable id. Agents can open, move, and choose with the same actions as a person. First slice: no nested submenus, no native OS menu bar, no searchable combo box, no multi-select, no fancy placement across monitors.
- **DX fit:** Agent-first by design. No extra toolkit dependency. Quiet scrim and typography. Rust and TSX share the same kinds and events.
- **Acceptance:** Keyboard (arrows, Enter, Escape), pointer, disabled items, focus restore, Rust/TSX parity, and AccessKit expanded/selected/dialog roles. Clicking a control behind the modal must fail.
- **Licence:** Own MIT code.

### Radio, switch, slider, progress, and tabs

- **Name:** First extra form controls, then tabs.
- **What it does:** Add radio group, switch, slider, and progress, each with disabled state, keyboard (arrows / Home / End where it applies), AccessKit roles, and agent actions (including a value-set action for the slider). Tabs come after: a tab list and panel with roving focus and arrow keys.
- **Why:** This is the obvious hole versus Flutter's widget catalog, Compose, Iced, egui, and Masonry. Tekto today has button, checkbox, a single-line field, list, and scroll. A slider also needs pointer capture (see below).
- **Priority:** high for the first batch, medium for tabs. **Size:** M for the batch, S for tabs.
- **Crates it touches:** `tekto-widgets`, `tekto-tree` (new `NodeKind` values), `tekto-a11y`, `tekto-render` (after paint primitives), JSX schema, MCP/CLI as needed.
- **Design notes:** Follow the existing add-widget skill and the checkbox pattern. First slice: no range slider, no vertical slider, no tick marks, no closable or draggable tabs.
- **DX fit:** No new deps. Same ids and MCP actions. Quiet muted defaults, including disabled.
- **Acceptance:** Headless tests for keyboard, pointer, agent set-value, disabled, AccessKit roles, and CPU paint. JSX and Rust helpers match.
- **Licence:** Own MIT code. Masonry (Apache-2.0) may be read for behaviour, with attribution if any code is copied.

### Text selection, copy/paste, then a multi-line field

- **Name:** Selection first, then `TextArea`.
- **What it does:** Give the existing single-line field a selection (keyboard and pointer), replace-selection, copy/cut/paste through the existing clipboard, and a visible IME composition range. Then add a plain multi-line area with wrapping, Up/Down/Home/End, scrolling to the caret, and AccessKit text selection.
- **Why:** Flutter, MAUI, SwiftUI, and Masonry treat selection and multi-line editing as core. This is the biggest practical limit for notes and forms. Parley already does rich layout; Tekto just does not expose it yet.
- **Priority:** high. **Size:** L. Ship selection before the text area if possible, and start the text area early because it is large.
- **Crates it touches:** `tekto-text`, `tekto-tree`, `tekto-widgets`, `tekto-a11y`, `tekto-platform`, `tekto-native` (clipboard), and input hosts (desktop, Android overlay, later web).
- **Design notes:** Check whether Parley's `PlainEditor` can be used directly instead of writing a new editor. Keep Unicode indexing conversions explicit at host boundaries. First slice is plain text only: no rich editing, no spellcheck, no syntax highlighting, no huge-document editor, no undo history beyond a simple start.
- **DX fit:** Agents already have `ui.type` and `ui.key`; extend them carefully for selection and multi-line movement. No new text engine. QuickJS and WASM stay optional. Quiet field chrome, same as today.
- **Acceptance:** Headless tests for start/middle/end selection, grapheme clusters and emoji, copy/cut/paste, IME commit and cancel, disabled fields, and (for the text area) wrapping and vertical movement. Desktop IME and Android overlay still need live checks on real devices; do not claim those from Linux CI alone.
- **Licence:** Parley is already a dependency (Apache-2.0 OR MIT; confirm the crate licence at the version we use if `PlainEditor` is adopted). Own MIT glue. No new text stack.

### Web TSX callbacks

- **Name:** Browser `onClick` and `onInput` parity.
- **What it does:** When the web canvas hits a node, the browser JS host should run the same JSX handlers that desktop QuickJS already runs: button `onClick` and field `onInput`, with cleanup on remove and remount.
- **Why:** Lynx, Valdi, and Wails all run JS handlers on every target. Tekto's web host already maps pointer and wheel to the tree, but a hit does not call TSX. Shared apps cannot behave the same on web and desktop.
- **Priority:** high. **Size:** M.
- **Crates / packages it touches:** `crates/tekto-wasm`, `packages/wasm`, `@tektogui/jsx-runtime`.
- **Design notes:** The browser already runs JS; do not enable QuickJS on WASM. No public DOM API for app code. First slice: no web IME, no web text selection, no extra gesture framework.
- **DX fit:** Same ids and events as desktop. No new deps. npm, pnpm, and bun tests keep working. Fast default builds unchanged.
- **Acceptance:** Node or wasm tests click a button and type in a field, see the handler run, ignore disabled controls, and drop stale handlers after remount. Existing pointer/scroll tests still pass.
- **Licence:** Own MIT code.

### State-preserving TSX reload

- **Name:** Opt-in keyed signal state across hot reload.
- **What it does:** Today a TSX reload remounts the app and resets signals. Allow named signals (for example `createSignal(init, { key })`) to be saved and restored across a reload. Incompatible or missing keys reset only that piece of state and report it.
- **Why:** Compose Hot Reload and typical Vite / Solid HMR keep some state, which makes iteration less painful. Agents lose their place after every edit today.
- **Priority:** high. **Size:** M.
- **Crates / packages it touches:** `@tektogui/core`, `@tektogui/jsx-runtime`, and `tekto-js` (and the web host where it can support the same hook).
- **Design notes:** Do not promise to serialize closures or the whole heap. Do not mix this up with live `ui.patch` writeback. Rust binary hot reload stays a separate, experimental topic.
- **DX fit:** QuickJS stays behind its feature. The JS API must work with pnpm, bun, and npm. Agent live-edit loops get much better without changing the tree model.
- **Acceptance:** A counter or form keeps its keyed state across reload. A renamed or incompatible key resets only that value and is reported. Unkeyed signals still reset, as today.
- **Licence:** Own MIT code. Compose and Solid HMR are ideas only (and Solid HMR is known to be only a partial solution).

### Test helpers, snapshots, and inspect (related slices)

These three work well together, and can share the `tekto-test` crate above.

**Richer semantic snapshots and actions**

- **What it does:** A compact, versioned tree snapshot with id, role, name, value/state, focus, bounds, and supported actions, plus search by role or name, and typed actions such as focus, activate, set value, and toggle. All of this goes through the same path as real user input.
- **Why:** Playwright MCP snapshots and Compose's experimental MCP access show that agents work best with a searchable semantic tree, not only a raw dump and a screenshot.
- **Priority:** high. **Size:** M.
- **Crates it touches:** `tekto-a11y`, `tekto-tree`, `tekto-mcp`, `tekto-cli`, and HostOp bindings.
- **Design notes:** Never bypass disabled state or fire a callback twice. Ids are for a node's lifetime; they are not a promise across a remount. For virtualized lists, do not invent ids for unrealized rows.
- **DX fit:** This is a direct upgrade to the MCP/CLI surface. No second accessibility tree. No new default deps.
- **Acceptance:** Rust and TSX parity tests, stale ids, duplicate test keys, disabled no-ops, and a documented CLI/MCP shape.
- **Licence:** Own MIT code. Playwright MCP is Apache-2.0 and is a reference only.

**Inspect and layout diagnostics**

- **What it does:** An `inspect <id>` output (CLI and MCP) with ancestry, computed bounds, clip, scroll, focus, role/name/state, and layout input versus computed size. Optional debug-only box overlay under the pointer. A thin browser page over the existing dev-server endpoints may come later.
- **Why:** Flutter Inspector, Lynx DevTool, Makepad, and egui all let you see layout boxes. Tekto's HTTP tree is strong, but a human or an agent still has to guess why something is clipped.
- **Priority:** high / medium. **Size:** M.
- **Crates / packages it touches:** `tekto-mcp`, `tekto-cli`, optional overlay in `tekto-platform`. A later `@tektogui/inspector` page is optional and dev-only.
- **Design notes:** Start with JSON/CLI, then a thin page or overlay. No network profiler, no full timeline, no heavy separate GUI app, no edit-in-place in the first slice.
- **DX fit:** Dev-only, not in release binaries. Same ids as `ui.tree`. No new Rust deps for a static HTML page.
- **Acceptance:** Headless tests assert the diagnostic JSON and overlay geometry. A missing id fails clearly.
- **Licence:** Own MIT code.

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

## Notes where the research sessions disagreed

- **First step:** one session wanted paint and overlays first; others wanted tests, web callbacks, and selection first. Both fit if paint and overlay are short foundations, not the whole milestone.
- **`tekto-test`:** one session wanted golden PNGs first; another wanted semantic queries first and left image-diff out of the first slice; a third wanted both. This document follows semantic assertions first, PNG compare second, in one crate.
- **Inspector:** on-screen overlay vs a browser page vs a CLI/MCP dump. Start with inspect JSON/CLI, then a thin page and/or overlay.
- **Images now or later:** two sessions ranked images high; one wanted a decoder and size review first. The compromise is: draw already-decoded pixels first, then an opt-in PNG feature after the licence check (`png` is already in the workspace).
- **Animation:** one session wanted springs and more easings; another said do not rebuild motion. This document keeps the current tween system and adds colour or repeat only when a widget needs them.
- **i18n home:** a new crate vs a first slice in existing crates. Start in the existing crates; split later if needed.
- **Keyed JSX lists vs lazy data lists:** these are different jobs and should stay separate issues if they are ever filed.
- **OS accessibility:** one session ranked a Linux AT-SPI smoke test as high. It is worth doing, but it needs a real desktop session and must stay behind `a11y-os`.

Windows runtime checks, macOS and iOS work, PR #101, and merging `nightly` into `main` are not part of this list.
