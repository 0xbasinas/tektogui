//! Renderer trait, a tiny-skia CPU backend, and an optional wgpu / Vello backend.
//!
//! The CPU backend is the default. It is also the path used for headless
//! screenshots and tests. The GPU backend is behind the `gpu` feature so
//! everyday builds do not compile wgpu or Vello.

use tekto_tree::{Color, NodeKind, Rect, Tree};

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("{0}")]
    Message(String),
    #[error("gpu backend is not enabled (build with the gpu feature later)")]
    GpuDisabled,
}

impl RenderError {
    pub fn msg(msg: impl Into<String>) -> Self {
        RenderError::Message(msg.into())
    }
}

/// What the window should do when a GPU readback or present fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentFailure {
    /// The native window or swapchain is gone. Do not lock or present it.
    SurfaceLost,
    /// Paint again with the CPU renderer. The window may still be valid.
    UseCpu,
}

/// Classify a present or readback error.
///
/// `SurfaceLost` covers wgpu's "surface lost" / "surface outdated" and a
/// destroyed Android `ANativeWindow`. Anything else (no adapter, device
/// lost, map timeout) is [`PresentFailure::UseCpu`]. The caller must still
/// skip present when the surface epoch is already retired.
pub fn classify_present_failure(message: &str) -> PresentFailure {
    let lower = message.to_ascii_lowercase();
    if lower.contains("surface lost")
        || lower.contains("surface outdated")
        || lower.contains("anativewindow")
        || lower.contains("surface is no longer valid")
    {
        PresentFailure::SurfaceLost
    } else {
        PresentFailure::UseCpu
    }
}

/// Backend-agnostic draw API.
pub trait Renderer {
    fn resize(&mut self, width: u32, height: u32) -> Result<(), RenderError>;
    /// Start a fresh frame, clearing the surface and any active clips.
    fn begin_frame(&mut self, clear: Color);
    fn fill_rect(&mut self, rect: Rect, color: Color);
    fn draw_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        max_width: Option<f32>,
    );
    /// Encode the current frame as PNG bytes.
    fn to_png(&self) -> Result<Vec<u8>, RenderError>;
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    /// Tight, packed RGBA8 pixels (row-major) for presenting to a window.
    fn rgba8(&self) -> Result<Vec<u8>, RenderError>;
    /// Clip later draws to `rect` (intersected with any clip already active).
    fn push_clip(&mut self, _rect: Rect) {}
    fn pop_clip(&mut self) {}
    /// Paint validated RGBA pixels inside a viewport. Other backends may omit images.
    fn draw_image(
        &mut self,
        _image: &tekto_image::Image,
        _bounds: Rect,
        _fit: tekto_image::Fit,
        _opacity: f32,
    ) {
    }
    /// Fill a rectangle whose corners are rounded by `radius` logical pixels.
    /// Backends without path support fall back to a square [`Renderer::fill_rect`].
    fn fill_rrect(&mut self, rect: Rect, radius: f32, color: Color) {
        let _ = radius;
        self.fill_rect(rect, color);
    }
    /// Stroke the inside edge of `rect` with `width`-thick lines. Falls back
    /// to four filled edge strips.
    fn stroke_rrect(&mut self, rect: Rect, radius: f32, width: f32, color: Color) {
        let w = width.max(0.0);
        if w <= 0.0 {
            return;
        }
        let _ = radius;
        let (x, y, rw, rh) = (rect.x, rect.y, rect.width, rect.height);
        self.fill_rect(Rect::new(x, y, rw, w), color);
        self.fill_rect(Rect::new(x, y + rh - w, rw, w), color);
        self.fill_rect(Rect::new(x, y, w, rh), color);
        self.fill_rect(Rect::new(x + rw - w, y, w, rh), color);
    }
    /// Draw a straight line from `(x0, y0)` to `(x1, y1)` with a round-ish
    /// `thickness`. Falls back to small filled steps along the line.
    fn draw_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, thickness: f32, color: Color) {
        let thickness = thickness.max(0.0);
        let dx = x1 - x0;
        let dy = y1 - y0;
        let steps = ((dx.abs() + dy.abs()) / thickness.max(1.0)).ceil().max(1.0) as u32;
        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            let cx = x0 + dx * t;
            let cy = y0 + dy * t;
            self.fill_rect(
                Rect::new(
                    cx - thickness / 2.0,
                    cy - thickness / 2.0,
                    thickness,
                    thickness,
                ),
                color,
            );
        }
    }
}

