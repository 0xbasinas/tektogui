//! Text layout built on Parley.
//!
//! A copy of Roboto Regular (Apache-2.0) is bundled so layout is the same
//! in CI, on a phone, and on a desktop with no system fonts.

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{
    FontContext, FontFamily, FontFamilyName, Layout, LayoutContext, LineHeight, StyleProperty,
};
use std::sync::{Arc, OnceLock};

/// Bundled Roboto Regular. Apache-2.0. See `fonts/LICENSE-Roboto.txt`.
pub const ROBOTO_REGULAR: &[u8] = include_bytes!("../fonts/Roboto-Regular.ttf");

const FAMILY_NAME: &str = "Roboto";

/// Default line-height relative to font size. Settled, not tight.
pub const LINE_HEIGHT: f32 = 1.4;

fn roboto_blob() -> Blob<u8> {
    Blob::new(Arc::new(ROBOTO_REGULAR.to_vec()))
}

// Frozen on first use so layout and paint cannot observe different fonts.
static FALLBACK: OnceLock<Option<Arc<Vec<u8>>>> = OnceLock::new();

/// Set a supplied fallback font before any text engine is created.
///
/// The bytes must be a font or font collection. Returns an error if the font is
/// invalid or text has already been initialized. All engines use the same font.
pub fn set_fallback_font(bytes: Vec<u8>) -> Result<(), &'static str> {
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    let bytes = Arc::new(bytes);
    if collection
        .register_fonts(Blob::new(bytes.clone()), None)
        .is_empty()
    {
        return Err("fallback font contains no usable fonts");
    }
    FALLBACK
        .set(Some(bytes))
        .map_err(|_| "text is already initialized")
}

fn fallback_font() -> &'static Option<Arc<Vec<u8>>> {
    FALLBACK.get_or_init(|| {
        std::env::var_os("TEKTO_FALLBACK_FONT").map(|path| {
            let bytes = std::fs::read(path).expect("cannot read TEKTO_FALLBACK_FONT");
            let mut collection = Collection::new(CollectionOptions {
                shared: false,
                system_fonts: false,
            });
            let bytes = Arc::new(bytes);
            assert!(
                !collection
                    .register_fonts(Blob::new(bytes.clone()), None)
                    .is_empty(),
                "TEKTO_FALLBACK_FONT contains no usable fonts"
            );
            bytes
        })
    })
}

/// Shared font and layout contexts. These are meant to be reused.
pub struct TextEngine {
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<[u8; 4]>,
    families: Vec<String>,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    pub fn new() -> Self {
        Self::with_fallback(fallback_font().as_ref())
    }

    fn with_fallback(fallback: Option<&Arc<Vec<u8>>>) -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let registered = collection.register_fonts(roboto_blob(), None);
        assert!(
            !registered.is_empty(),
            "bundled Roboto Regular did not register"
        );
        let mut families = vec![FAMILY_NAME.to_owned()];
        if let Some(bytes) = fallback {
            for (id, _) in collection.register_fonts(Blob::new(bytes.clone()), None) {
                if let Some(family) = collection.family(id) {
                    let name = family.name().to_owned();
                    if !families.contains(&name) {
                        families.push(name);
                    }
                }
            }
        }
        Self {
            families,
            font_cx: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
            layout_cx: LayoutContext::new(),
        }
    }

    pub fn layout(
        &mut self,
        text: &str,
        font_size: f32,
        color: [u8; 4],
        max_width: Option<f32>,
    ) -> Layout<[u8; 4]> {
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, true);
        builder.push_default(StyleProperty::Brush(color));
        builder.push_default(StyleProperty::FontSize(font_size));
        builder.push_default(LineHeight::FontSizeRelative(LINE_HEIGHT));
        if self.families.len() == 1 {
            builder.push_default(FontFamily::named(FAMILY_NAME));
        } else {
            let families: Vec<_> = self
                .families
                .iter()
                .map(|name| FontFamilyName::named(name))
                .collect();
            builder.push_default(FontFamily::from(families.as_slice()));
        }
        let mut layout: Layout<[u8; 4]> = builder.build(text);
        layout.break_all_lines(max_width);
        layout
    }

    pub fn measure(&mut self, text: &str, font_size: f32, max_width: Option<f32>) -> (f32, f32) {
        let layout = self.layout(text, font_size, [0, 0, 0, 255], max_width);
        (layout.width(), layout.height())
    }
}

