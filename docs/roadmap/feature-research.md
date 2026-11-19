# Feature opportunities and implementation slices

Research snapshot: 2026-11-11. These are proposed experiments informed by [framework research](framework-research.md), not shipped features or an approved schedule. S = days, M = one to two weeks, L = larger or cross-platform. Start with current Rust and TSX behavior and retain the same scene ids and AccessKit semantics. [Round 3](round3-research.md) lists the current gaps and platform evidence.

## 1. Predictable virtual lists (M)

**Why.** [Flutter's scroll documentation](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/widgets/scroll_view.dart) describes lazy construction and semantic indices/counts. Tekto already has `KeyedList` in `crates/tekto-widgets/src/lazy.rs`: fixed-height rows, application keys, overscan and explicit `sync`. A row that leaves the realized window is removed, so its node id is not a permanent id for that key.

**First slice.** Define focus behavior before changing realization: when a focused row leaves, move focus to the list container and retain an optional application key for an explicit return; when the key is deleted, clear it. Provide visible index range and total count to the existing semantics mapping where the platform schema can represent them. Keep agent inspection honest: a key outside the window is data, not a live accessible node. Add a TSX binding only after the Rust lifecycle and ownership rules are stable.

**Acceptance.** Headless tests cover reorder, deletion, scrolling away and back, duplicate keys, focus, callbacks and bounded realized-node count for 1,000 rows. Exercise a screen reader announcement separately. Variable heights need measurement, an offset index and anchoring when estimates change; treat them as a later slice, not a hidden requirement of the fixed-height version.

**Identity contract to decide.** An application key can survive reordering; the currently realized node id survives only while that row remains materialized; row-local state must either live in the application or have an explicit bounded retention policy. This differs from JSX `For`, which reconciles children by key but does not itself make them virtual. [Compose's item-key documentation](https://developer.android.com/develop/ui/compose/lists#item-keys) motivates stable item identity, not a guarantee that Tekto should retain every off-screen widget. For `scroll_to_key`, define whether completion means offset requested, row realized, layout complete or focus moved. Deletion while focused and an empty result need deterministic outcomes. Do not expose logical total/range to an OS adapter until the pinned AccessKit schema and that adapter are checked.

## 2. Navigation with observable state (M)

**Why.** The [Flutter navigator source](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/widgets/navigator.dart) distinguishes route operations and restoration. Tekto's roadmap already lists `tekto-navigation`; an agent should be able to know which route is active without reading pixels.

**First slice.** Prototype a Rust route stack with app-owned stable route keys and explicit push, pop and replace. Expose the active route and back availability through the existing opt-in state/agent surface, without publishing private route arguments by default. Save a focus target on leaving a route and restore it only if the node still exists, is visible and enabled; otherwise focus a named route root. Route transitions initially have no animation. Add a TSX adapter using the same operations and lifecycle after the Rust contract is tested; keep OS back integration platform-specific.

**Acceptance.** Test empty/root pop, replacement, invalid focus target, nested dialogs, route disposal, back and restore, and Rust/TSX parity. A manual Android back-button test is separate from unit tests. Persisting arbitrary app state, deep links and nested routers are later work.

## 3. Cancellable background work (M)

**Why.** [Iced documents tasks and passive subscriptions](https://github.com/iced-rs/iced/blob/master/src/lib.rs), including cancellation. Tekto apps need long work without blocking input, but a mandatory async runtime would conflict with lean defaults and web portability.

**First slice.** Define a small host-neutral completion/cancellation contract: an operation has an owner and generation; completion is delivered to the UI event loop only while that owner is alive. Cancellation is cooperative and late results are ignored, not assumed to stop underlying I/O. Prototype a native executor behind an optional feature and a browser Promise adapter without exposing DOM types in the app API. Decide error, shutdown and threading semantics before committing to a public API or dependency.

**Acceptance.** Test completion, error, cancellation, owner removal, reload, app shutdown and out-of-order results; verify callbacks run on the UI owner. Benchmark idle overhead and binary/build impact. Document platform limitations instead of promising identical thread models.

## 4. Accessible data views and interaction rules (L, staged)

**Why.** Round 3 defers a read-only grid and charts. [AccessKit](https://github.com/AccessKit/accesskit/blob/main/README.md) supports semantic roles and actions; [WAI-ARIA tabs](https://www.w3.org/WAI/ARIA/apg/patterns/tabs/) and [modal dialogs](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/) provide useful keyboard reference patterns, but do not replace native platform testing.

**First slice.** Ship a read-only table with a named container, header labels and a small fixed data set; define keyboard navigation and expose semantic row/cell relationships using what the current AccessKit version and target adapters actually support. Then add row virtualization using the list contract above. For a chart, begin with a titled bar chart and an equivalent textual values view; do not promise editable spreadsheets or rich text.

**Acceptance.** Verify labels, reading order, focus and keyboard actions in headless semantics and on at least one real assistive technology. Record unavailable adapter properties as limitations. Test empty data, large data, scrolling and duplicate labels. Both Rust and TSX examples must use the same underlying nodes.

## 5. Complete input before expanding text (M–L)

The [Parley overview](https://github.com/linebender/parley/blob/main/README.md) distinguishes shaping, bidi, segmentation, layout and selection; these are distinct failure modes. Round 3 already identifies mixed-direction movement and real IME/clipboard testing. First create repeatable examples for a Windows IME, Android field overlay, browser composition and clipboard, including cancellation, focus change and surrogate/grapheme boundaries. Then tackle per-run bidi and visual caret movement with headless geometry tests plus real input-method checks. Read-only styled spans can follow once selection and accessibility behavior are specified.

**Offset and behavior matrix.** [Unicode UAX #29](https://www.unicode.org/reports/tr29/) defines grapheme segmentation; [UAX #9](https://www.unicode.org/reports/tr9/) describes bidi logical and display order. Neither makes UTF-8 bytes, Unicode scalar indices, UTF-16 code units, grapheme clusters, glyphs and visual positions interchangeable. Document the unit and conversion at each Rust tree, Android overlay, browser input and AccessKit boundary. Exercise combining marks, emoji sequences, Arabic/Hebrew mixed with digits and Latin, wraps, selection replacement and IME preedit cancellation. Test copy/paste without putting actual user clipboard content into logs. Platform IME and screen-reader checks remain manual until a suitable controlled harness exists.

## 6. Drag, reorder and gestures (M–L)

**Evidence and current state.** Tekto already captures a pointer for sliders, cancels touch without clicking, and suppresses synthetic mouse events in `crates/tekto-platform/src/pointer_route.rs`; round 3 says there is no generic drag callback or agent `ui.drag`. The [Pointer Events specification](https://www.w3.org/TR/pointerevents3/#pointer-capture) distinguishes capture and cancellation, while the [APG slider pattern](https://www.w3.org/WAI/ARIA/apg/patterns/slider/) documents keyboard behavior and warns about touch assistive-technology differences.

**First slice.** Define begin/move/end/cancel with a pointer identity, logical coordinates, capture ownership and cancellation on node removal, focus loss or host suspend. Choose a movement threshold and whether scrolling or dragging wins before adding reorder. Deliver callbacks on the UI owner and bound queued moves; do not synthesize a click after a completed drag or cancellation. Add an agent action that drives the same routing, with explicit start/end and intermediate timing, rather than directly rewriting list state. Make keyboard move-up/move-down an equivalent accessible operation for reorder.

**Acceptance.** Headless tests cover crossing bounds, second pointer, capture removal, cancel, touch-to-mouse suppression, scroll conflict and a target disappearing mid-drag. Test mouse, touch and screen reader on real target devices; do not call external drag-and-drop, files, pinch or multi-touch complete based on a single-pointer prototype.

## 7. Focus, selection and motion consistency (S–M per control)

The [APG listbox](https://www.w3.org/WAI/ARIA/apg/patterns/listbox/) distinguishes focus from selection and recommends type-ahead; the [tree view pattern](https://www.w3.org/WAI/ARIA/apg/patterns/treeview/) describes expansion and directional navigation; the [grid pattern](https://www.w3.org/WAI/ARIA/apg/patterns/grid/) distinguishes a navigable grid from a static table. These are web guidance to adapt and validate against native conventions, not automatic proof of AccessKit support.

Audit existing select, tabs, menu, slider and dialog with a table of initial focus, arrows, Home/End, activation, Escape, disabled behavior, removal and focus restoration. Add new tree/grid controls only after choosing whether focus and selection follow one another and whether a static table is sufficient. Round 3 already has reduced motion for tweens; the [WCAG animation guidance](https://www.w3.org/WAI/WCAG22/Understanding/animation-from-interactions.html) describes a Level AAA criterion for disabling nonessential interaction-triggered motion. Investigate OS preference observation and per-app override, test changes while animation is active, and state any platform limitations; do not claim WCAG conformance from a single setting.

## 8. Localization and native documents (M–L, independent tracks)

**Localization.** Round 3 defers `tekto-i18n`. [ICU4X](https://github.com/unicode-org/icu4x/blob/main/README.md) describes modular internationalization components and pluggable locale data; Tekto already has ICU transitive dependencies through the text stack, but this does not make a public localization API or all locale data available. Begin with an app-supplied locale, named messages, a documented fallback chain and a small plural example. Define how Rust and TSX receive the same resolved strings and how locale changes invalidate layout and accessible names. Test missing keys, plural categories beyond English, long translations and right-to-left layouts. Measure data and binary cost and review direct dependency licenses before selecting a library; do not implement plural rules with an English-only `count == 1` shortcut. Text bidi correctness and layout direction are related but separate.

**Native documents.** Existing Android file operations are app-private; round 3 separately lists shared documents and SAF. The [Android Storage Access Framework guide](https://developer.android.com/training/data-storage/shared/documents-files) describes user-selected documents and URI-based access. Prototype a user-initiated open/save operation that returns an opaque, scoped handle with explicit lifetime and permissions, rather than pretending a document URI is a normal filesystem path. Define unsupported, user-cancelled, permission-revoked and I/O errors across Rust and TSX. Test with fake host results first, then a real device including process restart if persistent access is offered. Do not imply that app-private paths already provide shared-document access.

## Sequencing and dependencies

| Candidate | Prerequisite | Small reviewable outcome | Evidence still needed |
| --- | --- | --- | --- |
| Virtual list contract | Existing `KeyedList`, focus/semantics audit | Fixed-height list with documented ownership and focus | AT announcement and TSX host parity |
| Drag/reorder | Pointer cancel/capture rules, virtual-list identity | One keyboard-accessible reorder example | Real touch and screen-reader test |
| Navigation | Focus restoration and disposal contract | Push/pop/replace with a named route root | Android back and host lifecycle |
| Async work | UI-owner dispatch and cancellation contract | One cancellable request with ignored late completion | Native/web lifecycle and binary cost |
| Read-only data view | Semantic roles, focus and list realization | Static labeled table, then virtual rows | Platform adapter support and AT behavior |
| Input and bidi | Explicit offset conversions and fixtures | Mixed-script caret/selection correctness | Real Windows, Android and browser IME |
| Localization | Message and locale ownership; text invalidation | Two locales, plural and fallback example | Data size, translated content and RTL device checks |
| Shared documents | Capability/permission and error contract | User-selected opaque document handle | Android SAF and desktop dialog tests on devices |

## Decision gates

- Every interactive node gets a stable id, accessible role and name, and the existing tree remains the agent tree.
- Define ownership, cancellation and focus on removal before exposing an API. Add a Rust and TSX example for user-facing features.
- Keep GPU, QuickJS and heavy integrations optional. Review licenses and target-specific dependency graphs before adding crates.
- Treat device checks as evidence for the tested device and OS only. Do not claim browser, Windows or Android parity from compilation or headless tests.
