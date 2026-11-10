//! Layout pass: map the scene tree to Taffy, then write bounds back.
//!
//! Text nodes are measured with Parley (via `tekto-text`) so width/height
//! come from real glyph layout, not a font-size guess.

use std::cell::RefCell;
use taffy::compute_leaf_layout;
use taffy::prelude::{
    AvailableSpace, Dimension, Display, FlexDirection, JustifyContent, LengthPercentage,
    LengthPercentageAuto, Rect, Size, Style, TaffyTree,
};
use tekto_text::TextEngine;
use tekto_tree::{NodeId, NodeKind, Tree, LIST_OVERSCAN_PX};

thread_local! {
    static TEXT: RefCell<TextEngine> = RefCell::new(TextEngine::new());
}

/// Compute bounds for every node in a viewport of
/// `viewport_width` x `viewport_height` logical pixels.
pub fn layout_tree(tree: &mut Tree, viewport_width: f32, viewport_height: f32) {
    TEXT.with(|engine| {
        layout_tree_with_text(
            tree,
            viewport_width,
            viewport_height,
            &mut engine.borrow_mut(),
        )
    });
}

/// Like [`layout_tree`], but uses a caller-provided text engine (useful in tests).
pub fn layout_tree_with_text(
    tree: &mut Tree,
    viewport_width: f32,
    viewport_height: f32,
    text: &mut TextEngine,
) {
    let Some(root) = tree.root() else {
        return;
    };
    virtualize_lists(tree, text);
    // The viewport is the window: keep the root's layout size in sync so a
    // resize actually relayouts children (padding, banner width, and so on).
    if let Some(n) = tree.get_mut(root) {
        n.layout.width = viewport_width;
        n.layout.height = viewport_height;
    }
    let mut taffy: TaffyTree<NodeId> = TaffyTree::new();
    let mut map: Vec<(NodeId, taffy::NodeId)> = Vec::new();
    let taffy_root = build(&mut taffy, tree, root, &mut map);
    let _ = taffy.compute_layout_with_measure(
        taffy_root,
        Size {
            width: AvailableSpace::Definite(viewport_width),
            height: AvailableSpace::Definite(viewport_height),
        },
        |inputs, _tid, ctx, style| {
            compute_leaf_layout(
                inputs,
                style,
                |_, _| 0.0,
                |known, avail| {
                    let Some(id) = ctx.copied() else {
                        return Size::ZERO;
                    };
                    measure_text(tree, text, id, known, avail)
                },
            )
        },
    );
    write_bounds(&taffy, tree, root, 0.0, 0.0, &map);
    place_carets(tree, text);
}

fn place_carets(tree: &mut Tree, text: &mut TextEngine) {
    let ids: Vec<NodeId> = tree
        .ids()
        .filter(|id| tree.get(*id).is_some_and(|n| n.kind == NodeKind::TextField))
        .collect();
    for id in ids {
        let Some(node) = tree.get(id) else {
            continue;
        };
        let value = node.composed_text();
        let caret = node.composed_caret().unwrap_or_else(|| {
            node.caret
                .min(node.text.as_deref().unwrap_or("").chars().count())
        });
        let prefix: String = value.chars().take(caret).collect();
        let max_w = {
            let inner = node.bounds.width - 2.0 * node.layout.padding;
            if inner > 0.0 {
                Some(inner)
            } else {
                None
            }
        };
        let size = node.font_size;
        let (w, _) = text.measure(&prefix, size, max_w);
        if let Some(node) = tree.get_mut(id) {
            node.caret = caret;
            node.caret_x = w;
        }
    }
}

fn measure_text(
    tree: &Tree,
    text: &mut TextEngine,
    id: NodeId,
    known: Size<Option<f32>>,
    avail: Size<AvailableSpace>,
) -> Size<f32> {
    let Some(node) = tree.get(id) else {
        return Size::ZERO;
    };
    if !matches!(
        node.kind,
        NodeKind::Text | NodeKind::Button | NodeKind::TextField | NodeKind::ListItem
    ) {
        return Size::ZERO;
    }
    let composed;
    let content = if node.kind == NodeKind::TextField {
        composed = node.composed_text();
        composed.as_str()
    } else {
        node.text.as_deref().unwrap_or("")
    };
    // An empty field still needs a line box so the caret has somewhere to sit.
    let sample = if content.is_empty() { " " } else { content };
    let max_width = known.width.or_else(|| match avail.width {
        AvailableSpace::Definite(w) => Some(w.max(0.0)),
        AvailableSpace::MinContent => Some(0.0),
        AvailableSpace::MaxContent => None,
    });
    let (w, h) = text.measure(sample, node.font_size, max_width);
    let w = if content.is_empty() { 0.0 } else { w };
    Size {
        width: known.width.unwrap_or(w),
        height: known.height.unwrap_or(h.max(1.0)),
    }
}

