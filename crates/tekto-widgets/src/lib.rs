//! Built-in widgets. The look is plain and quiet: warm page, charcoal
//! ink, comfortable spacing. Themes come later.

use tekto_text::TextEngine;
use tekto_tree::{Color, LayoutStyle, Node, NodeId, NodeKind, Tree, TreeError};

/// Space used around the window content.
pub const PAGE_PADDING: f32 = 24.0;
/// Space between stacked children (body sits comfortably under the banner).
pub const STACK_GAP: f32 = 20.0;
/// Default body text size.
pub const BODY_SIZE: f32 = 20.0;
/// Default title text size.
pub const TITLE_SIZE: f32 = 22.0;
/// Quiet header band. Title is padded and vertically centered inside it.
pub const BANNER_HEIGHT: f32 = 64.0;
/// Inset for title text inside the banner.
pub const BANNER_PADDING: f32 = 20.0;

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
            justify_center: false,
        };
    }
    Ok(id)
}

/// Dark banner used as a simple header surface.
///
/// Pass a non-finite `width` (the usual case) so the banner stretches to
/// the parent's content box and stays inside the window after a resize.
/// A finite width is a fixed size and will not shrink with the viewport.
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
        n.layout = LayoutStyle {
            width,
            height,
            padding: BANNER_PADDING,
            gap: 0.0,
            column: true,
            justify_center: true,
        };
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

/// A quiet, labeled button. The returned id is the interactive scene node.
pub fn button(tree: &mut Tree, parent: NodeId, label: &str) -> Result<NodeId, WidgetError> {
    let id = tree.add(NodeKind::Button, Some(parent))?;
    if let Some(n) = tree.get_mut(id) {
        n.name = Some(label.into());
        n.text = Some(label.into());
        n.fill = Some(Color::BANNER);
        n.color = Some(Color::ON_BANNER);
        n.font_size = BODY_SIZE;
        n.layout.padding = 12.0;
    }
    Ok(id)
}

/// A single-line text field. `label` is the accessible name; `value` is `text`.
///
/// The caret starts at the end of `value`. Full IME (composition, candidates,
/// marked text) is not implemented. Desktop keys that produce a character,
/// and Backspace, edit `value` when the field is focused.
pub fn text_field(
    tree: &mut Tree,
    parent: NodeId,
    label: &str,
    value: &str,
) -> Result<NodeId, WidgetError> {
    let id = tree.add(NodeKind::TextField, Some(parent))?;
    if let Some(n) = tree.get_mut(id) {
        n.name = Some(label.into());
        n.text = Some(value.into());
        n.fill = Some(Color::FIELD);
        n.color = Some(Color::INK);
        n.font_size = BODY_SIZE;
        n.caret = value.chars().count();
        n.layout.padding = 10.0;
    }
    Ok(id)
}

/// A vertical scroll container. Children are clipped to this box and shifted
/// by [`tekto_tree::Node::scroll_y`]. Wheel and drag change that offset;
/// there is no rubber-band. `height` is the viewport (a non-finite height
/// grows with the content and will not scroll).
pub fn scroll(
    tree: &mut Tree,
    parent: NodeId,
    width: f32,
    height: f32,
) -> Result<NodeId, WidgetError> {
    let id = tree.add(NodeKind::Scroll, Some(parent))?;
    if let Some(n) = tree.get_mut(id) {
        n.name = Some("scroll".into());
        n.layout = LayoutStyle {
            width,
            height,
            padding: 0.0,
            gap: 0.0,
            column: true,
            justify_center: false,
        };
    }
    Ok(id)
}

/// Space between list rows.
pub const LIST_GAP: f32 = 8.0;
/// Inset of a row label inside its box.
pub const LIST_ROW_PADDING: f32 = 10.0;

/// One labeled row. `height` is the row box in logical pixels (variable per row).
#[derive(Clone, Copy, Debug)]
pub struct ListRow<'a> {
    pub label: &'a str,
    pub height: f32,
}

/// Ids for a list. `scroll` is the scroll parent when the rows are taller than
/// the viewport, including after later [`list_item`] calls made in this `list()`
/// call. A scroll parent inserted by a later [`list_item`] is not written back
/// into a `ListIds` value the caller already holds; read the list's parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListIds {
    pub list: NodeId,
    pub scroll: Option<NodeId>,
}

