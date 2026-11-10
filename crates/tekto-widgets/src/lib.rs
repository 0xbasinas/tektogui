//! Built-in widgets. The look is plain and quiet: warm page, charcoal
//! ink, comfortable spacing. Themes come later.

use tekto_tree::{Color, LayoutStyle, NodeId, NodeKind, Tree, TreeError};

/// Space used around the window content.
pub const PAGE_PADDING: f32 = 24.0;
/// Space between stacked children.
pub const STACK_GAP: f32 = 16.0;
/// Default body text size.
pub const BODY_SIZE: f32 = 20.0;
/// Default title text size.
pub const TITLE_SIZE: f32 = 22.0;

pub struct WidgetError(pub TreeError);

impl From<TreeError> for WidgetError {
    fn from(value: TreeError) -> Self {
        WidgetError(value)
    }
}

impl std::fmt::Display for WidgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Debug for WidgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for WidgetError {}

/// A window root with page padding and a column layout.
pub fn window(
    tree: &mut Tree,
    title: &str,
    width: f32,
    height: f32,
) -> Result<NodeId, WidgetError> {
    let id = tree.add(NodeKind::Window, None)?;
    if let Some(n) = tree.get_mut(id) {
        n.name = Some(title.into());
        n.fill = Some(Color::PAGE);
        n.layout = LayoutStyle {
            width,
            height,
            padding: PAGE_PADDING,
            gap: STACK_GAP,
            column: true,
        };
    }
    Ok(id)
}

/// Dark banner used as a simple header surface.
pub fn banner(
    tree: &mut Tree,
    parent: NodeId,
    width: f32,
    height: f32,
) -> Result<NodeId, WidgetError> {
    let id = tree.add(NodeKind::Rect, Some(parent))?;
    if let Some(n) = tree.get_mut(id) {
        n.name = Some("banner".into());
        n.fill = Some(Color::BANNER);
        n.layout = LayoutStyle::sized(width, height);
    }
    Ok(id)
}

/// Text node with the default typography.
pub fn text(
    tree: &mut Tree,
    parent: NodeId,
    content: &str,
    size: f32,
    color: Color,
) -> Result<NodeId, WidgetError> {
    let id = tree.add(NodeKind::Text, Some(parent))?;
    if let Some(n) = tree.get_mut(id) {
        n.text = Some(content.into());
        n.name = Some(content.into());
        n.color = Some(color);
        n.font_size = size;
        // Height is left unset so Taffy can measure the line box with Parley.
        n.layout = LayoutStyle::default();
    }
    Ok(id)
}

/// Title text drawn on the dark banner.
pub fn title_on_banner(
    tree: &mut Tree,
    banner: NodeId,
    content: &str,
    width: f32,
) -> Result<NodeId, WidgetError> {
    let id = tree.add(NodeKind::Text, Some(banner))?;
    if let Some(n) = tree.get_mut(id) {
        n.text = Some(content.into());
        n.name = Some(content.into());
        n.color = Some(Color::ON_BANNER);
        n.font_size = TITLE_SIZE;
        n.layout = LayoutStyle {
            width,
            height: 48.0,
            padding: 14.0,
            gap: 0.0,
            column: true,
        };
    }
    Ok(id)
}

/// The first example screen: a Notes window with a banner and a greeting.
pub fn notes_tree(width: f32, height: f32) -> Result<Tree, WidgetError> {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Notes", width, height)?;
    let banner_w = (width - PAGE_PADDING * 2.0).max(80.0);
    let b = banner(&mut tree, root, banner_w, 64.0)?;
    title_on_banner(&mut tree, b, "Notes", banner_w)?;
    text(&mut tree, root, "Hello from Tekto", BODY_SIZE, Color::INK)?;
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_has_stable_ids() {
        let tree = notes_tree(640.0, 400.0).unwrap();
        assert!(tree.len() >= 3);
        let pretty = tree.to_pretty_text();
        assert!(pretty.contains("Hello from Tekto"));
        assert!(pretty.contains("window#1"));
    }
}
