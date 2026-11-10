//! Text layout built on Parley.
//!
//! A copy of Roboto Regular (Apache-2.0) is bundled so layout is the same
//! in CI, on a phone, and on a desktop with no system fonts.

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{FontContext, FontFamily, Layout, LayoutContext, LineHeight, StyleProperty};
use std::sync::Arc;

/// Bundled Roboto Regular. Apache-2.0. See `fonts/LICENSE-Roboto.txt`.
pub const ROBOTO_REGULAR: &[u8] = include_bytes!("../fonts/Roboto-Regular.ttf");

const FAMILY_NAME: &str = "Roboto";

/// Default line-height relative to font size. Settled, not tight.
pub const LINE_HEIGHT: f32 = 1.4;

fn roboto_blob() -> Blob<u8> {
    Blob::new(Arc::new(ROBOTO_REGULAR.to_vec()))
}

/// Shared font and layout contexts. These are meant to be reused.
pub struct TextEngine {
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<[u8; 4]>,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    pub fn new() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let registered = collection.register_fonts(roboto_blob(), None);
        assert!(
            !registered.is_empty(),
            "bundled Roboto Regular did not register"
        );
        Self {
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
        builder.push_default(FontFamily::named(FAMILY_NAME));
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
}
