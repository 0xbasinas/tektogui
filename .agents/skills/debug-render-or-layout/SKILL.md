---
name: debug-render-or-layout
description: Debug Tekto layout, text, or CPU painting (wrong bounds, clipped or missing text, bad colors, blank screenshots). Use when a headless PNG or window looks wrong, or when Taffy/Parley/tiny-skia code fails to compile.
---

# Debug render or layout

Pipeline: scene tree → `layout_tree` (Taffy) → `paint_tree` / `CpuRenderer` (tiny-skia + Skrifa outlines) → PNG or softbuffer. Text is Parley with bundled Roboto only (`system_fonts: false`).

The optional `gpu` feature swaps the desktop window to `GpuRenderer` (wgpu + Vello). It uses the same `paint_tree` calls, then reads the texture back for softbuffer. Official headless PNGs (`render_tree_png`) stay on the CPU. If GPU init fails, the window falls back to CPU and says so on stderr. Check it with `cargo test -p tekto-render --features gpu --lib gpu_paints` or `cargo run -p notes --features gpu`. Do not turn `gpu` on for default tests.

## Where to look

- Bounds and tree dump: `crates/tekto-tree` (`layout`, `bounds`, `to_pretty_text`, `hit`)
- Flex layout: `crates/tekto-layout/src/lib.rs` — window uses padding/gap; text nodes are measured with Parley (`layout_tree_with_text` / a thread-local `TextEngine`)
- Text: `crates/tekto-text/src/lib.rs` — `FontFamily::named("Roboto")`, `Blob::new(Arc::new(bytes.to_vec()))`, `register_fonts(blob, None)`
- Paint: `crates/tekto-render/src/cpu.rs` — glyph y is flipped (`self.y - y`); `run.font()` is `FontData` (`.data`, `.index`); `NormalizedCoord::from_bits(c.to_bits())`
- Window blit: `crates/tekto-platform/src/desktop.rs` and `rgba_to_u32_softbuffer` in `lib.rs`
- Headless entry: `tekto::render_headless_to_file`

## Steps

1. Dump the tree after layout and check `x,y,w,h` in `to_pretty_text()` (or a debugger on `node.bounds`).
2. Render a PNG:

```bash
cargo run -p tekto-cli -- dev --headless --out /tmp/tekto-debug.png
# or
TEKTO_SHOT_DIR=/tmp/tekto-shots cargo test -p tekto --test headless_snapshot -- --nocapture
```

3. If text is missing, first run `cargo test -p tekto-text`. Confirm the font file is `crates/tekto-text/fonts/Roboto-Regular.ttf`. Do not re-enable Parley's `system` / fontconfig feature.
4. If only the live window is blank but the PNG is fine, look at resize/scale-factor and the softbuffer present path in `desktop.rs`. On this box, use `DISPLAY=:99` (Xvfb), never `:1` or `:2`. A grab of the Xvfb *root* may be black if no window manager is running; trust the headless PNG or `GET /screenshot`.
5. Compile errors in font/glyph code: keep `skrifa = "0.44"` (not a newer workspace pin).

## Verify

```bash
cargo test -p tekto-layout -p tekto-text -p tekto-render -p tekto --all-targets
```

The Notes PNG should show a dark banner with “Notes” and “Hello from Tekto” on a warm off-white page.
