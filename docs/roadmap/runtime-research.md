# Runtime and rendering research

Research snapshot: 2026-11-11. Recommendations below build on [round 3](round3-research.md) and the [framework comparison](framework-research.md). Sizes are rough engineering estimates, not deadlines: S = days, M = one to two weeks, L = larger or cross-platform. Measure on representative machines before accepting any performance claim.

## 1. Trace resize and frame stages before changing presentation (S)

**Evidence.** Round 3 reports an intermittent thin black strip during resize and layout costs of 40–60 ms on some resize frames. `tekto-platform/src/desktop.rs` already has opt-in `TEKTO_FRAME_LOG`; `tekto-render/src/gpu.rs` has a direct window-surface path. The [wgpu example](https://github.com/gfx-rs/wgpu/blob/trunk/examples/features/src/hello_triangle/mod.rs) treats acquisition outcomes separately and reconfigures or redraws in several cases. That example is a comparison, not proof of the cause of Tekto's strip.

**Slice.** Extend opt-in diagnostics with a frame sequence, monotonic timestamp, logical size, scale, physical `inner_size`, renderer size, surface configuration, acquired texture size, outcome and present result. Separate input/queue, layout, paint recording, GPU submission and present timing where measurable; GPU completion needs an explicitly asynchronous measurement rather than being inferred from submit time. Bound the log and avoid text, paths or app data. Record adapter, backend, OS and driver in a user-supplied report.

**Done when.** A Windows reproduction compares these values on frames with and without the strip, including resize in each direction and mixed-DPI moves. Unit-test outcome-to-action decisions headlessly; verify real recovery on the target device. Do not assert a DWM, rounding, or swapchain cause until the trace supports it. Compare p50/p95 frame times and worst resize frame with logging disabled; record hardware, build profile and workload.

## 2. Reuse text work with explicit invalidation (M)

**Evidence.** [Parley](https://github.com/linebender/parley/blob/main/README.md) separates font fallback, shaping, bidi/segmentation, line layout and selection. Tekto already uses Parley and has fixed Roboto headless output; round 3 says shaping repeats during layout.

**Slice.** Instrument shaping and line breaking first. Prototype a bounded per-node cache in `tekto-text`/`tekto-layout`, keyed by text and font identity, size, styles, locale/direction when introduced, scale policy and available width. If Parley's actual API supports safe reuse, separate width-independent shaping from width-dependent wrapping; otherwise cache complete layouts and document that limit. Invalidate on text, font/fallback configuration, style, width, scale or node removal. Keep caches out of serialized trees and avoid an unbounded cache for large or frequently changing strings.

**Done when.** Tests compare cached and uncached line breaks, glyph/selection geometry and CPU pixels for edits, width changes, fallback fonts, mixed direction and scale changes. A resize benchmark shows cache hits, memory ceiling and before/after p50/p95; a deliberate cache disable remains available for diagnosis. This is a proposal, not a claim that Parley exposes every desired cache boundary today.

## 3. Make damage tracking an experiment, not the next default (L)

**Evidence.** The current roadmap says the full scene is painted each frame. A retained tree makes dirty regions plausible, but transparency, clips, text antialiasing, overlays, animation, resizing and old/new bounds make partial redraw correctness difficult.

**Slice.** Start with paint invalidation reasons and counters, not partial presentation. Prototype CPU damage as the union of old and new bounds, expanded for strokes and antialiasing, clipped to the viewport. Force full repaint on resize, unknown effects, global changes or uncertain damage. Only investigate GPU/swapchain partial presentation after measuring a win; no assumption that a newly acquired surface contains the previous frame.

**Done when.** Identical scripted actions produce the same pixels as a full repaint over overlapping translucent nodes, scrolling, moving overlays, animation, text edits and resize. Report damaged area, CPU cost and memory, and retain a full-render fallback.

## 4. Test the semantic tree and host separately (M)

[AccessKit](https://github.com/AccessKit/accesskit/blob/main/README.md) describes a full initial tree followed by incremental updates and platform action requests. Tekto already maps its scene into AccessKit and has an optional OS adapter; do not add a second agent tree. Add an inspector view of the existing node id, role, name, bounds, state, focus and supported actions, and highlight the corresponding scene bounds. Show when a row is unrealized instead of pretending an off-screen node exists.

Headless tests should assert stable ids, role/name, focus, action and update sequences after patch, scroll, removal and overlay dismissal. A separate Linux AT-SPI session should read, activate, toggle, type and observe focus events in a real desktop session. Windows UIA, Android and browser checks remain separate platform evidence. See [feature proposals](feature-research.md) for focus rules and [round 3](round3-research.md) for unverified paths.
