//! One small set of quiet defaults shared by Rust and TSX.
//!
//! These are not a theme engine. The defaults reproduce the palette the
//! widgets already use, so an app that never sets tokens paints exactly as
//! before. [`Tree::set_tokens`](crate::Tree::set_tokens) changes what new
//! nodes and the renderer use; it does not repaint existing nodes.
//!
//! The same values can come from TSX through the `setTokens` HostOp.

use crate::{Color, Tree, TreeError};
use serde::Deserialize;

/// Quiet default colours and the two layout numbers the built-in widgets use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    /// Window and menu surface.
    pub page: Color,
    /// Body text and dark surfaces.
    pub ink: Color,
    /// Button and banner fill.
    pub banner: Color,
    /// Text on a banner or button.
    pub on_banner: Color,
    /// Inset field and control track.
    pub field: Color,
    /// Default menu and dialog outline.
    pub border: Color,
    /// Focus ring.
    pub focus: Color,
    /// Stack gap for a window.
    pub gap: f32,
    /// Window padding.
    pub padding: f32,
    /// Body text size.
    pub body_size: f32,
    /// Banner title size.
    pub title_size: f32,
}

impl Tokens {
    /// The current defaults, unchanged from before tokens existed.
    pub const DEFAULT: Self = Self {
        page: Color::PAGE,
        ink: Color::INK,
        banner: Color::BANNER,
        on_banner: Color::ON_BANNER,
        field: Color::FIELD,
        border: Color::INK,
        focus: Color::rgba(42, 100, 190, 255),
        gap: 20.0,
        padding: 24.0,
        body_size: 20.0,
        title_size: 22.0,
    };
}

impl Default for Tokens {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A partial token set from an app. Colours are `#rrggbb` or `#rrggbbaa`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TokenPatch {
    #[serde(default, alias = "page")]
    pub page: Option<String>,
    #[serde(default)]
    pub ink: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default, alias = "on_banner")]
    pub on_banner: Option<String>,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub border: Option<String>,
    #[serde(default)]
    pub focus: Option<String>,
    #[serde(default)]
    pub gap: Option<f32>,
    #[serde(default)]
    pub padding: Option<f32>,
    #[serde(default, alias = "body_size")]
    pub body_size: Option<f32>,
    #[serde(default, alias = "title_size")]
    pub title_size: Option<f32>,
}

impl TokenPatch {
    /// Overlay this patch on `tokens`. Unknown or invalid fields leave the
    /// set untouched and return a message an agent can act on.
    pub fn apply_to(&self, tokens: &mut Tokens) -> Result<(), String> {
        fn color(name: &str, value: &Option<String>) -> Result<Option<Color>, String> {
            let Some(text) = value else {
                return Ok(None);
            };
            Color::from_hex(text)
                .map(Some)
                .ok_or_else(|| format!("{name} must be #rrggbb or #rrggbbaa, got `{text}`"))
        }
        fn size(name: &str, value: Option<f32>) -> Result<Option<f32>, String> {
            match value {
                None => Ok(None),
                Some(value) if value.is_finite() && value >= 0.0 => Ok(Some(value)),
                Some(_) => Err(format!("{name} must be a finite number >= 0")),
            }
        }
        let mut next = *tokens;
        if let Some(value) = color("page", &self.page)? {
            next.page = value;
        }
        if let Some(value) = color("ink", &self.ink)? {
            next.ink = value;
        }
        if let Some(value) = color("banner", &self.banner)? {
            next.banner = value;
        }
        if let Some(value) = color("onBanner", &self.on_banner)? {
            next.on_banner = value;
        }
        if let Some(value) = color("field", &self.field)? {
            next.field = value;
        }
        if let Some(value) = color("border", &self.border)? {
            next.border = value;
        }
        if let Some(value) = color("focus", &self.focus)? {
            next.focus = value;
        }
        if let Some(value) = size("gap", self.gap)? {
            next.gap = value;
        }
        if let Some(value) = size("padding", self.padding)? {
            next.padding = value;
        }
        if let Some(value) = size("bodySize", self.body_size)? {
            next.body_size = value;
        }
        if let Some(value) = size("titleSize", self.title_size)? {
            next.title_size = value;
        }
        *tokens = next;
        Ok(())
    }
}

impl Tree {
    /// The token set new nodes and the renderer use.
    pub fn tokens(&self) -> Tokens {
        self.tokens
    }

    /// Replace the token set. Existing nodes keep their resolved colours and
    /// sizes; nodes created afterwards and the renderer use the new values.
    pub fn set_tokens(&mut self, tokens: Tokens) {
        self.tokens = tokens;
    }

    /// Apply a partial token set, for example from `setTokens` JSON.
    pub fn apply_tokens(&mut self, patch: &TokenPatch) -> Result<(), TreeError> {
        let mut tokens = self.tokens;
        patch
            .apply_to(&mut tokens)
            .map_err(|message| TreeError { message })?;
        self.tokens = tokens;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> TokenPatch {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn defaults_match_the_built_in_palette() {
        let tokens = Tokens::default();
        assert_eq!(tokens.page, Color::PAGE);
        assert_eq!(tokens.ink, Color::INK);
        assert_eq!(tokens.banner, Color::BANNER);
        assert_eq!(tokens.on_banner, Color::ON_BANNER);
        assert_eq!(tokens.field, Color::FIELD);
        assert_eq!(tokens.border, Color::INK);
        assert_eq!(tokens.focus, Color::rgba(42, 100, 190, 255));
        assert_eq!(tokens.gap, 20.0);
        assert_eq!(tokens.padding, 24.0);
        assert_eq!(tokens.body_size, 20.0);
        assert_eq!(tokens.title_size, 22.0);
    }

    #[test]
    fn patch_updates_named_tokens_and_accepts_snake_aliases() {
        let mut tokens = Tokens::default();
        let patch = parse(r##"{"page":"#101010","on_banner":"#f0f0f0","gap":8}"##);
        patch.apply_to(&mut tokens).unwrap();
        assert_eq!(tokens.page, Color::rgb(0x10, 0x10, 0x10));
        assert_eq!(tokens.on_banner, Color::rgb(0xf0, 0xf0, 0xf0));
        assert_eq!(tokens.gap, 8.0);
        assert_eq!(tokens.ink, Color::INK);

        let alias = parse(r##"{"body_size":26,"on_banner":"#000000"}"##);
        alias.apply_to(&mut tokens).unwrap();
        assert_eq!(tokens.body_size, 26.0);
    }

    #[test]
    fn bad_values_are_rejected_without_changing_the_set() {
        let mut tokens = Tokens::default();
        let patch = parse(r##"{"ink":"teal"}"##);
        let error = patch.apply_to(&mut tokens).unwrap_err();
        assert!(error.contains("ink must be #rrggbb"), "{error}");
        assert_eq!(tokens, Tokens::default());

        let patch = parse(r#"{"gap":-1}"#);
        assert!(patch.apply_to(&mut tokens).is_err());
        assert_eq!(tokens, Tokens::default());

        assert!(serde_json::from_str::<TokenPatch>(r#"{"nope":1}"#).is_err());
    }
}
