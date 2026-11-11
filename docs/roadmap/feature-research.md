# Feature opportunities and implementation slices

Research snapshot: 2026-11-11. These are proposed experiments informed by [framework research](framework-research.md), not shipped features or an approved schedule. S = days, M = one to two weeks, L = larger or cross-platform. Start with current Rust and TSX behavior and retain the same scene ids and AccessKit semantics. [Round 3](round3-research.md) lists the current gaps and platform evidence.

## 1. Predictable virtual lists (M)

**Why.** [Flutter's scroll documentation](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/widgets/scroll_view.dart) describes lazy construction and semantic indices/counts. Tekto already has `KeyedList` in `crates/tekto-widgets/src/lazy.rs`: fixed-height rows, application keys, overscan and explicit `sync`. A row that leaves the realized window is removed, so its node id is not a permanent id for that key.

**First slice.** Define focus behavior before changing realization: when a focused row leaves, move focus to the list container and retain an optional application key for an explicit return; when the key is deleted, clear it. Provide visible index range and total count to the existing semantics mapping where the platform schema can represent them. Keep agent inspection honest: a key outside the window is data, not a live accessible node. Add a TSX binding only after the Rust lifecycle and ownership rules are stable.

**Acceptance.** Headless tests cover reorder, deletion, scrolling away and back, duplicate keys, focus, callbacks and bounded realized-node count for 1,000 rows. Exercise a screen reader announcement separately. Variable heights need measurement, an offset index and anchoring when estimates change; treat them as a later slice, not a hidden requirement of the fixed-height version.

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

## Decision gates

- Every interactive node gets a stable id, accessible role and name, and the existing tree remains the agent tree.
- Define ownership, cancellation and focus on removal before exposing an API. Add a Rust and TSX example for user-facing features.
- Keep GPU, QuickJS and heavy integrations optional. Review licenses and target-specific dependency graphs before adding crates.
- Treat device checks as evidence for the tested device and OS only. Do not claim browser, Windows or Android parity from compilation or headless tests.
