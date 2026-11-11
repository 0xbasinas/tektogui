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