fn style_for(tree: &Tree, id: NodeId, is_root: bool) -> Style {
    let Some(node) = tree.get(id) else {
        return Style::DEFAULT;
    };
    let ls = node.layout;
    let size = Size {
        width: if ls.is_width_set() {
            Dimension::length(ls.width)
        } else if is_root {
            Dimension::percent(1.0)
        } else {
            Dimension::auto()
        },
        height: if ls.is_height_set() {
            Dimension::length(ls.height)
        } else if is_root {
            Dimension::percent(1.0)
        } else {
            // Text height comes from the Parley measure function.
            Dimension::auto()
        },
    };
    Style {
        display: Display::Flex,
        flex_direction: if ls.column {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        },
        size,
        padding: length(ls.padding),
        gap: Size {
            width: LengthPercentage::length(ls.gap),
            height: LengthPercentage::length(ls.gap),
        },
        justify_content: if ls.justify_center {
            JustifyContent::CENTER
        } else {
            JustifyContent::START
        },
        // A scroll viewport has a fixed height. Children keep their own
        // height and paint past the bottom; the renderer clips them.
        flex_shrink: if parent_is_scroll(tree, id) { 0.0 } else { 1.0 },
        min_size: Size {
            width: LengthPercentageAuto::auto(),
            height: if parent_is_scroll(tree, id) && ls.is_height_set() {
                LengthPercentageAuto::length(ls.height)
            } else {
                LengthPercentageAuto::auto()
            },
        },
        ..Style::DEFAULT
    }
}

fn parent_is_scroll(tree: &Tree, id: NodeId) -> bool {
    tree.get(id)
        .and_then(|n| n.parent)
        .and_then(|p| tree.get(p))
        .is_some_and(|p| p.kind == NodeKind::Scroll)
}

fn length(value: f32) -> Rect<LengthPercentage> {
    Rect {
        left: LengthPercentage::length(value),
        right: LengthPercentage::length(value),
        top: LengthPercentage::length(value),
        bottom: LengthPercentage::length(value),
    }
}

fn build(
    taffy: &mut TaffyTree<NodeId>,
    tree: &Tree,
    id: NodeId,
    map: &mut Vec<(NodeId, taffy::NodeId)>,
) -> taffy::NodeId {
    let is_root = tree.root() == Some(id);
    let style = style_for(tree, id, is_root);
    let child_ids: Vec<NodeId> = tree.get(id).map(|n| n.children.clone()).unwrap_or_default();
    let tid = if child_ids.is_empty() {
        let is_text = tree.get(id).is_some_and(|n| {
            matches!(
                n.kind,
                NodeKind::Text | NodeKind::Button | NodeKind::TextField | NodeKind::ListItem
            )
        });
        if is_text {
            taffy.new_leaf_with_context(style, id).expect("taffy")
        } else {
            taffy.new_leaf(style).expect("taffy")
        }
    } else {
        let children: Vec<taffy::NodeId> = child_ids
            .iter()
            .map(|c| build(taffy, tree, *c, map))
            .collect();
        taffy.new_with_children(style, &children).expect("taffy")
    };
    map.push((id, tid));
    tid
}