/// Paint a whole tree with the default look.
pub fn paint_tree<R: Renderer>(renderer: &mut R, tree: &Tree, clear: Color) {
    renderer.begin_frame(clear);
    if let Some(root) = tree.root() {
        paint_node(renderer, tree, root);
        if let Some(overlay) = tree.overlay() {
            if tree
                .get(overlay.node)
                .is_some_and(|node| node.kind == NodeKind::Dialog)
            {
                if let Some(window) = tree.get(root) {
                    renderer.fill_rect(window.bounds, Color::rgba(0, 0, 0, 96));
                }
            }
            paint_node(renderer, tree, overlay.node);
        }
    }
}

fn paint_node<R: Renderer>(renderer: &mut R, tree: &Tree, id: tekto_tree::NodeId) {
    paint_node_opacity(renderer, tree, id, 1.0);
}

fn with_opacity(color: Color, opacity: f32) -> Color {
    let opacity = opacity.clamp(0.0, 1.0);
    Color {
        a: (color.a as f32 * opacity).round() as u8,
        ..color
    }
}

fn paint_node_opacity<R: Renderer>(
    renderer: &mut R,
    tree: &Tree,
    id: tekto_tree::NodeId,
    parent_opacity: f32,
) {
    let Some(node) = tree.get(id).filter(|_| tree.is_visible(id)) else {
        return;
    };
    let opacity = (node.opacity
        * parent_opacity
        * if node.disabled
            && matches!(
                node.kind,
                NodeKind::Button
                    | NodeKind::TextField
                    | NodeKind::Checkbox
                    | NodeKind::Switch
                    | NodeKind::Slider
                    | NodeKind::Radio
                    | NodeKind::Tab
                    | NodeKind::MenuItem
                    | NodeKind::Select
            )
        {
            0.5
        } else {
            1.0
        })
    .clamp(0.0, 1.0);
    if opacity <= 0.001 {
        return;
    }
    if let Some(fill) = node.fill {
        if node.radius > 0.0 {
            renderer.fill_rrect(node.bounds, node.radius, with_opacity(fill, opacity));
        } else {
            renderer.fill_rect(node.bounds, with_opacity(fill, opacity));
        }
    }
    if node.kind == NodeKind::Image {
        if let Some(image) = &node.image {
            renderer.draw_image(image, node.bounds, node.image_fit, opacity);
        }
    }
    if let Some(border) = node.border_color {
        let width = node.border_width;
        if width > 0.0 {
            if node.radius > 0.0 {
                renderer.stroke_rrect(
                    node.bounds,
                    node.radius,
                    width,
                    with_opacity(border, opacity),
                );
            } else {
                renderer.stroke_rrect(node.bounds, 0.0, width, with_opacity(border, opacity));
            }
        }
    }
    if tree.focus == Some(id)
        && matches!(
            node.kind,
            tekto_tree::NodeKind::Button
                | tekto_tree::NodeKind::MenuItem
                | tekto_tree::NodeKind::Select
                | tekto_tree::NodeKind::Checkbox
                | tekto_tree::NodeKind::Radio
                | tekto_tree::NodeKind::Tab
                | tekto_tree::NodeKind::Switch
                | tekto_tree::NodeKind::Slider
                | tekto_tree::NodeKind::TextField
        )
    {
        let b = node.bounds;
        let color = with_opacity(tree.tokens().focus, opacity);
        let thickness = 2.0_f32.min(b.width / 2.0).min(b.height / 2.0).max(0.0);
        for rect in [
            Rect::new(b.x, b.y, b.width, thickness),
            Rect::new(b.x, b.y + b.height - thickness, b.width, thickness),
            Rect::new(b.x, b.y, thickness, b.height),
            Rect::new(b.x + b.width - thickness, b.y, thickness, b.height),
        ] {
            renderer.fill_rect(rect, color);
        }
    }
    if node.kind == NodeKind::Checkbox {
        let x = node.bounds.x + node.layout.padding;
        let y = node.bounds.y + (node.bounds.height - 18.0) / 2.0;
        let ink = with_opacity(node.color.unwrap_or(tree.tokens().ink), opacity);
        renderer.fill_rect(Rect::new(x, y, 18.0, 18.0), ink);
        renderer.fill_rect(
            Rect::new(x + 2.0, y + 2.0, 14.0, 14.0),
            with_opacity(tree.tokens().page, opacity),
        );
        if node.checked {
            renderer.fill_rect(Rect::new(x + 5.0, y + 5.0, 8.0, 8.0), ink);
        }
    }
    if node.kind == NodeKind::Tab && node.checked {
        let b = node.bounds;
        renderer.fill_rect(
            Rect::new(
                b.x,
                b.y + (b.height - 2.0).max(0.0),
                b.width,
                2.0_f32.min(b.height),
            ),
            with_opacity(node.color.unwrap_or(tree.tokens().ink), opacity),
        );
    }
    if node.kind == NodeKind::Radio {
        let x = node.bounds.x + node.layout.padding;
        let y = node.bounds.y + (node.bounds.height - 18.0) / 2.0;
        let ink = with_opacity(node.color.unwrap_or(tree.tokens().ink), opacity);
        renderer.fill_rrect(Rect::new(x, y, 18.0, 18.0), 9.0, ink);
        renderer.fill_rrect(
            Rect::new(x + 2.0, y + 2.0, 14.0, 14.0),
            7.0,
            with_opacity(tree.tokens().page, opacity),
        );
        if node.checked {
            renderer.fill_rrect(Rect::new(x + 5.0, y + 5.0, 8.0, 8.0), 4.0, ink);
        }
    }
    if node.kind == NodeKind::Slider {
        let b = node.bounds;
        let x = b.x + 8.0 + (b.width - 16.0).max(0.0) * node.progress.clamp(0.0, 1.0);
        renderer.fill_rrect(
            Rect::new(x - 8.0, b.y + (b.height - 16.0) / 2.0, 16.0, 16.0),
            8.0,
            with_opacity(node.color.unwrap_or(tree.tokens().ink), opacity),
        );
    }
    if node.kind == NodeKind::Progress && node.progress > 0.0 {
        let b = node.bounds;
        let width = b.width * node.progress.clamp(0.0, 1.0);
        renderer.fill_rrect(
            Rect::new(b.x, b.y, width, b.height),
            node.radius.min(width / 2.0),
            with_opacity(node.color.unwrap_or(tree.tokens().ink), opacity),
        );
    }
    if node.kind == NodeKind::Switch {
        let x = node.bounds.x + node.layout.padding;
        let y = node.bounds.y + (node.bounds.height - 18.0) / 2.0;
        let ink = with_opacity(node.color.unwrap_or(tree.tokens().ink), opacity);
        renderer.fill_rrect(Rect::new(x, y, 32.0, 18.0), 9.0, ink);
        renderer.fill_rrect(
            Rect::new(
                x + if node.checked { 16.0 } else { 2.0 },
                y + 2.0,
                14.0,
                14.0,
            ),
            7.0,
            with_opacity(tree.tokens().page, opacity),
        );
    }
    if node.kind == NodeKind::TextField && tree.focus == Some(id) {
        if !node.preedit.is_empty() {
            renderer.push_clip(node.bounds);
            for selection in &node.preedit_selection_rects {
                renderer.fill_rect(
                    Rect::new(
                        node.bounds.x + node.layout.padding + selection.x,
                        node.bounds.y + node.layout.padding + selection.y
                            - if node.multiline {
                                node.text_scroll_y
                            } else {
                                0.0
                            },
                        selection.width,
                        selection.height,
                    ),
                    with_opacity(Color::rgba(125, 168, 222, 150), opacity),
                );
            }
            renderer.pop_clip();
        }
        if node.multiline {
            let b = node.bounds;
            let inset = node.layout.padding;
            renderer.push_clip(b);
            for selection in &node.selection_rects {
                renderer.fill_rect(
                    Rect::new(
                        b.x + inset + selection.x,
                        b.y + inset + selection.y - node.text_scroll_y,
                        selection.width,
                        selection.height,
                    ),
                    with_opacity(Color::rgba(125, 168, 222, 150), opacity),
                );
            }
            renderer.pop_clip();
        }
        if let Some((start, end)) = node.selection_x {
            let inset = node.layout.padding;
            let b = node.bounds;
            renderer.push_clip(b);
            renderer.fill_rect(
                Rect::new(
                    b.x + inset + start,
                    b.y + inset,
                    (end - start).max(0.0),
                    (b.height - 2.0 * inset).max(0.0),
                ),
                with_opacity(Color::rgba(125, 168, 222, 150), opacity),
            );
            renderer.pop_clip();
        }
    }
    let composed;
    let mut placeholder = false;
    let text_to_draw = if node.kind == tekto_tree::NodeKind::TextField {
        composed = node.composed_text();
        if composed.is_empty() {
            placeholder = true;
            node.name.as_deref().filter(|name| !name.is_empty())
        } else {
            Some(composed.as_str())
        }
    } else {
        node.text.as_deref()
    };
    if let Some(text) = text_to_draw {
        let color = with_opacity(
            node.color.unwrap_or(tree.tokens().ink),
            opacity * if placeholder { 0.55 } else { 1.0 },
        );
        let inset = if matches!(
            node.kind,
            tekto_tree::NodeKind::Button
                | tekto_tree::NodeKind::MenuItem
                | tekto_tree::NodeKind::Select
                | tekto_tree::NodeKind::Checkbox
                | tekto_tree::NodeKind::Radio
                | tekto_tree::NodeKind::Tab
                | tekto_tree::NodeKind::Switch
                | tekto_tree::NodeKind::TextField
                | tekto_tree::NodeKind::ListItem
        ) {
            node.layout.padding
        } else {
            0.0
        };
        let max_w = if node.bounds.width > 0.0 {
            Some(
                (node.bounds.width
                    - 2.0 * inset
                    - if matches!(node.kind, NodeKind::Checkbox | NodeKind::Radio) {
                        28.0
                    } else if node.kind == NodeKind::Switch {
                        42.0
                    } else {
                        0.0
                    })
                .max(0.0),
            )
        } else {
            None
        };
        if node.kind == NodeKind::TextField && node.multiline {
            renderer.push_clip(node.bounds);
        }
        renderer.draw_text(
            text,
            node.bounds.x
                + inset
                + if matches!(node.kind, NodeKind::Checkbox | NodeKind::Radio) {
                    28.0
                } else if node.kind == NodeKind::Switch {
                    42.0
                } else {
                    0.0
                },
            node.bounds.y + inset
                - if node.multiline {
                    node.text_scroll_y
                } else {
                    0.0
                },
            node.font_size,
            color,
            max_w,
        );
        if node.kind == NodeKind::TextField && node.multiline {
            renderer.pop_clip();
        }
    }
    if node.kind == NodeKind::TextField && tree.focus == Some(id) {
        if node.multiline && !node.preedit.is_empty() {
            let b = node.bounds;
            let inset = node.layout.padding;
            renderer.push_clip(b);
            for range in &node.preedit_rects {
                renderer.fill_rect(
                    Rect::new(
                        b.x + inset + range.x,
                        b.y + inset + range.y + range.height - node.text_scroll_y - 1.0,
                        range.width,
                        1.0,
                    ),
                    with_opacity(node.color.unwrap_or(tree.tokens().ink), opacity),
                );
            }
            renderer.pop_clip();
        }
        if let Some((start, end)) = node.preedit_x.filter(|_| !node.preedit.is_empty()) {
            let b = node.bounds;
            let inset = node.layout.padding;
            renderer.push_clip(b);
            renderer.fill_rect(
                Rect::new(
                    b.x + inset + start,
                    b.y + b.height - inset - 1.0,
                    (end - start).max(0.0),
                    1.0,
                ),
                with_opacity(node.color.unwrap_or(tree.tokens().ink), opacity),
            );
            renderer.pop_clip();
        }
    }
    if node.kind == tekto_tree::NodeKind::TextField
        && tree.focus == Some(id)
        && tree.caret_visible()
        && node.composed_caret().is_some()
    {
        let inset = node.layout.padding;
        let h = if node.multiline {
            node.caret_height.max(1.0)
        } else {
            (node.bounds.height - 2.0 * inset).max(1.0)
        };
        if node.multiline {
            renderer.push_clip(node.bounds);
        }
        renderer.fill_rect(
            tekto_tree::Rect::new(
                node.bounds.x + inset + node.caret_x,
                node.bounds.y
                    + inset
                    + if node.multiline {
                        node.caret_y - node.text_scroll_y
                    } else {
                        0.0
                    },
                1.0,
                h,
            ),
            with_opacity(node.color.unwrap_or(tree.tokens().ink), opacity),
        );
        if node.multiline {
            renderer.pop_clip();
        }
    }
    let clip = node.kind == tekto_tree::NodeKind::Scroll;
    if clip {
        renderer.push_clip(node.bounds);
    }
    for child in &node.children {
        if node.kind == NodeKind::Select {
            continue;
        }
        if tree.overlay().is_some_and(|o| o.node == *child) {
            continue;
        }
        paint_node_opacity(renderer, tree, *child, opacity);
    }
    if clip {
        renderer.pop_clip();
    }
}

