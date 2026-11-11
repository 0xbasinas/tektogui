# Runtime and rendering research

Research snapshot: 2026-11-11. Recommendations below build on [round 3](round3-research.md) and the [framework comparison](framework-research.md). Sizes are rough engineering estimates, not deadlines: S = days, M = one to two weeks, L = larger or cross-platform. Measure on representative machines before accepting any performance claim.

## 1. Trace resize and frame stages before changing presentation (S)

**Evidence.** Round 3 reports an intermittent thin black strip during resize and layout costs of 40–60 ms on some resize frames. `tekto-platform/src/desktop.rs` already has opt-in `TEKTO_FRAME_LOG`; `tekto-render/src/gpu.rs` has a direct window-surface path. The [wgpu example](https://github.com/gfx-rs/wgpu/blob/trunk/examples/features/src/hello_triangle/mod.rs) treats acquisition outcomes separately and reconfigures or redraws in several cases. That example is a comparison, not proof of the cause of Tekto's strip.

**Slice.** Extend opt-in diagnostics with a frame sequence, monotonic timestamp, logical size, scale, physical `inner_size`, renderer size, surface configuration, acquired texture size, outcome and present result. Separate input/queue, layout, paint recording, GPU submission and present timing where measurable; GPU completion needs an explicitly asynchronous measurement rather than being inferred from submit time. Bound the log and avoid text, paths or app data. Record adapter, backend, OS and driver in a user-supplied report.

**Done when.** A Windows reproduction compares these values on frames with and without the strip, including resize in each direction and mixed-DPI moves. Unit-test outcome-to-action decisions headlessly; verify real recovery on the target device. Do not assert a DWM, rounding, or swapchain cause until the trace supports it. Compare p50/p95 frame times and worst resize frame with logging disabled; record hardware, build profile and workload.

**Surface lifecycle to investigate.** In the current desktop path, `present_window` outcomes feed a softbuffer/readback fallback; inspect both `GpuRenderer::present_window` and `redraw` in `desktop.rs` before changing policy. The [upstream wgpu surface documentation](https://github.com/gfx-rs/wgpu/blob/trunk/wgpu/src/api/surface.rs) warns that reconfiguration may wait for GPU idle, requires nonzero sizes, and must not occur while a surface texture is alive. Verify those details against the pinned wgpu version. Test acquisition timeout, occlusion, suboptimal, outdated, lost, validation error, minimize/restore and surface recreation as separate cases; avoid retry loops that spin when occluded. A fake outcome test proves policy only, while surface/device-loss behavior needs an actual backend. In particular, classify a direct-present failure at that boundary rather than assuming a later readback error describes it.

## 2. Reuse text work with explicit invalidation (M)

**Evidence.** [Parley](https://github.com/linebender/parley/blob/main/README.md) separates font fallback, shaping, bidi/segmentation, line layout and selection. Tekto already uses Parley and has fixed Roboto headless output; round 3 says shaping repeats during layout.

**Slice.** Instrument shaping and line breaking first. Prototype a bounded per-node cache in `tekto-text`/`tekto-layout`, keyed by text and font identity, size, styles, locale/direction when introduced, scale policy and available width. If Parley's actual API supports safe reuse, separate width-independent shaping from width-dependent wrapping; otherwise cache complete layouts and document that limit. Invalidate on text, font/fallback configuration, style, width, scale or node removal. Keep caches out of serialized trees and avoid an unbounded cache for large or frequently changing strings.

**Done when.** Tests compare cached and uncached line breaks, glyph/selection geometry and CPU pixels for edits, width changes, fallback fonts, mixed direction and scale changes. A resize benchmark shows cache hits, memory ceiling and before/after p50/p95; a deliberate cache disable remains available for diagnosis. This is a proposal, not a claim that Parley exposes every desired cache boundary today.

**Current code and alternatives.** `layout_tree_with_text` builds a new `TaffyTree` and node map for each layout, measures leaves through `TextEngine`, then places carets and character geometry. The text engine has a font source cache; that is not evidence of a reusable layout cache. Compare (a) no cache, (b) bounded text-result caching with the existing Taffy rebuild, and only then (c) a persistent Taffy tree with explicit insert/remove/style invalidation. [Taffy's own benchmark notes](https://github.com/DioxusLabs/taffy/blob/main/README.md#benchmarks-vs-yoga) exclude tree creation and text, so their numbers cannot be used as Tekto frame-time estimates. A persistent mapping introduces stale-node, overlay and reordered-child risks; reject it if measured savings do not justify that complexity.

## 3. Make damage tracking an experiment, not the next default (L)

**Evidence.** The current roadmap says the full scene is painted each frame. A retained tree makes dirty regions plausible, but transparency, clips, text antialiasing, overlays, animation, resizing and old/new bounds make partial redraw correctness difficult.

**Slice.** Start with paint invalidation reasons and counters, not partial presentation. Prototype CPU damage as the union of old and new bounds, expanded for strokes and antialiasing, clipped to the viewport. Force full repaint on resize, unknown effects, global changes or uncertain damage. Only investigate GPU/swapchain partial presentation after measuring a win; no assumption that a newly acquired surface contains the previous frame.

**Done when.** Identical scripted actions produce the same pixels as a full repaint over overlapping translucent nodes, scrolling, moving overlays, animation, text edits and resize. Report damaged area, CPU cost and memory, and retain a full-render fallback.

## 4. Test the semantic tree and host separately (M)

[AccessKit](https://github.com/AccessKit/accesskit/blob/main/README.md) describes a full initial tree followed by incremental updates and platform action requests. Tekto already maps its scene into AccessKit and has an optional OS adapter; do not add a second agent tree. Add an inspector view of the existing node id, role, name, bounds, state, focus and supported actions, and highlight the corresponding scene bounds. Show when a row is unrealized instead of pretending an off-screen node exists.

Headless tests should assert stable ids, role/name, focus, action and update sequences after patch, scroll, removal and overlay dismissal. A separate Linux AT-SPI session should read, activate, toggle, type and observe focus events in a real desktop session. Windows UIA, Android and browser checks remain separate platform evidence. See [feature proposals](feature-research.md) for focus rules and [round 3](round3-research.md) for unverified paths.

## 5. Reproducible performance and parity matrix (S for harness; M for device coverage)

Use the same workload and build profile for before/after; record commit, OS, CPU, GPU/driver, backend, scale, viewport, build flags, warmup, sample count and unavailable features. Keep app text and user data out of shared traces. Capture baseline and candidate on the same machine; report distributions and absolute values, not a single best frame. Suggested workloads:

| Workload | Metrics | Correctness oracle |
| --- | --- | --- |
| Notes open, idle, edit one field | startup to first frame, idle redraw count, edit-to-frame p50/p95, memory | same text, focus, semantic value and CPU image |
| Resize a text-heavy split pane at fixed recorded sizes | layout/measure calls, per-stage p50/p95, maximum frame, cache hits and memory | same line breaks, caret bounds and final viewport |
| Scroll 1,000 fixed-height keys | realized node count, sync/layout/paint time, allocations | same key order, focus rule and semantic range |
| Animate overlapping translucent nodes | CPU time, damage area if enabled, GPU submit and separately measured completion | full-render pixels versus experimental path |
| Surface loss/minimize/restore | acquisition result, retry count, successful recovery, CPU fallback occurrence | no stale surface present, no unbounded retry, final frame at current size |

Numbers such as 16.7 ms are illustrative 60 Hz budgets, not an established Tekto baseline or an acceptance threshold for every machine. Agree thresholds only after baselines exist. Keep GPU smoke tests optional and report adapter name and whether the test skipped; a software adapter is useful for coverage but is not evidence of hardware-driver performance. Existing headless snapshots remain CPU and deterministic; native GPU pixel comparisons need tolerances and driver-specific reporting rather than reblessing CPU snapshots.

## 6. Live editing, identity and replay (M)

Tekto's JSX `For` reuses entries by application key and disposes removed entries; the scene has separate node ids. The explicit version-1 `Recording` in `crates/tekto-tree/src/recording.rs` stores selected tree actions and requires the same initial tree and a layout callback. It does not capture host callbacks, actual clock time, IME lifecycle, native calls or a full UI session. [Xilem's separation of view and retained widget state](https://github.com/linebender/xilem/blob/main/README.md) is useful architectural context, not a prescription to add another tree.

Prototype a dev-only event log with a version, monotonic sequence, input kind, target id/key when available, and explicit clock advancement. Keep existing v1 replay readable; reject unsupported versions instead of silently guessing. Redact field values by default, require explicit opt-in for captured text, bound memory and surface nondeterministic native results as fixtures. Test reload while a patch, async completion or pointer capture is pending; distinguish runtime patch success from source writeback success. A replay should report the first divergent event and semantic assertion, with optional CPU screenshot, while remaining honest about which side effects are simulated.
