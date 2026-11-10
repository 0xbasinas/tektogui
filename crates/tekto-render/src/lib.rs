//! Renderer trait, a tiny-skia CPU backend, and a reserved GPU slot.
//!
//! The CPU backend is the default. It is also the path used for headless
//! screenshots, tests, and (later) a canvas2d-style web fallback.
//! The GPU backend (wgpu / Vello, and WebGPU on the web) is a feature
//! and is not pulled in until we need it, so everyday builds stay fast.

use tekto_tree::{Color, Rect, Tree};

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

/// Backend-agnostic draw API.
pub trait Renderer {
    fn resize(&mut self, width: u32, height: u32) -> Result<(), RenderError>;
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
}

/// Paint a whole tree with the default look.
pub fn paint_tree<R: Renderer>(renderer: &mut R, tree: &Tree, clear: Color) {
    renderer.begin_frame(clear);
    if let Some(root) = tree.root() {
        paint_node(renderer, tree, root);
    }
}

fn paint_node<R: Renderer>(renderer: &mut R, tree: &Tree, id: tekto_tree::NodeId) {
    let Some(node) = tree.get(id) else {
        return;
    };
    if let Some(fill) = node.fill {
        renderer.fill_rect(node.bounds, fill);
    }
    let composed;
    let text_to_draw = if node.kind == tekto_tree::NodeKind::TextField {
        composed = node.composed_text();
        if composed.is_empty() {
            None
        } else {
            Some(composed.as_str())
        }
    } else {
        node.text.as_deref()
    };
    if let Some(text) = text_to_draw {
        let color = node.color.unwrap_or(Color::INK);
        let inset = if matches!(
            node.kind,
            tekto_tree::NodeKind::Button
                | tekto_tree::NodeKind::TextField
                | tekto_tree::NodeKind::ListItem
        ) {
            node.layout.padding
        } else {
            0.0
        };
        let max_w = if node.bounds.width > 0.0 {
            Some((node.bounds.width - 2.0 * inset).max(0.0))
        } else {
            None
        };
        renderer.draw_text(
            text,
            node.bounds.x + inset,
            node.bounds.y + inset,
            node.font_size,
            color,
            max_w,
        );
    }
    if node.kind == tekto_tree::NodeKind::TextField
        && tree.focus == Some(id)
        && node.composed_caret().is_some()
    {
        let inset = node.layout.padding;
        let h = (node.bounds.height - 2.0 * inset).max(1.0);
        renderer.fill_rect(
            tekto_tree::Rect::new(
                node.bounds.x + inset + node.caret_x,
                node.bounds.y + inset,
                1.0,
                h,
            ),
            node.color.unwrap_or(Color::INK),
        );
    }
    let clip = node.kind == tekto_tree::NodeKind::Scroll;
    if clip {
        renderer.push_clip(node.bounds);
    }
    for child in &node.children {
        paint_node(renderer, tree, *child);
    }
    if clip {
        renderer.pop_clip();
    }
}

#[cfg(feature = "cpu")]
mod cpu;
#[cfg(feature = "cpu")]
pub use cpu::CpuRenderer;

/// Placeholder for the wgpu / Vello backend.
pub struct GpuRenderer;

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