#[cfg(test)]
mod present_failure_tests {
    use super::{classify_present_failure, PresentFailure};

    #[test]
    fn lost_surface_is_not_presented() {
        assert_eq!(
            classify_present_failure("surface lost"),
            PresentFailure::SurfaceLost
        );
        assert_eq!(
            classify_present_failure("wgpu: surface outdated"),
            PresentFailure::SurfaceLost
        );
        assert_eq!(
            classify_present_failure("ANativeWindow was destroyed"),
            PresentFailure::SurfaceLost
        );
    }

    #[test]
    fn other_gpu_errors_fall_back_to_cpu() {
        assert_eq!(
            classify_present_failure("no GPU adapter: not found"),
            PresentFailure::UseCpu
        );
        assert_eq!(
            classify_present_failure("GPU device: lost"),
            PresentFailure::UseCpu
        );
    }
}

#[cfg(feature = "cpu")]
mod cpu;
#[cfg(feature = "cpu")]
pub use cpu::CpuRenderer;

#[cfg(feature = "gpu")]
mod gpu;
#[cfg(feature = "gpu")]
pub use gpu::GpuRenderer;
#[cfg(all(feature = "gpu", target_arch = "wasm32"))]
pub use gpu::{paint_tree_webgpu, WebCanvas};

/// Placeholder used when the crate is built without the `gpu` feature.
#[cfg(not(feature = "gpu"))]
pub struct GpuRenderer;

