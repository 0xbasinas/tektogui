//! tiny-skia CPU renderer. Glyph outlines come from Skrifa, as in the
//! official Parley tiny_skia example (Apache-2.0 OR MIT).

use crate::{RenderError, Renderer};
use parley::{GlyphRun, PositionedLayoutItem};
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::FontRef as ReadFontsRef;
use skrifa::{GlyphId, MetadataProvider, OutlineGlyph};
use tekto_text::TextEngine;
use tekto_tree::{Color as TektoColor, Rect};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapMut, Transform};

pub struct CpuRenderer {
    pixmap: Pixmap,
    text: TextEngine,
    /// Physical pixels per logical pixel. Layout and paint stay in logical
    /// units; the pixmap is `logical * scale`. At 1.0 the output matches
    /// the old unscaled path (headless snapshots stay stable).
    scale: f32,
}

impl CpuRenderer {
    pub fn new(width: u32, height: u32) -> Result<Self, RenderError> {
        let width = width.max(1);
        let height = height.max(1);
        let pixmap = Pixmap::new(width, height).ok_or_else(|| RenderError::msg("pixmap"))?;
        Ok(Self {
            pixmap,
            text: TextEngine::new(),
            scale: 1.0,
        })
    }

    /// Paint logical coordinates into a physical pixmap.
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = if scale > 0.0 { scale } else { 1.0 };
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }
}

impl Renderer for CpuRenderer {
    fn width(&self) -> u32 {
        self.pixmap.width()
    }

    fn height(&self) -> u32 {
        self.pixmap.height()
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        if width == self.pixmap.width() && height == self.pixmap.height() {
            return Ok(());
        }
        self.pixmap = Pixmap::new(width.max(1), height.max(1))
            .ok_or_else(|| RenderError::msg("pixmap resize"))?;
        Ok(())
    }

    fn begin_frame(&mut self, clear: TektoColor) {
        self.pixmap.fill(to_color(clear));
    }

    fn fill_rect(&mut self, rect: Rect, color: TektoColor) {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        let s = self.scale;
        let Some(r) =
            tiny_skia::Rect::from_xywh(rect.x * s, rect.y * s, rect.width * s, rect.height * s)
        else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color(to_color(color));
        paint.anti_alias = true;
        self.pixmap
            .fill_rect(r, &paint, Transform::identity(), None);
    }

    fn draw_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: TektoColor,
        max_width: Option<f32>,
    ) {
        if text.is_empty() {
            return;
        }
        let s = self.scale;
        let brush = [color.r, color.g, color.b, color.a];
        let layout = self
            .text
            .layout(text, size * s, brush, max_width.map(|w| w * s));
        let pixmap = self.pixmap.as_mut();
        let mut pen = TinySkiaPen::new(pixmap);
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                    render_glyph_run(&glyph_run, &mut pen, x * s, y * s);
                }
            }
        }
        let _ = pixmap;
    }

    fn to_png(&self) -> Result<Vec<u8>, RenderError> {
        self.pixmap
            .encode_png()
            .map_err(|e| RenderError::msg(e.to_string()))
    }

    fn rgba8(&self) -> Result<Vec<u8>, RenderError> {
        // tiny-skia stores premultiplied RGBA. For a software window we
        // unpremultiply into straight RGBA so softbuffer can take it.
        let mut out = Vec::with_capacity((self.pixmap.width() * self.pixmap.height() * 4) as usize);
        for px in self.pixmap.pixels() {
            let r = px.red();
            let g = px.green();
            let b = px.blue();
            let a = px.alpha();
            if a == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else if a == 255 {
                out.extend_from_slice(&[r, g, b, a]);
            } else {
                let ur = (r as u16 * 255 / a as u16) as u8;
                let ug = (g as u16 * 255 / a as u16) as u8;
                let ub = (b as u16 * 255 / a as u16) as u8;
                out.extend_from_slice(&[ur, ug, ub, a]);
            }
        }
        Ok(out)
    }
}

fn to_color(c: TektoColor) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn render_glyph_run(
    glyph_run: &GlyphRun<'_, [u8; 4]>,
    pen: &mut TinySkiaPen<'_>,
    ox: f32,
    oy: f32,
) {
    let mut run_x = glyph_run.offset();
    let run_y = glyph_run.baseline();
    let style = glyph_run.style();
    let color = Color::from_rgba8(
        style.brush[0],
        style.brush[1],
        style.brush[2],
        style.brush[3],
    );
    let run = glyph_run.run();
    let font = run.font();
    let font_size = run.font_size();
    let normalized_coords: Vec<NormalizedCoord> = run
        .normalized_coords()
        .iter()
        .map(|c| NormalizedCoord::from_bits(c.to_bits()))
        .collect();
    let font_collection_ref = font.data.as_ref();
    let Ok(font_ref) = ReadFontsRef::from_index(font_collection_ref, font.index) else {
        return;
    };
    let outlines = font_ref.outline_glyphs();
    for glyph in glyph_run.glyphs() {
        let glyph_x = ox + run_x + glyph.x;
        let glyph_y = oy + run_y + glyph.y;
        run_x += glyph.advance;
        let glyph_id = GlyphId::from(glyph.id);
        if let Some(glyph_outline) = outlines.get(glyph_id) {
            pen.set_origin(glyph_x, glyph_y);
            pen.set_color(color);
            pen.draw_glyph(&glyph_outline, font_size, &normalized_coords);
        }
    }
}

struct TinySkiaPen<'a> {
    pixmap: PixmapMut<'a>,
    x: f32,
    y: f32,
    paint: Paint<'static>,
    open_path: PathBuilder,
}

impl<'a> TinySkiaPen<'a> {
    fn new(pixmap: PixmapMut<'a>) -> Self {
        Self {
            pixmap,
            x: 0.0,
            y: 0.0,
            paint: Paint::default(),
            open_path: PathBuilder::new(),
        }
    }

    fn set_origin(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
    }

    fn set_color(&mut self, color: Color) {
        self.paint.set_color(color);
        self.paint.anti_alias = true;
    }

    fn draw_glyph(
        &mut self,
        glyph: &OutlineGlyph<'_>,
        size: f32,
        normalized_coords: &[NormalizedCoord],
    ) {
        let location_ref = LocationRef::new(normalized_coords);
        let settings = DrawSettings::unhinted(Size::new(size), location_ref);
        if glyph.draw(settings, self).is_err() {
            return;
        }
        let builder = core::mem::replace(&mut self.open_path, PathBuilder::new());
        if let Some(path) = builder.finish() {
            self.pixmap.fill_path(
                &path,
                &self.paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

impl OutlinePen for TinySkiaPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.open_path.move_to(self.x + x, self.y - y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.open_path.line_to(self.x + x, self.y - y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.open_path
            .quad_to(self.x + cx0, self.y - cy0, self.x + x, self.y - y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.open_path.cubic_to(
            self.x + cx0,
            self.y - cy0,
            self.x + cx1,
            self.y - cy1,
            self.x + x,
            self.y - y,
        );
    }

    fn close(&mut self) {
        self.open_path.close();
    }
}
