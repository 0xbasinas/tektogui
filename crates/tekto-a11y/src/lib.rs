//! Map the Tekto scene tree to an AccessKit tree.
//!
//! This is the same tree agents get from MCP `ui.tree`. Screen readers
//! and coding agents share one source of truth.

use accesskit::{Node, NodeId, Rect, Role, Tree, TreeUpdate};
use tekto_tree::{NodeKind, Tree as SceneTree};

/// Build a full AccessKit tree update from the current scene.
pub fn tree_update(scene: &SceneTree) -> Option<TreeUpdate> {
    let root = scene.root()?;
    let mut nodes = Vec::new();
    collect(scene, root, &mut nodes);
    Some(TreeUpdate {
        nodes,
        tree: Some(Tree::new(ak_id(root))),
        focus: ak_id(root),
    })
}

fn collect(scene: &SceneTree, id: tekto_tree::NodeId, out: &mut Vec<(NodeId, Node)>) {
    let Some(src) = scene.get(id) else {
        return;
    };
    let mut node = Node::new(role_for(src.kind));
    let label = src.display_name();
    if !label.is_empty() {
        node.set_label(label);
    }
    if let Some(text) = &src.text {
        node.set_value(text.clone());
    }
    let b = src.bounds;
    node.set_bounds(Rect::new(
        b.x as f64,
        b.y as f64,
        (b.x + b.width) as f64,
        (b.y + b.height) as f64,
    ));
    let children: Vec<NodeId> = src.children.iter().copied().map(ak_id).collect();
    if !children.is_empty() {
        node.set_children(children);
    }
    out.push((ak_id(id), node));
    for child in &src.children.clone() {
        collect(scene, *child, out);
    }
}

fn role_for(kind: NodeKind) -> Role {
    match kind {
        NodeKind::Window => Role::Window,
        NodeKind::Group => Role::GenericContainer,
        NodeKind::Rect => Role::GenericContainer,
        NodeKind::Text => Role::Label,
        NodeKind::Button => Role::Button,
    }
}

fn ak_id(id: tekto_tree::NodeId) -> NodeId {
    NodeId(id.raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tekto_tree::NodeKind as K;

    #[test]
    fn builds_a_window_and_text() {
        let mut scene = SceneTree::new();
        let w = scene.add(K::Window, None).unwrap();
        scene.get_mut(w).unwrap().name = Some("Notes".into());
        let t = scene.add(K::Text, Some(w)).unwrap();
        scene.get_mut(t).unwrap().text = Some("Hello from Tekto".into());
        let update = tree_update(&scene).expect("update");
        assert_eq!(update.nodes.len(), 2);
        assert!(update
            .nodes
            .iter()
            .any(|(_, n)| { n.label().map(|l| l.contains("Notes")).unwrap_or(false) }));
    }
}