#[cfg(not(feature = "gpu"))]
impl GpuRenderer {
    pub fn new(_width: u32, _height: u32) -> Result<Self, RenderError> {
        Err(RenderError::GpuDisabled)
    }
}

#[cfg(feature = "cpu")]
pub fn render_tree_png(
    tree: &Tree,
    width: u32,
    height: u32,
    clear: Color,
) -> Result<Vec<u8>, RenderError> {
    render_tree_png_scaled(tree, width, height, 1.0, clear)
}

/// Like [`render_tree_png`], but the pixmap is `width * scale` by
/// `height * scale` physical pixels. Glyphs are rasterized at the
/// physical size so HiDPI text stays sharp.
pub fn render_tree_png_scaled(
    tree: &Tree,
    width: u32,
    height: u32,
    scale: f32,
    clear: Color,
) -> Result<Vec<u8>, RenderError> {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let pw = ((width as f32) * scale).round().max(1.0) as u32;
    let ph = ((height as f32) * scale).round().max(1.0) as u32;
    let mut renderer = CpuRenderer::new(pw, ph)?;
    renderer.set_scale(scale);
    paint_tree(&mut renderer, tree, clear);
    renderer.to_png()
}

#[cfg(all(test, feature = "cpu"))]
mod tests {
    use super::*;
    use tekto_tree::{Color, Rect};

