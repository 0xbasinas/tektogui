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
        let value = node.text.clone().unwrap_or_default();
        let caret = node.caret.min(value.chars().count());
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
    let content = node.text.as_deref().unwrap_or("");
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
}