fn write_bounds(
    taffy: &TaffyTree<NodeId>,
    tree: &mut Tree,
    id: NodeId,
    parent_x: f32,
    parent_y: f32,
    map: &[(NodeId, taffy::NodeId)],
) {
    let Some((_, tid)) = map.iter().find(|(n, _)| *n == id) else {
        return;
    };
    let layout = taffy.layout(*tid).expect("layout");
    let mut x = parent_x + layout.location.x;
    let mut y = parent_y + layout.location.y;
    // A finished position transition stores its target on the layout style.
    // Keep that point so the next layout does not restore the flex origin.
    if let Some(node) = tree.get(id) {
        if node.layout.x.is_finite() {
            x = node.layout.x;
        }
        if node.layout.y.is_finite() {
            y = node.layout.y;
        }
    }
    if let Some(node) = tree.get_mut(id) {
        node.bounds = tekto_tree::Rect::new(x, y, layout.size.width, layout.size.height);
    }
    let children: Vec<NodeId> = tree.get(id).map(|n| n.children.clone()).unwrap_or_default();
    // Shift descendants of a scroll container. Taffy still laid them out at
    // the unshifted positions, so the extent is measured from those.
    let mut child_origin_y = y;
    if tree.get(id).is_some_and(|n| n.kind == NodeKind::Scroll) {
        let pad = tree.get(id).map(|n| n.layout.padding).unwrap_or(0.0);
        let mut bottom = 0.0_f32;
        for child in &children {
            let Some((_, tid)) = map.iter().find(|(n, _)| n == child) else {
                continue;
            };
            let Ok(child_layout) = taffy.layout(*tid) else {
                continue;
            };
            bottom = bottom.max(child_layout.location.y + child_layout.size.height + pad);
        }
        if let Some(node) = tree.get_mut(id) {
            node.scroll_extent = bottom.max(node.bounds.height);
            let max = (node.scroll_extent - node.bounds.height).max(0.0);
            if !node.scroll_y.is_finite() {
                node.scroll_y = 0.0;
            }
            node.scroll_y = node.scroll_y.clamp(0.0, max);
            child_origin_y = y - node.scroll_y;
        }
    }
    for child in children {
        write_bounds(taffy, tree, child, x, child_origin_y, map);
    }
}

/// Keep only the rows that intersect the scroll viewport, plus
/// [`LIST_OVERSCAN_PX`]. Top and bottom spacers preserve the full content
/// height so `scroll_y` stays put when the window jumps.
fn virtualize_lists(tree: &mut Tree, text: &mut TextEngine) {
    let lists: Vec<NodeId> = tree
        .ids()
        .filter(|id| tree.get(*id).is_some_and(|n| n.kind == NodeKind::List))
        .collect();
    for id in lists {
        virtualize_list(tree, text, id);
    }
}

fn virtualize_list(tree: &mut Tree, text: &mut TextEngine, list_id: NodeId) {
    let Some(list) = tree.get(list_id) else {
        return;
    };
    let Some(scroll_id) = list.parent.filter(|parent| {
        tree.get(*parent)
            .is_some_and(|node| node.kind == NodeKind::Scroll)
    }) else {
        return;
    };
    let viewport = list_viewport_px(tree, list_id);
    if !viewport.is_finite() || viewport <= 0.0 {
        return;
    }

    let mut slots = list.list_slots.clone();
    for child in &list.children {
        if tree
            .get(*child)
            .is_some_and(|n| n.kind == NodeKind::ListItem)
            && !slots.contains(child)
        {
            slots.push(*child);
        }
    }
    slots.retain(|id| tree.get(*id).is_some_and(|n| n.kind == NodeKind::ListItem));
    if slots.is_empty() {
        if let Some(node) = tree.get_mut(list_id) {
            node.list_slots.clear();
        }
        return;
    }

    let gap = list.layout.gap;
    let gap = if gap.is_finite() { gap.max(0.0) } else { 0.0 };
    let list_width = list.layout.width;
    let mut heights = Vec::with_capacity(slots.len());
    for id in &slots {
        let height = row_box_height(tree, text, *id, list_width);
        if !height.is_finite() {
            reattach_all(tree, list_id, &slots);
            return;
        }
        heights.push(height);
    }

    let mut starts = Vec::with_capacity(heights.len());
    let mut cursor = 0.0_f32;
    for (index, height) in heights.iter().copied().enumerate() {
        starts.push(cursor);
        cursor += height;
        if index + 1 != heights.len() {
            cursor += gap;
        }
    }
    let total = cursor;

    if let Some(node) = tree.get_mut(list_id) {
        node.list_slots = slots.clone();
    }

    if total <= viewport + 0.5 {
        reattach_all(tree, list_id, &slots);
        return;
    }

    let scroll_y = tree.get(scroll_id).map(|node| node.scroll_y).unwrap_or(0.0);
    let scroll_y = if scroll_y.is_finite() {
        scroll_y.max(0.0)
    } else {
        0.0
    };
    let window_top = (scroll_y - LIST_OVERSCAN_PX).max(0.0);
    let window_bottom = scroll_y + viewport + LIST_OVERSCAN_PX;

    let mut first = None;
    let mut last = None;
    for index in 0..slots.len() {
        let end = starts[index] + heights[index];
        if end > window_top && starts[index] < window_bottom {
            if first.is_none() {
                first = Some(index);
            }
            last = Some(index);
        }
    }
    let (Some(first), Some(last)) = (first, last) else {
        reattach_all(tree, list_id, &slots);
        return;
    };

    let mut children = Vec::new();
    if first > 0 {
        let pad = (starts[first] - gap).max(0.0);
        if pad > 0.5 {
            if let Some(id) = ensure_pad(tree, list_id, true, pad, list_width) {
                children.push(id);
            }
        } else {
            detach_pad(tree, list_id, true);
        }
    } else {
        detach_pad(tree, list_id, true);
    }
    children.extend(slots[first..=last].iter().copied());
    if last + 1 < slots.len() {
        let last_end = starts[last] + heights[last];
        let pad = (total - last_end - gap).max(0.0);
        if pad > 0.5 {
            if let Some(id) = ensure_pad(tree, list_id, false, pad, list_width) {
                children.push(id);
            }
        } else {
            detach_pad(tree, list_id, false);
        }
    } else {
        detach_pad(tree, list_id, false);
    }
    assign_list_children(tree, list_id, &slots, &children);
}