    #[test]
    fn cpu_clips_rounded_borders_and_resets_clips_each_frame() {
        let mut renderer = CpuRenderer::new(32, 24).unwrap();
        renderer.begin_frame(Color::PAGE);
        renderer.push_clip(Rect::new(0.0, 0.0, 16.0, 24.0));
        renderer.stroke_rrect(Rect::new(2.0, 2.0, 28.0, 20.0), 4.0, 3.0, Color::INK);
        let pixels = renderer.rgba8().unwrap();
        let pixel = |x: usize, y: usize| &pixels[(y * 32 + x) * 4..(y * 32 + x + 1) * 4];
        assert_eq!(
            pixel(8, 3),
            &[Color::INK.r, Color::INK.g, Color::INK.b, 255]
        );
        assert_eq!(
            pixel(24, 3),
            &[Color::PAGE.r, Color::PAGE.g, Color::PAGE.b, 255]
        );

        renderer.begin_frame(Color::PAGE);
        renderer.fill_rect(Rect::new(0.0, 0.0, 32.0, 24.0), Color::INK);
        let pixels = renderer.rgba8().unwrap();
        let offset = (12 * 32 + 24) * 4;
        assert_eq!(
            &pixels[offset..offset + 4],
            &[Color::INK.r, Color::INK.g, Color::INK.b, 255]
        );
    }

