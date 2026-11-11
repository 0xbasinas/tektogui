//! A reusable in-window app bar: leading slot, title, and actions.
//!
//! This is a bar inside the client area, not OS window chrome. It uses the
//! tree's design tokens, so a tinted bar follows `Tree::set_tokens`. Action
//! buttons are quiet: no fill, banner text colour, and a radius on focus.

use super::*;

/// Height of an app bar in logical pixels.
pub const APP_BAR_HEIGHT: f32 = 56.0;
const APP_BAR_PADDING: f32 = 12.0;
const ACTION_HEIGHT: f32 = 32.0;

/// The parts of an app bar. Add actions or a leading button through the
/// helpers; keeping the ids lets an app or test find them.
pub struct AppBar {
    pub root: NodeId,
    pub leading: NodeId,
    pub title: NodeId,
    pub actions: NodeId,
}

/// A banner-coloured bar with a title that takes the free space, so actions
/// sit on the right edge.
pub fn app_bar(tree: &mut Tree, parent: NodeId, title: &str) -> Result<AppBar, WidgetError> {
    let tokens = tree.tokens();
    let root = tree.add(NodeKind::Rect, Some(parent))?;
    if let Some(node) = tree.get_mut(root) {
        node.name = Some("app bar".into());
        node.fill = Some(tokens.banner);
        node.layout.height = APP_BAR_HEIGHT;
        node.layout.padding = APP_BAR_PADDING;
        node.layout.gap = 8.0;
        node.layout.column = false;
    }
    let leading = tree.add(NodeKind::Group, Some(root))?;
    if let Some(node) = tree.get_mut(leading) {
        node.name = Some("app bar leading".into());
        node.layout.column = false;
        node.layout.gap = 6.0;
    }
    let title_id = tree.add(NodeKind::Text, Some(root))?;
    if let Some(node) = tree.get_mut(title_id) {
        node.name = Some(title.into());
        node.text = Some(title.into());
        node.color = Some(tokens.on_banner);
        node.font_size = tokens.body_size;
        node.grow = 1.0;
    }
    let actions = tree.add(NodeKind::Group, Some(root))?;
    if let Some(node) = tree.get_mut(actions) {
        node.name = Some("app bar actions".into());
        node.layout.column = false;
        node.layout.gap = 8.0;
    }
    Ok(AppBar {
        root,
        leading,
        title: title_id,
        actions,
    })
}

/// Add a quiet text button to the bar's right-hand actions.
pub fn app_bar_action(tree: &mut Tree, bar: &AppBar, label: &str) -> Result<NodeId, WidgetError> {
    action_button(tree, bar.actions, label)
}

/// Add a quiet button to the leading slot, for a menu or back control.
pub fn app_bar_leading_button(
    tree: &mut Tree,
    bar: &AppBar,
    label: &str,
) -> Result<NodeId, WidgetError> {
    action_button(tree, bar.leading, label)
}

fn action_button(tree: &mut Tree, parent: NodeId, label: &str) -> Result<NodeId, WidgetError> {
    let tokens = tree.tokens();
    let id = tree.add(NodeKind::Button, Some(parent))?;
    if let Some(node) = tree.get_mut(id) {
        node.name = Some(label.into());
        node.text = Some(label.into());
        node.fill = None;
        node.color = Some(tokens.on_banner);
        node.font_size = tokens.body_size;
        node.layout.height = ACTION_HEIGHT;
        node.layout.padding = 6.0;
        node.radius = 6.0;
    }
    Ok(id)
}