fn list_viewport_px(tree: &Tree, list_id: NodeId) -> f32 {
    let Some(list) = tree.get(list_id) else {
        return f32::NAN;
    };
    if list.list_viewport.is_finite() && list.list_viewport > 0.0 {
        return list.list_viewport;
    }
    let Some(parent) = list.parent.and_then(|id| tree.get(id)) else {
        return f32::NAN;
    };
    if parent.kind != NodeKind::Scroll {
        return f32::NAN;
    }
    if parent.layout.height.is_finite() && parent.layout.height > 0.0 {
        parent.layout.height
    } else if parent.bounds.height.is_finite() && parent.bounds.height > 0.0 {
        parent.bounds.height
    } else {
        f32::NAN
    }
}

fn row_box_height(tree: &Tree, text: &mut TextEngine, id: NodeId, list_width: f32) -> f32 {
    let Some(node) = tree.get(id) else {
        return 0.0;
    };
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
    let (_, measured) = text.measure(sample, node.font_size, max_width);
    measured.max(1.0) + 2.0 * pad
}

fn reattach_all(tree: &mut Tree, list_id: NodeId, slots: &[NodeId]) {
    detach_pad(tree, list_id, true);
    detach_pad(tree, list_id, false);
    if let Some(node) = tree.get_mut(list_id) {
        node.list_slots = slots.to_vec();
    }
    assign_list_children(tree, list_id, slots, slots);
}

fn ensure_pad(
    tree: &mut Tree,
    list_id: NodeId,
    top: bool,
    height: f32,
    width: f32,
) -> Option<NodeId> {
    let raw = tree.get(list_id).map(|node| {
        if top {
            node.list_pad_top
        } else {
            node.list_pad_bottom
        }
    })?;
    let existing = NodeId::from_raw(raw);
    let id = if raw != 0 && tree.get(existing).is_some() {
        existing
    } else {
        let created = tree.add(NodeKind::Rect, None).ok()?;
        if let Some(list) = tree.get_mut(list_id) {
            if top {
                list.list_pad_top = created.raw();
            } else {
                list.list_pad_bottom = created.raw();
            }
        }
        created
    };
    if let Some(node) = tree.get_mut(id) {
        node.name = Some(
            if top {
                "list-pad-top"
            } else {
                "list-pad-bottom"
            }
            .into(),
        );
        node.fill = None;
        node.text = None;
        node.layout.width = if width.is_finite() { width } else { 0.0 };
        node.layout.height = height;
        node.layout.padding = 0.0;
        node.layout.gap = 0.0;
        node.layout.column = true;
    }
    Some(id)
}

