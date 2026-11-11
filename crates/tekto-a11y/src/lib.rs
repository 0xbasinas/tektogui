//! Map the Tekto scene tree to an AccessKit tree.
//!
//! This is the same tree agents get from MCP `ui.tree` and `GET /accesskit`.
//! Screen readers and coding agents share one source of truth.

use accesskit::{Node, NodeId, Rect, Role, Tree, TreeUpdate};
use serde::Serialize;
use tekto_tree::{NodeKind, Tree as SceneTree};

/// Build a full AccessKit tree update from the current scene.
///
/// Focus is `scene.focus` if that node still exists, otherwise the root.
pub fn tree_update(scene: &SceneTree) -> Option<TreeUpdate> {
    tree_update_with_focus(scene, scene.focus)
}

/// Like [`tree_update`], but the caller can override focus.
pub fn tree_update_with_focus(
    scene: &SceneTree,
    focus: Option<tekto_tree::NodeId>,
) -> Option<TreeUpdate> {
    let root = scene.root()?;
    let focus_id = focus.filter(|id| scene.get(*id).is_some()).unwrap_or(root);
    let mut nodes = Vec::new();
    collect(scene, root, &mut nodes);
    Some(TreeUpdate {
        nodes,
        tree: Some(Tree::new(ak_id(root))),
        focus: ak_id(focus_id),
    })
}

/// A JSON-friendly dump of the AccessKit tree (same ids as the scene tree).
#[derive(Clone, Debug, Serialize)]
pub struct AccessKitDump {
    pub focus: u64,
    pub nodes: Vec<AccessKitNodeDump>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AccessKitNodeDump {
    pub id: u64,
    pub role: String,
    pub disabled: bool,
    pub label: Option<String>,
    pub value: Option<String>,
    pub bounds: [f64; 4],
    pub children: Vec<u64>,
}

/// Dump the AccessKit tree for the dev server and for tests.
pub fn dump(scene: &SceneTree) -> Option<AccessKitDump> {
    let update = tree_update(scene)?;
    let nodes = update
        .nodes
        .iter()
        .map(|(id, node)| AccessKitNodeDump {
            id: id.0,
            role: format!("{:?}", node.role()),
            disabled: node.is_disabled(),
            label: node.label().map(|s| s.to_string()),
            value: node.value().map(|s| s.to_string()),
            bounds: node.bounds().map_or([0.0; 4], |r| [r.x0, r.y0, r.x1, r.y1]),
            children: node.children().iter().map(|c| c.0).collect(),
        })
        .collect();
    Some(AccessKitDump {
        focus: update.focus.0,
        nodes,
    })
}

fn collect(scene: &SceneTree, id: tekto_tree::NodeId, out: &mut Vec<(NodeId, Node)>) {
    let Some(src) = scene.get(id) else {
        return;
    };
    let mut node = Node::new(role_for(src.kind));
    if src.disabled {
        node.set_disabled();
    }
    if src.kind == NodeKind::TextField {
        if let Some(name) = src.name.as_ref().filter(|s| !s.is_empty()) {
            node.set_label(name.clone());
        }
        // Committed text plus the current preedit, so a screen reader sees
        // the composition before it is committed.
        node.set_value(src.composed_text());
    } else {
        let label = src.display_name();
        if !label.is_empty() {
            node.set_label(label);
        }
        if let Some(text) = &src.text {
            node.set_value(text.clone());
        }
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
        NodeKind::TextField => Role::TextInput,
        NodeKind::Scroll => Role::ScrollView,
        NodeKind::List => Role::List,
        NodeKind::ListItem => Role::ListItem,
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
        assert_eq!(update.focus, ak_id(w));
        assert!(update
            .nodes
            .iter()
            .any(|(_, n)| { n.label().map(|l| l.contains("Notes")).unwrap_or(false) }));
    }

    #[test]
    fn focus_follows_set_focus_and_dump_uses_same_ids() {
        let mut scene = SceneTree::new();
        let w = scene.add(K::Window, None).unwrap();
        let t = scene.add(K::Text, Some(w)).unwrap();
        scene.get_mut(t).unwrap().text = Some("Hello".into());
        scene.get_mut(t).unwrap().bounds = tekto_tree::Rect::new(10.0, 20.0, 80.0, 24.0);
        scene.set_focus(t).unwrap();
        let update = tree_update(&scene).expect("update");
        assert_eq!(update.focus, ak_id(t));
        let dump = dump(&scene).expect("dump");
        assert_eq!(dump.focus, t.raw());
        let text = dump.nodes.iter().find(|n| n.id == t.raw()).expect("text");
        assert_eq!(text.role, "Label");
        assert_eq!(text.value.as_deref(), Some("Hello"));
        assert_eq!(text.bounds, [10.0, 20.0, 90.0, 44.0]);
        assert!(dump
            .nodes
            .iter()
            .any(|n| n.id == w.raw() && n.role == "Window"));
    }

    #[test]
    fn text_field_accesskit_value_includes_preedit_then_commit() {
        let mut scene = SceneTree::new();
        let w = scene.add(K::Window, None).unwrap();
        let field = scene.add(K::TextField, Some(w)).unwrap();
        scene.get_mut(field).unwrap().name = Some("Title".into());
        scene.get_mut(field).unwrap().text = Some("Hi".into());
        scene.get_mut(field).unwrap().caret = 2;
        scene.set_focus(field).unwrap();
        scene
            .apply_ime(tekto_tree::ImeEvent::Preedit {
                text: "你".into(),
                cursor: Some((3, 3)),
            })
            .unwrap();
        let dump = dump(&scene).expect("dump");
        let node = dump.nodes.iter().find(|n| n.id == field.raw()).unwrap();
        assert_eq!(node.role, "TextInput");
        assert_eq!(node.label.as_deref(), Some("Title"));
        assert_eq!(node.value.as_deref(), Some("Hi你"));
        scene
            .apply_ime(tekto_tree::ImeEvent::Commit("你".into()))
            .unwrap();
        let dump = super::dump(&scene).expect("dump");
        let node = dump.nodes.iter().find(|n| n.id == field.raw()).unwrap();
        assert_eq!(node.value.as_deref(), Some("Hi你"));
        assert_eq!(scene.get(field).unwrap().text.as_deref(), Some("Hi你"));
    }

    #[test]
    fn android_overlay_value_is_the_accesskit_value() {
        let mut scene = SceneTree::new();
        let w = scene.add(K::Window, None).unwrap();
        let field = scene.add(K::TextField, Some(w)).unwrap();
        scene.get_mut(field).unwrap().name = Some("Title".into());
        scene.get_mut(field).unwrap().text = Some("".into());
        scene.set_focus(field).unwrap();
        assert!(scene.set_focused_field_value("Milk", 4).unwrap());
        let dump = dump(&scene).expect("dump");
        let node = dump.nodes.iter().find(|n| n.id == field.raw()).unwrap();
        assert_eq!(node.role, "TextInput");
        assert_eq!(node.label.as_deref(), Some("Title"));
        assert_eq!(node.value.as_deref(), Some("Milk"));
    }
}