/// Largest text, in graphemes, that gets per-grapheme geometry.
pub const MAX_GEOMETRY_GRAPHEMES: usize = 4000;

/// One `(x, y, width, height)` rectangle per grapheme cluster, relative to the
/// layout origin. A grapheme with no visible box (such as a line break) gets a
/// zero-width rectangle at its caret position. Returns an empty list for text
/// longer than [`MAX_GEOMETRY_GRAPHEMES`].
pub fn grapheme_rects(layout: &Layout<[u8; 4]>, text: &str) -> Vec<(f32, f32, f32, f32)> {
    use unicode_segmentation::UnicodeSegmentation;
    let count = text.graphemes(true).count();
    if count > MAX_GEOMETRY_GRAPHEMES {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(count);
    let mut scalar = 0;
    for grapheme in text.graphemes(true) {
        let end = scalar + grapheme.chars().count();
        let rects = TextSelection::new(layout, text, scalar, end).rectangles(layout);
        let rect = if let Some(first) = rects.first() {
            let (mut x0, mut y0) = (first.0, first.1);
            let (mut x1, mut y1) = (first.0 + first.2, first.1 + first.3);
            for r in &rects[1..] {
                x0 = x0.min(r.0);
                y0 = y0.min(r.1);
                x1 = x1.max(r.0 + r.2);
                y1 = y1.max(r.1 + r.3);
            }
            (x0, y0, x1 - x0, y1 - y0)
        } else {
            let (x, y, _, h) = TextSelection::new(layout, text, scalar, scalar).caret_rect(layout);
            (x, y, 0.0, h)
        };
        out.push(rect);
        scalar = end;
    }
    out
}

/// Return the UTF-8 byte index nearest a point in a laid out text block.
pub fn cursor_index(layout: &Layout<[u8; 4]>, x: f32, y: f32) -> usize {
    parley::editing::Cursor::from_point(layout, x, y).index()
}

/// A selection over a laid-out text block. Public positions are Unicode scalar
/// indices; Parley uses UTF-8 byte indices internally.
pub struct TextSelection {
    selection: parley::editing::Selection,
}

impl TextSelection {
    pub fn new(layout: &Layout<[u8; 4]>, text: &str, anchor: usize, focus: usize) -> Self {
        use parley::{editing::Cursor, layout::Affinity};
        let byte = |index: usize| {
            text.char_indices()
                .nth(index)
                .map_or(text.len(), |(i, _)| i)
        };
        Self {
            selection: parley::editing::Selection::new(
                Cursor::from_byte_index(layout, byte(anchor), Affinity::Downstream),
                Cursor::from_byte_index(layout, byte(focus), Affinity::Downstream),
            ),
        }
    }

    /// Move by visual lines, preserving the preferred horizontal position.
    pub fn move_lines(&mut self, layout: &Layout<[u8; 4]>, delta: isize, extend: bool) {
        self.selection = self.selection.move_lines(layout, delta, extend);
    }

    /// Move vertically using a column saved by the caller across events.
    pub fn move_lines_at_x(
        &mut self,
        layout: &Layout<[u8; 4]>,
        delta: isize,
        extend: bool,
        x: f32,
    ) {
        let original = self.selection;
        let moved = original.move_lines(layout, delta, extend);
        let bounds = moved.focus().geometry(layout, 1.0);
        let focus =
            parley::editing::Cursor::from_point(layout, x, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        self.selection =
            parley::editing::Selection::new(if extend { original.anchor() } else { focus }, focus);
    }

    pub fn indices(&self, text: &str) -> (usize, usize) {
        let scalar = |index: usize| {
            let mut boundary = index.min(text.len());
            while !text.is_char_boundary(boundary) {
                boundary -= 1;
            }
            text[..boundary].chars().count()
        };
        (
            scalar(self.selection.anchor().index()),
            scalar(self.selection.focus().index()),
        )
    }

    /// Caret rectangle relative to the text layout's origin.
    pub fn caret_rect(&self, layout: &Layout<[u8; 4]>) -> (f32, f32, f32, f32) {
        let bounds = self.selection.focus().geometry(layout, 1.0);
        (
            bounds.x0 as f32,
            bounds.y0 as f32,
            (bounds.x1 - bounds.x0) as f32,
            (bounds.y1 - bounds.y0) as f32,
        )
    }

    /// Visual rectangles covering the selection, one per shaped span.
    pub fn rectangles(&self, layout: &Layout<[u8; 4]>) -> Vec<(f32, f32, f32, f32)> {
        self.selection
            .geometry(layout)
            .into_iter()
            .map(|(b, _)| {
                (
                    b.x0 as f32,
                    b.y0 as f32,
                    (b.x1 - b.x0) as f32,
                    (b.y1 - b.y0) as f32,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_hello() {
        let mut engine = TextEngine::new();
        let (w, h) = engine.measure("Hello from Tekto", 20.0, Some(600.0));
        assert!(w > 10.0, "width was {w}");
        assert!(h > 10.0, "height was {h}");
    }

    #[test]
    fn selection_moves_across_wrapped_lines_and_unicode() {
        let mut engine = TextEngine::new();
        let text = "aé🙂 one two three four five";
        let layout = engine.layout(text, 20.0, [0, 0, 0, 255], Some(60.0));
        assert!(layout.len() > 1);
        let mut selection = TextSelection::new(&layout, text, 1, 1);
        let first = selection.caret_rect(&layout);
        selection.move_lines(&layout, 1, true);
        let (anchor, focus) = selection.indices(text);
        assert_eq!(anchor, 1);
        assert!(focus > anchor);
        assert!(selection.caret_rect(&layout).1 > first.1);
        assert!(!selection.rectangles(&layout).is_empty());
        selection.move_lines(&layout, -1, false);
        assert_eq!(selection.indices(text).0, selection.indices(text).1);
        assert!(selection.caret_rect(&layout).1 <= first.1);
    }

    fn glyph_ids(layout: &Layout<[u8; 4]>) -> Vec<u32> {
        let mut ids = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                if let parley::PositionedLayoutItem::GlyphRun(run) = item {
                    ids.extend(run.glyphs().map(|glyph| glyph.id));
                }
            }
        }
        ids
    }

    #[test]
    fn supplied_font_resolves_cjk_when_available() {
        let Some(path) = std::env::var_os("TEKTO_TEST_CJK_FONT") else {
            eprintln!("TEKTO_TEST_CJK_FONT not set; skipping local CJK font check");
            return;
        };
        let bytes = Arc::new(std::fs::read(path).expect("read local CJK font"));
        let mut default = TextEngine::with_fallback(None);
        let mut fallback = TextEngine::with_fallback(Some(&bytes));
        let missing = glyph_ids(&default.layout("日本", 24.0, [0; 4], None));
        let resolved = glyph_ids(&fallback.layout("日本", 24.0, [0; 4], None));
        assert!(
            missing.contains(&0),
            "Roboto unexpectedly covers test text: {missing:?}"
        );
        assert_eq!(resolved.len(), 2);
        assert!(
            resolved.iter().all(|id| *id != 0),
            "missing CJK glyphs: {resolved:?}"
        );
        let latin_default = glyph_ids(&default.layout("Hello", 24.0, [0; 4], None));
        let latin_fallback = glyph_ids(&fallback.layout("Hello", 24.0, [0; 4], None));
        assert_eq!(latin_default, latin_fallback, "Roboto must remain first");
    }

    #[test]
    fn rejects_invalid_font() {
        assert!(set_fallback_font(vec![0, 1, 2]).is_err());
    }
}