/// A vertical list of variable-height rows.
///
/// Each row is a [`NodeKind::ListItem`] with a stable scene id. The list node
/// is [`NodeKind::List`]. Rows are not virtualized.
///
/// When `viewport_height` is finite and the stacked rows are taller than that
/// viewport, the list is parented under [`scroll`]. A shorter list, or a
/// non-finite viewport, is a direct child of `parent` and is not clipped.
/// [`list_item`] revisits that decision: once appended rows make the stack
/// taller than the viewport, it inserts a [`scroll`] container in the list's
/// place and reparents the list under it.
pub fn list(
    tree: &mut Tree,
    parent: NodeId,
    width: f32,
    viewport_height: f32,
    rows: &[ListRow<'_>],
) -> Result<ListIds, WidgetError> {
    let list_id = tree.add(NodeKind::List, Some(parent))?;
    if let Some(n) = tree.get_mut(list_id) {
        n.name = Some("list".into());
        n.list_viewport = if viewport_height.is_finite() {
            viewport_height
        } else {
            f32::NAN
        };
        n.layout = LayoutStyle {
            width,
            height: f32::NAN,
            padding: 0.0,
            gap: LIST_GAP,
            column: true,
            justify_center: false,
        };
    }
    for row in rows {
        list_item(tree, list_id, row.label, row.height)?;
    }
    Ok(ListIds {
        list: list_id,
        scroll: scroll_parent(tree, list_id),
    })
}

fn scroll_parent(tree: &Tree, list_id: NodeId) -> Option<NodeId> {
    let parent = tree.get(list_id)?.parent?;
    if tree.get(parent)?.kind == NodeKind::Scroll {
        Some(parent)
    } else {
        None
    }
}

/// One list row. `label` is the accessible name and the painted text.
/// A finite `height` is the row box; a non-finite height hugs the label.
/// The returned id is stable for the life of the tree.
///
/// If `list` was built by [`list`] with a finite viewport and the rows are now
/// taller than that viewport, the list is wrapped in a [`scroll`] container.
pub fn list_item(
    tree: &mut Tree,
    list: NodeId,
    label: &str,
    height: f32,
) -> Result<NodeId, WidgetError> {
    let id = tree.add(NodeKind::ListItem, Some(list))?;
    if let Some(n) = tree.get_mut(id) {
        n.name = Some(label.into());
        n.text = Some(label.into());
        n.color = Some(Color::INK);
        n.font_size = BODY_SIZE;
        n.layout = LayoutStyle {
            width: f32::NAN,
            height,
            padding: LIST_ROW_PADDING,
            gap: 0.0,
            column: true,
            justify_center: false,
        };
    }
    ensure_list_scroll(tree, list)?;
    Ok(id)
}

/// Insert a [`scroll`] parent when a managed list's rows exceed its viewport.
fn ensure_list_scroll(tree: &mut Tree, list_id: NodeId) -> Result<(), WidgetError> {
    let Some(list) = tree.get(list_id) else {
        return Ok(());
    };
    if list.kind != NodeKind::List {
        return Ok(());
    }
    let viewport = list.list_viewport;
    if !viewport.is_finite() {
        return Ok(());
    }
    let Some(parent) = list.parent else {
        return Ok(());
    };
    if tree.get(parent).is_some_and(|n| n.kind == NodeKind::Scroll) {
        return Ok(());
    }
    let width = list.layout.width;
    let gap = list.layout.gap;
    let children = list.children.clone();
    let content = stacked_node_height(tree, &children, gap, width);
    if content <= viewport {
        return Ok(());
    }
    let index = tree
        .get(parent)
        .and_then(|n| n.children.iter().position(|c| *c == list_id))
        .unwrap_or(0);
    let scroll_id = scroll(tree, parent, width, viewport)?;
    tree.append(scroll_id, list_id)?;
    if let Some(p) = tree.get_mut(parent) {
        p.children.retain(|c| *c != scroll_id);
        let index = index.min(p.children.len());
        p.children.insert(index, scroll_id);
    }
    Ok(())
}

fn stacked_node_height(tree: &Tree, children: &[NodeId], gap: f32, list_width: f32) -> f32 {
    if children.is_empty() {
        return 0.0;
    }
    let mut text = None;
    let mut total = 0.0;
    for id in children {
        let Some(node) = tree.get(*id) else {
            continue;
        };
        let height = row_border_height(node, list_width, &mut text);
        if !height.is_finite() {
            return f32::INFINITY;
        }
        total += height;
    }
    let gap = if gap.is_finite() { gap.max(0.0) } else { 0.0 };
    total + gap * (children.len() - 1) as f32
}

/// Border-box height used for the scroll decision.
///
/// A finite row height is that box. A non-finite height hugs the label: the
/// same Parley measure layout uses, plus the row's vertical padding. A
/// negative height is not a hug and is treated as unbounded.
fn row_border_height(node: &Node, list_width: f32, text: &mut Option<TextEngine>) -> f32 {
    let set = node.layout.height;
    if set.is_finite() {
        return if set < 0.0 { f32::INFINITY } else { set };
    }
    let content = node.text.as_deref().unwrap_or("");
    let sample = if content.is_empty() { " " } else { content };
    let pad = if node.layout.padding.is_finite() {
        node.layout.padding.max(0.0)
    } else {
        0.0
    };
    let width = if node.layout.width.is_finite() {
        node.layout.width
    } else {
        list_width
    };
    let max_width = if width.is_finite() {
        Some((width - 2.0 * pad).max(0.0))
    } else {
        None
    };
    let engine = text.get_or_insert_with(TextEngine::new);
    let (_, measured) = engine.measure(sample, node.font_size, max_width);
    measured.max(1.0) + 2.0 * pad
}

/// Title text drawn on the dark banner.
pub fn title_on_banner(
    tree: &mut Tree,
    banner: NodeId,
    content: &str,
    _width: f32,
) -> Result<NodeId, WidgetError> {
    let id = tree.add(NodeKind::Text, Some(banner))?;
    if let Some(n) = tree.get_mut(id) {
        n.text = Some(content.into());
        n.name = Some(content.into());
        n.color = Some(Color::ON_BANNER);
        n.font_size = TITLE_SIZE;
        // Let Taffy measure the line box; the banner pads and centers it.
        n.layout = LayoutStyle::default();
    }
    Ok(id)
}

/// The first example screen: a Notes window with a banner and a greeting.
pub fn notes_tree(width: f32, height: f32) -> Result<Tree, WidgetError> {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Notes", width, height)?;
    // Do not bake the construction-time width into the banner; a later
    // relayout (phone-sized Android window, desktop resize) must be able
    // to shrink it to the new content box.
    let b = banner(&mut tree, root, f32::NAN, BANNER_HEIGHT)?;
    title_on_banner(&mut tree, b, "Notes", f32::NAN)?;
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

    #[test]
    fn text_field_stores_label_and_value() {
        let mut tree = Tree::new();
        let root = window(&mut tree, "Notes", 320.0, 200.0).unwrap();
        let field = text_field(&mut tree, root, "Title", "Milk").unwrap();
        let node = tree.get(field).unwrap();
        assert_eq!(node.kind, NodeKind::TextField);
        assert_eq!(node.name.as_deref(), Some("Title"));
        assert_eq!(node.text.as_deref(), Some("Milk"));
        assert_eq!(node.caret, 4);
        assert_eq!(node.fill, Some(Color::FIELD));
    }

    #[test]
    fn tall_list_uses_scroll_and_keeps_row_ids() {
        let mut tree = Tree::new();
        let root = window(&mut tree, "Items", 320.0, 400.0).unwrap();
        let rows = [
            ListRow {
                label: "Short",
                height: 36.0,
            },
            ListRow {
                label: "Tall note",
                height: 96.0,
            },
            ListRow {
                label: "Last",
                height: 48.0,
            },
        ];
        let built = list(&mut tree, root, f32::NAN, 100.0, &rows).unwrap();
        let scroll_id = built.scroll.expect("taller than viewport");
        assert_eq!(tree.get(scroll_id).unwrap().kind, NodeKind::Scroll);
        assert_eq!(tree.get(built.list).unwrap().parent, Some(scroll_id));
        assert_eq!(tree.get(built.list).unwrap().kind, NodeKind::List);
        let children = tree.get(built.list).unwrap().children.clone();
        assert_eq!(children.len(), 3);
        assert_eq!(
            tree.get(children[0]).unwrap().text.as_deref(),
            Some("Short")
        );
        assert_eq!(tree.get(children[1]).unwrap().layout.height, 96.0);
        assert_eq!(tree.get(children[2]).unwrap().layout.height, 48.0);
        assert_eq!(tree.get(children[0]).unwrap().kind, NodeKind::ListItem);
        let extra = list_item(&mut tree, built.list, "More", 40.0).unwrap();
        assert_ne!(extra, children[0]);
        assert!(tree.get(children[0]).is_some());
        assert_eq!(tree.get(extra).unwrap().parent, Some(built.list));
    }

    #[test]
    fn short_list_is_not_scrolled() {
        let mut tree = Tree::new();
        let root = window(&mut tree, "Items", 320.0, 400.0).unwrap();
        let built = list(
            &mut tree,
            root,
            200.0,
            200.0,
            &[ListRow {
                label: "Only",
                height: 40.0,
            }],
        )
        .unwrap();
        assert!(built.scroll.is_none());
        assert_eq!(tree.get(built.list).unwrap().parent, Some(root));
    }

    #[test]
    fn appending_rows_wraps_an_empty_list_in_scroll() {
        let mut tree = Tree::new();
        let root = window(&mut tree, "Items", 320.0, 400.0).unwrap();
        let built = list(&mut tree, root, 200.0, 100.0, &[]).unwrap();
        assert!(built.scroll.is_none());
        let first = list_item(&mut tree, built.list, "One", 50.0).unwrap();
        assert_eq!(tree.get(built.list).unwrap().parent, Some(root));
        let second = list_item(&mut tree, built.list, "Two", 50.0).unwrap();
        let third = list_item(&mut tree, built.list, "Three", 50.0).unwrap();
        let scroll_id = tree.get(built.list).unwrap().parent.unwrap();
        assert_ne!(scroll_id, root);
        assert_eq!(tree.get(scroll_id).unwrap().kind, NodeKind::Scroll);
        assert_eq!(tree.get(scroll_id).unwrap().layout.height, 100.0);
        assert_eq!(tree.get(scroll_id).unwrap().parent, Some(root));
        assert_eq!(
            tree.get(built.list).unwrap().children,
            vec![first, second, third]
        );
        assert_eq!(tree.get(first).unwrap().kind, NodeKind::ListItem);
        let again = list_item(&mut tree, built.list, "Four", 50.0).unwrap();
        assert_eq!(tree.get(built.list).unwrap().parent, Some(scroll_id));
        assert_eq!(tree.get(again).unwrap().parent, Some(built.list));
    }

    #[test]
    fn short_auto_height_row_stays_unwrapped() {
        let mut tree = Tree::new();
        let root = window(&mut tree, "Items", 320.0, 400.0).unwrap();
        let built = list(&mut tree, root, 200.0, 100.0, &[]).unwrap();
        list_item(&mut tree, built.list, "Hi", f32::NAN).unwrap();
        assert_eq!(tree.get(built.list).unwrap().parent, Some(root));
        assert!(scroll_parent(&tree, built.list).is_none());
    }

    #[test]
    fn overflowing_auto_height_rows_wrap_once() {
        let mut tree = Tree::new();
        let root = window(&mut tree, "Items", 320.0, 400.0).unwrap();
        let built = list(&mut tree, root, 200.0, 100.0, &[]).unwrap();
        for label in ["One", "Two", "Three", "Four", "Five", "Six"] {
            list_item(&mut tree, built.list, label, f32::NAN).unwrap();
        }
        let scroll_id = scroll_parent(&tree, built.list).expect("rows exceed 100px");
        assert_eq!(tree.get(scroll_id).unwrap().parent, Some(root));
        assert_eq!(tree.get(scroll_id).unwrap().layout.height, 100.0);
        list_item(&mut tree, built.list, "Seven", f32::NAN).unwrap();
        assert_eq!(tree.get(built.list).unwrap().parent, Some(scroll_id));
        let scrolls = tree
            .get(root)
            .unwrap()
            .children
            .iter()
            .filter(|id| tree.get(**id).unwrap().kind == NodeKind::Scroll)
            .count();
        assert_eq!(scrolls, 1);
    }
}
