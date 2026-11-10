//! Layout pass: map the scene tree to Taffy, then write bounds back.
//!
//! Text nodes are measured with Parley (via `tekto-text`) so width/height
//! come from real glyph layout, not a font-size guess.

use std::cell::RefCell;
use taffy::compute_leaf_layout;
use taffy::prelude::{
    AvailableSpace, Dimension, Display, FlexDirection, JustifyContent, LengthPercentage, Rect,
    Size, Style, TaffyTree,
};
use tekto_text::TextEngine;
use tekto_tree::{NodeId, NodeKind, Tree};

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
    if !matches!(node.kind, NodeKind::Text) {
        return Size::ZERO;
    }
    let content = node.text.as_deref().unwrap_or("");
    let max_width = known.width.or_else(|| match avail.width {
        AvailableSpace::Definite(w) => Some(w.max(0.0)),
        AvailableSpace::MinContent => Some(0.0),
        AvailableSpace::MaxContent => None,
    });
    let (w, h) = text.measure(content, node.font_size, max_width);
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
        ..Style::DEFAULT
    }
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
        let is_text = tree
            .get(id)
            .is_some_and(|n| matches!(n.kind, NodeKind::Text));
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
    let x = parent_x + layout.location.x;
    let y = parent_y + layout.location.y;
    if let Some(node) = tree.get_mut(id) {
        node.bounds = tekto_tree::Rect::new(x, y, layout.size.width, layout.size.height);
    }
    let children: Vec<NodeId> = tree.get(id).map(|n| n.children.clone()).unwrap_or_default();
    for child in children {
        write_bounds(taffy, tree, child, x, y, map);
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
}