fn detach_pad(tree: &mut Tree, list_id: NodeId, top: bool) {
    let raw = tree
        .get(list_id)
        .map(|node| {
            if top {
                node.list_pad_top
            } else {
                node.list_pad_bottom
            }
        })
        .unwrap_or(0);
    if raw == 0 {
        return;
    }
    let id = NodeId::from_raw(raw);
    if tree
        .get(id)
        .is_some_and(|node| node.parent == Some(list_id))
    {
        if let Some(list) = tree.get_mut(list_id) {
            list.children.retain(|child| *child != id);
        }
        if let Some(node) = tree.get_mut(id) {
            node.parent = None;
        }
    }
}

fn assign_list_children(tree: &mut Tree, list_id: NodeId, slots: &[NodeId], children: &[NodeId]) {
    let previous = tree
        .get(list_id)
        .map(|node| node.children.clone())
        .unwrap_or_default();
    let top = tree.get(list_id).map(|node| node.list_pad_top).unwrap_or(0);
    let bottom = tree
        .get(list_id)
        .map(|node| node.list_pad_bottom)
        .unwrap_or(0);
    let mut managed = previous;
    managed.extend(slots.iter().copied());
    if top != 0 {
        managed.push(NodeId::from_raw(top));
    }
    if bottom != 0 {
        managed.push(NodeId::from_raw(bottom));
    }
    for id in managed {
        if children.contains(&id) {
            continue;
        }
        if tree
            .get(id)
            .is_some_and(|node| node.parent == Some(list_id))
        {
            if let Some(node) = tree.get_mut(id) {
                node.parent = None;
            }
        }
    }
    if let Some(node) = tree.get_mut(list_id) {
        node.children = children.to_vec();
    }
    for id in children {
        if let Some(node) = tree.get_mut(*id) {
            node.parent = Some(list_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tekto_tree::{LayoutStyle, NodeKind};

    #[test]
    fn column_stacks_children() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().layout = LayoutStyle {
            width: 200.0,
            height: 200.0,
            padding: 10.0,
            gap: 8.0,
            column: true,
            justify_center: false,
            x: f32::NAN,
            y: f32::NAN,
        };
        let a = tree.add(NodeKind::Rect, Some(root)).unwrap();
        tree.get_mut(a).unwrap().layout = LayoutStyle::sized(180.0, 40.0);
        let b = tree.add(NodeKind::Rect, Some(root)).unwrap();
        tree.get_mut(b).unwrap().layout = LayoutStyle::sized(180.0, 40.0);
        layout_tree(&mut tree, 200.0, 200.0);
        let ra = tree.get(a).unwrap().bounds;
        let rb = tree.get(b).unwrap().bounds;
        assert!(rb.y >= ra.y + ra.height);
        assert_eq!(ra.x, rb.x);
    }

    #[test]
    fn text_height_comes_from_parley_not_a_guess() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().layout = LayoutStyle {
            width: 400.0,
            height: 200.0,
            padding: 0.0,
            gap: 0.0,
            column: true,
            justify_center: false,
            x: f32::NAN,
            y: f32::NAN,
        };
        let t = tree.add(NodeKind::Text, Some(root)).unwrap();
        {
            let n = tree.get_mut(t).unwrap();
            n.text = Some("Hello from Tekto".into());
            n.font_size = 20.0;
            // No explicit height — measure must provide one.
        }
        layout_tree(&mut tree, 400.0, 200.0);
        let b = tree.get(t).unwrap().bounds;
        assert!(b.width > 10.0, "width was {}", b.width);
        assert!(b.height > 16.0, "height was {}", b.height);
        // A one-line 20px string should be much shorter than the old 1.4 guess * many lines.
        assert!(b.height < 50.0, "height was {}", b.height);

        // Wrapping should make a long string taller than a short one.
        let long = tree.add(NodeKind::Text, Some(root)).unwrap();
        {
            let n = tree.get_mut(long).unwrap();
            n.text = Some("word ".repeat(40).trim().to_string());
            n.font_size = 20.0;
            n.layout.width = 200.0;
        }
        layout_tree(&mut tree, 400.0, 400.0);
        let short_h = tree.get(t).unwrap().bounds.height;
        let long_h = tree.get(long).unwrap().bounds.height;
        assert!(
            long_h > short_h + 10.0,
            "expected wrapped text to be taller: short={short_h} long={long_h}"
        );
    }

    #[test]
    fn viewport_resize_relayouts_root() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().layout = LayoutStyle {
            width: 200.0,
            height: 100.0,
            padding: 0.0,
            gap: 0.0,
            column: true,
            justify_center: false,
            x: f32::NAN,
            y: f32::NAN,
        };
        layout_tree(&mut tree, 200.0, 100.0);
        assert_eq!(tree.get(root).unwrap().bounds.width, 200.0);
        layout_tree(&mut tree, 400.0, 250.0);
        let b = tree.get(root).unwrap().bounds;
        assert_eq!(b.width, 400.0);
        assert_eq!(b.height, 250.0);
    }

    #[test]
    fn auto_width_child_shrinks_with_the_viewport() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().layout = LayoutStyle {
            width: 640.0,
            height: 400.0,
            padding: 24.0,
            gap: 0.0,
            column: true,
            justify_center: false,
            x: f32::NAN,
            y: f32::NAN,
        };
        let child = tree.add(NodeKind::Rect, Some(root)).unwrap();
        tree.get_mut(child).unwrap().layout = LayoutStyle {
            width: f32::NAN,
            height: 64.0,
            ..LayoutStyle::default()
        };
        layout_tree(&mut tree, 640.0, 400.0);
        let wide = tree.get(child).unwrap().bounds;
        assert!(
            (wide.width - 592.0).abs() < 0.5,
            "wide width was {}",
            wide.width
        );

        layout_tree(&mut tree, 393.0, 807.0);
        let root_b = tree.get(root).unwrap().bounds;
        let narrow = tree.get(child).unwrap().bounds;
        assert_eq!(root_b.width, 393.0);
        assert!(
            narrow.x + narrow.width <= root_b.width + 0.5,
            "child overflowed: x={} w={} parent={}",
            narrow.x,
            narrow.width,
            root_b.width
        );
        assert!(
            (narrow.width - 345.0).abs() < 0.5,
            "expected 393-48=345, got {}",
            narrow.width
        );
    }

    #[test]
    fn finished_position_and_size_survive_another_layout() {
        use tekto_tree::StyleTransition;

        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().layout = LayoutStyle {
            width: 200.0,
            height: 120.0,
            padding: 0.0,
            gap: 0.0,
            column: true,
            justify_center: false,
            x: f32::NAN,
            y: f32::NAN,
        };
        let id = tree.add(NodeKind::Rect, Some(root)).unwrap();
        tree.get_mut(id).unwrap().layout = LayoutStyle::sized(40.0, 16.0);
        layout_tree(&mut tree, 200.0, 120.0);

        tree.transition(
            id,
            StyleTransition {
                x: Some(80.0),
                y: Some(30.0),
                width: Some(90.0),
                height: Some(24.0),
                duration_ms: Some(100.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();
        assert!(tree.advance(0.0));
        layout_tree(&mut tree, 200.0, 120.0);
        assert!(tree.advance(0.0));
        assert!(!tree.advance(100.0));
        layout_tree(&mut tree, 200.0, 120.0);
        let end = tree.get(id).unwrap().bounds;
        assert!((end.x - 80.0).abs() < 0.01, "x {}", end.x);
        assert!((end.y - 30.0).abs() < 0.01, "y {}", end.y);
        assert!((end.width - 90.0).abs() < 0.01, "w {}", end.width);
        assert!((end.height - 24.0).abs() < 0.01, "h {}", end.height);

        // A second layout, with no running tween, still shows the target.
        layout_tree(&mut tree, 200.0, 120.0);
        let again = tree.get(id).unwrap().bounds;
        assert!((again.x - 80.0).abs() < 0.01);
        assert!((again.y - 30.0).abs() < 0.01);
        assert!((again.width - 90.0).abs() < 0.01);
        assert!((again.height - 24.0).abs() < 0.01);
    }

    #[test]
    fn zero_duration_and_reduced_motion_targets_survive_layout() {
        use tekto_tree::StyleTransition;

        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().layout = LayoutStyle {
            width: 200.0,
            height: 120.0,
            ..LayoutStyle::default()
        };
        let id = tree.add(NodeKind::Rect, Some(root)).unwrap();
        tree.get_mut(id).unwrap().layout = LayoutStyle::sized(40.0, 16.0);
        layout_tree(&mut tree, 200.0, 120.0);
        tree.transition(
            id,
            StyleTransition {
                x: Some(12.0),
                width: Some(70.0),
                duration_ms: Some(0.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();
        layout_tree(&mut tree, 200.0, 120.0);
        let snapped = tree.get(id).unwrap().bounds;
        assert!((snapped.x - 12.0).abs() < 0.01, "x {}", snapped.x);
        assert!((snapped.width - 70.0).abs() < 0.01, "w {}", snapped.width);

        tree.set_reduced_motion(true);
        tree.transition(
            id,
            StyleTransition {
                y: Some(44.0),
                height: Some(28.0),
                duration_ms: Some(400.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();
        layout_tree(&mut tree, 200.0, 120.0);
        let reduced = tree.get(id).unwrap().bounds;
        assert!((reduced.x - 12.0).abs() < 0.01);
        assert!((reduced.y - 44.0).abs() < 0.01, "y {}", reduced.y);
        assert!((reduced.width - 70.0).abs() < 0.01);
        assert!((reduced.height - 28.0).abs() < 0.01, "h {}", reduced.height);
    }

    #[test]
    fn parent_transition_keeps_child_with_the_parent() {
        use tekto_tree::StyleTransition;

        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().layout = LayoutStyle {
            width: 300.0,
            height: 200.0,
            padding: 0.0,
            gap: 0.0,
            column: true,
            justify_center: false,
            x: f32::NAN,
            y: f32::NAN,
        };
        let parent = tree.add(NodeKind::Rect, Some(root)).unwrap();
        tree.get_mut(parent).unwrap().layout = LayoutStyle {
            width: 80.0,
            height: 40.0,
            padding: 4.0,
            gap: 0.0,
            column: true,
            justify_center: false,
            x: f32::NAN,
            y: f32::NAN,
        };
        let child = tree.add(NodeKind::Rect, Some(parent)).unwrap();
        tree.get_mut(child).unwrap().layout = LayoutStyle::sized(20.0, 10.0);
        layout_tree(&mut tree, 300.0, 200.0);

        let parent_start = tree.get(parent).unwrap().bounds;
        let child_start = tree.get(child).unwrap().bounds;
        let offset_x = child_start.x - parent_start.x;
        let offset_y = child_start.y - parent_start.y;
        assert!(offset_x.abs() > 0.5 || offset_y.abs() > 0.5);

        tree.transition(
            parent,
            StyleTransition {
                x: Some(parent_start.x + 40.0),
                y: Some(parent_start.y + 24.0),
                width: Some(parent_start.width + 30.0),
                height: Some(parent_start.height + 16.0),
                duration_ms: Some(100.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();

        let assert_child_follows = |tree: &Tree, label: &str| {
            let parent_b = tree.get(parent).unwrap().bounds;
            let child_b = tree.get(child).unwrap().bounds;
            let dx = child_b.x - parent_b.x;
            let dy = child_b.y - parent_b.y;
            assert!(
                (dx - offset_x).abs() < 0.01,
                "{label} child x offset {dx}, expected {offset_x}"
            );
            assert!(
                (dy - offset_y).abs() < 0.01,
                "{label} child y offset {dy}, expected {offset_y}"
            );
            assert!((child_b.width - child_start.width).abs() < 0.01);
            assert!((child_b.height - child_start.height).abs() < 0.01);
        };

        // Same order as a desktop frame: layout, then advance.
        layout_tree(&mut tree, 300.0, 200.0);
        assert!(tree.advance(0.0));
        assert_child_follows(&tree, "start");

        layout_tree(&mut tree, 300.0, 200.0);
        assert!(tree.advance(50.0));
        let mid = tree.get(parent).unwrap().bounds;
        assert!(
            mid.x > parent_start.x + 10.0 && mid.x < parent_start.x + 30.0,
            "mid x {}",
            mid.x
        );
        assert!(
            mid.y > parent_start.y + 6.0 && mid.y < parent_start.y + 18.0,
            "mid y {}",
            mid.y
        );
        assert!(
            mid.width > parent_start.width + 8.0 && mid.width < parent_start.width + 22.0,
            "mid w {}",
            mid.width
        );
        assert_child_follows(&tree, "midpoint");

        layout_tree(&mut tree, 300.0, 200.0);
        assert!(!tree.advance(100.0));
        let end = tree.get(parent).unwrap().bounds;
        assert!((end.x - (parent_start.x + 40.0)).abs() < 0.01);
        assert!((end.y - (parent_start.y + 24.0)).abs() < 0.01);
        assert!((end.width - (parent_start.width + 30.0)).abs() < 0.01);
        assert!((end.height - (parent_start.height + 16.0)).abs() < 0.01);
        assert_child_follows(&tree, "completion");

        layout_tree(&mut tree, 300.0, 200.0);
        let again = tree.get(parent).unwrap().bounds;
        assert!((again.x - end.x).abs() < 0.01);
        assert!((again.y - end.y).abs() < 0.01);
        assert!((again.width - end.width).abs() < 0.01);
        assert!((again.height - end.height).abs() < 0.01);
        assert_child_follows(&tree, "later layout");
    }

    /// A child that already has a stored absolute position must move with a
    /// parent translation. The next layout must not restore the old absolute.
    #[test]
    fn persisted_child_stays_with_parent_after_transition() {
        use tekto_tree::StyleTransition;

        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().layout = LayoutStyle {
            width: 200.0,
            height: 120.0,
            padding: 0.0,
            gap: 0.0,
            column: true,
            justify_center: false,
            x: f32::NAN,
            y: f32::NAN,
        };
        let parent = tree.add(NodeKind::Rect, Some(root)).unwrap();
        tree.get_mut(parent).unwrap().layout = LayoutStyle::sized(80.0, 40.0);
        let child = tree.add(NodeKind::Rect, Some(parent)).unwrap();
        tree.get_mut(child).unwrap().layout = LayoutStyle::sized(20.0, 10.0);
        layout_tree(&mut tree, 200.0, 120.0);

        tree.transition(
            child,
            StyleTransition {
                x: Some(10.0),
                duration_ms: Some(0.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();
        layout_tree(&mut tree, 200.0, 120.0);
        let parent_start = tree.get(parent).unwrap().bounds;
        let child_start = tree.get(child).unwrap().bounds;
        assert!(
            parent_start.x.abs() < 0.01,
            "parent start x {}",
            parent_start.x
        );
        assert!(
            (child_start.x - 10.0).abs() < 0.01,
            "child start x {}",
            child_start.x
        );

        tree.transition(
            parent,
            StyleTransition {
                x: Some(40.0),
                duration_ms: Some(100.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();

        layout_tree(&mut tree, 200.0, 120.0);
        assert!(tree.advance(0.0));
        assert!((tree.get(parent).unwrap().bounds.x - 0.0).abs() < 0.01);
        assert!((tree.get(child).unwrap().bounds.x - 10.0).abs() < 0.01);

        layout_tree(&mut tree, 200.0, 120.0);
        assert!(tree.advance(50.0));
        assert!(
            (tree.get(parent).unwrap().bounds.x - 20.0).abs() < 0.01,
            "mid parent {}",
            tree.get(parent).unwrap().bounds.x
        );
        assert!(
            (tree.get(child).unwrap().bounds.x - 30.0).abs() < 0.01,
            "mid child {}",
            tree.get(child).unwrap().bounds.x
        );

        layout_tree(&mut tree, 200.0, 120.0);
        assert!(!tree.advance(100.0));
        assert!((tree.get(parent).unwrap().bounds.x - 40.0).abs() < 0.01);
        assert!((tree.get(child).unwrap().bounds.x - 50.0).abs() < 0.01);

        layout_tree(&mut tree, 200.0, 120.0);
        let parent_again = tree.get(parent).unwrap().bounds;
        let child_again = tree.get(child).unwrap().bounds;
        assert!(
            (parent_again.x - 40.0).abs() < 0.01,
            "later parent {}",
            parent_again.x
        );
        assert!(
            (child_again.x - 50.0).abs() < 0.01,
            "later child {} detached from parent",
            child_again.x
        );
        assert!((child_again.y - child_start.y).abs() < 0.01);
    }
}
