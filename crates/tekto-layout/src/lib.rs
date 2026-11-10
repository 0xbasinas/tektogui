//! Layout pass: map the scene tree to Taffy, then write bounds back.

use taffy::prelude::{
    AvailableSpace, Dimension, Display, FlexDirection, LengthPercentage, Rect, Size, Style,
    TaffyTree,
};
use tekto_tree::{NodeId, NodeKind, Tree};

/// Compute bounds for every node, starting at `origin` in a viewport of
/// `viewport_width` x `viewport_height` logical pixels.
pub fn layout_tree(tree: &mut Tree, viewport_width: f32, viewport_height: f32) {
    let Some(root) = tree.root() else {
        return;
    };
    let mut taffy: TaffyTree<()> = TaffyTree::new();
    let mut map: Vec<(NodeId, taffy::NodeId)> = Vec::new();
    // We store taffy NodeId in a side table keyed by tekto NodeId via a hashmap.
    let taffy_root = build(&mut taffy, tree, root, &mut map);
    let _ = taffy.compute_layout(
        taffy_root,
        Size {
            width: AvailableSpace::Definite(viewport_width),
            height: AvailableSpace::Definite(viewport_height),
        },
    );
    write_bounds(&taffy, tree, root, 0.0, 0.0, &map);
    let _ = map;
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
        } else if matches!(node.kind, NodeKind::Text) {
            // Approximate line box until text measurement is wired into Taffy.
            Dimension::length((node.font_size * 1.4).max(16.0))
        } else {
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
    taffy: &mut TaffyTree<()>,
    tree: &Tree,
    id: NodeId,
    map: &mut Vec<(NodeId, taffy::NodeId)>,
) -> taffy::NodeId {
    let is_root = tree.root() == Some(id);
    let style = style_for(tree, id, is_root);
    let children: Vec<taffy::NodeId> = tree
        .get(id)
        .map(|n| {
            n.children
                .iter()
                .map(|c| build(taffy, tree, *c, map))
                .collect()
        })
        .unwrap_or_default();
    let tid = taffy.new_with_children(style, &children).expect("taffy");
    map.push((id, tid));
    tid
}

fn write_bounds(
    taffy: &TaffyTree<()>,
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
}