    #[test]
    fn tokens_drive_new_nodes_and_the_ink_fallback() {
        use tekto_tree::{NodeKind, Tokens, Tree};
        let mut tree = Tree::new();
        let tokens = Tokens {
            page: Color::rgb(10, 20, 30),
            ink: Color::rgb(200, 10, 10),
            ..Tokens::default()
        };
        tree.set_tokens(tokens);
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().bounds = Rect::new(0.0, 0.0, 20.0, 20.0);
        assert_eq!(tree.get(root).unwrap().fill, Some(Color::rgb(10, 20, 30)));
        let mut renderer = CpuRenderer::new(20, 20).unwrap();
        paint_tree(&mut renderer, &tree, Color::PAGE);
        let pixels = renderer.rgba8().unwrap();
        assert_eq!(&pixels[0..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn repeated_image_frames_reuse_premultiplied_pixels() {
        let image = tekto_image::Image::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
        let mut renderer = CpuRenderer::new(8, 8).unwrap();
        let bounds = Rect::new(2.0, 2.0, 4.0, 4.0);
        for _ in 0..3 {
            renderer.begin_frame(Color::PAGE);
            renderer.draw_image(&image, bounds, tekto_image::Fit::Contain, 1.0);
            renderer.rgba8().unwrap();
        }
        assert_eq!(renderer.image_cache_hits(), 2);
        // A different buffer with the same bytes must not reuse the cache.
        let other = tekto_image::Image::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
        renderer.draw_image(&other, bounds, tekto_image::Fit::Contain, 1.0);
        assert_eq!(renderer.image_cache_hits(), 2);
    }

    #[test]
    fn animation_frames_differ_and_stay_cheap() {
        use std::time::Instant;
        use tekto_tree::{NodeKind, StyleTransition, Tree};

        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().bounds = Rect::new(0.0, 0.0, 200.0, 120.0);
        tree.get_mut(root).unwrap().fill = Some(Color::PAGE);
        let mut ids = Vec::new();
        for i in 0..3 {
            let id = tree.add(NodeKind::Rect, Some(root)).unwrap();
            let node = tree.get_mut(id).unwrap();
            node.bounds = Rect::new(10.0 + i as f32 * 24.0, 20.0, 20.0, 20.0);
            node.fill = Some(Color::INK);
            ids.push(id);
        }
        for id in &ids {
            tree.transition(
                *id,
                StyleTransition {
                    opacity: Some(0.15),
                    x: Some(tree.get(*id).unwrap().bounds.x + 30.0),
                    duration_ms: Some(180.0),
                    ..StyleTransition::default()
                },
            )
            .unwrap();
        }

        let mut frames = Vec::new();
        let started = Instant::now();
        for step in 0..12 {
            tree.advance(step as f64 * (180.0 / 11.0));
            let png = render_tree_png(&tree, 200, 120, Color::PAGE).unwrap();
            assert!(png.starts_with(b"\x89PNG"));
            frames.push(png);
        }
        let elapsed = started.elapsed();
        eprintln!("cpu animation: 12 frames, 3 rects, 200x120, {:?}", elapsed);
        // Full tiny-skia paints, no dirty rects. Debug builds still finish
        // a short scene well under a second on CI. See docs/architecture.md.
        assert!(
            elapsed.as_millis() < 2000,
            "cpu animation cost too high: {elapsed:?}"
        );
        assert_ne!(frames[0], frames[6], "mid frame should move");
        assert_ne!(frames[0], frames[11]);

        let shot_dir = std::path::PathBuf::from("/workspace/tektogui-shots");
        if shot_dir.is_dir() || std::fs::create_dir_all(&shot_dir).is_ok() {
            let _ = std::fs::write(shot_dir.join("i38-anim-start.png"), &frames[0]);
            let _ = std::fs::write(shot_dir.join("i38-anim-mid.png"), &frames[6]);
            let _ = std::fs::write(shot_dir.join("i38-anim-end.png"), &frames[11]);
        }
    }

    #[test]
    fn focused_controls_have_visible_outline() {
        use tekto_tree::NodeKind;
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        for kind in [NodeKind::Button, NodeKind::TextField] {
            let id = tree.add(kind, Some(root)).unwrap();
            tree.get_mut(id).unwrap().bounds = Rect::new(5.0, 5.0, 40.0, 20.0);
            let mut renderer = CpuRenderer::new(50, 30).unwrap();
            paint_tree(&mut renderer, &tree, Color::PAGE);
            let before = renderer.rgba8().unwrap();
            tree.set_focus(id).unwrap();
            paint_tree(&mut renderer, &tree, Color::PAGE);
            let after = renderer.rgba8().unwrap();
            let pixel = (5 * 50 + 5) * 4;
            assert_ne!(&before[pixel..pixel + 4], &after[pixel..pixel + 4]);
            tree.set_focus(root).unwrap();
            tree.remove(id).unwrap();
        }
    }

    #[test]
    fn paints_logical_rect_at_physical_scale() {
        let mut renderer = CpuRenderer::new(200, 100).unwrap();
        renderer.set_scale(2.0);
        renderer.begin_frame(Color::PAGE);
        // 80x40 logical at scale 2 -> 160x80 physical, inside a 200x100 pixmap.
        renderer.fill_rect(Rect::new(0.0, 0.0, 80.0, 40.0), Color::BANNER);
        let rgba = renderer.rgba8().unwrap();
        // Logical (10, 10) is physical (20, 20).
        let i = ((20 * 200 + 20) * 4) as usize;
        assert_eq!(
            &rgba[i..i + 3],
            &[Color::BANNER.r, Color::BANNER.g, Color::BANNER.b]
        );
        // To the right of the scaled rect (physical x=180).
        let j = ((20 * 200 + 180) * 4) as usize;
        assert_eq!(
            &rgba[j..j + 3],
            &[Color::PAGE.r, Color::PAGE.g, Color::PAGE.b]
        );
        assert_eq!(renderer.width(), 200);
        assert_eq!(renderer.height(), 100);
    }
}

#[cfg(test)]
mod field_hint_tests {
    use super::*;
    use tekto_tree::NodeKind;

    #[derive(Default)]
    struct Recorder(Vec<(String, Color)>);
    impl Renderer for Recorder {
        fn resize(&mut self, _: u32, _: u32) -> Result<(), RenderError> {
            Ok(())
        }
        fn begin_frame(&mut self, _: Color) {}
        fn fill_rect(&mut self, _: Rect, _: Color) {}
        fn draw_text(&mut self, text: &str, _: f32, _: f32, _: f32, color: Color, _: Option<f32>) {
            self.0.push((text.to_owned(), color));
        }
        fn to_png(&self) -> Result<Vec<u8>, RenderError> {
            Ok(Vec::new())
        }
        fn width(&self) -> u32 {
            0
        }
        fn height(&self) -> u32 {
            0
        }
        fn rgba8(&self) -> Result<Vec<u8>, RenderError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn empty_name_is_quiet_and_value_and_preedit_replace_it() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        let node = tree.get_mut(field).unwrap();
        node.name = Some("Title".into());
        node.color = Some(Color::INK);
        let mut recorder = Recorder::default();
        paint_tree(&mut recorder, &tree, Color::PAGE);
        assert_eq!(recorder.0, [("Title".into(), Color::rgba(44, 51, 58, 140))]);
        tree.get_mut(field).unwrap().text = Some("Hello".into());
        recorder.0.clear();
        paint_tree(&mut recorder, &tree, Color::PAGE);
        assert_eq!(recorder.0, [("Hello".into(), Color::INK)]);
        let node = tree.get_mut(field).unwrap();
        node.text = Some(String::new());
        node.preedit = "あ".into();
        recorder.0.clear();
        paint_tree(&mut recorder, &tree, Color::PAGE);
        assert_eq!(recorder.0, [("あ".into(), Color::INK)]);
        let node = tree.get_mut(field).unwrap();
        node.preedit.clear();
        node.name = None;
        recorder.0.clear();
        paint_tree(&mut recorder, &tree, Color::PAGE);
        assert!(recorder.0.is_empty());
    }
}

#[cfg(test)]
mod paint_primitives_tests {
    use super::*;
    use tekto_tree::{NodeKind, Tree};

    #[test]
    fn rounded_rect_border_and_line_pixels() {
        let mut tree = Tree::new();
        tree.add(NodeKind::Window, None).unwrap();
        let card = tree
            .add(NodeKind::Rect, Some(tree.root().unwrap()))
            .unwrap();
        tree.get_mut(card).unwrap().bounds = Rect::new(10.0, 10.0, 80.0, 60.0);
        tree.get_mut(card).unwrap().radius = 10.0;
        tree.get_mut(card).unwrap().fill = Some(Color::BANNER);
        tree.get_mut(card).unwrap().border_color = Some(Color::INK);
        let mut renderer = CpuRenderer::new(100, 80).unwrap();
        paint_tree(&mut renderer, &tree, Color::PAGE);
        let rgba = renderer.rgba8().unwrap();
        let at = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 100 + x) * 4;
            (rgba[i], rgba[i + 1], rgba[i + 2])
        };
        // Corner is cut, so the exact corner pixel stays page-colored.
        assert_eq!(at(10, 10), at(0, 0), "corner should be clear");
        // Center of the card is filled.
        let banner = at(60, 50);
        assert_ne!(banner, at(0, 0), "center should be filled");
        // Border ring right at the edge, away from corner rounding.
        let edge = at(10, 50);
        assert_ne!(edge, banner, "left edge should be the border color");
        assert_eq!(edge, at(89, 55), "symmetric edges match");

        // A line paints a diagonal run of pixels.
        let mut renderer = CpuRenderer::new(40, 40).unwrap();
        renderer.draw_line(2.0, 2.0, 30.0, 30.0, 2.0, Color::INK);
        let rgba = renderer.rgba8().unwrap();
        let mid = (20 * 40 + 20) * 4;
        assert_eq!(
            &rgba[mid..mid + 3],
            &[Color::INK.r, Color::INK.g, Color::INK.b]
        );
    }
}
