use tekto::{
    accesskit_dump, layout_tree, paint_tree, radio, radio_group, window, Color, CpuRenderer,
    NodePatch, Renderer, Tree,
};
use tekto_tree::{route_named_key, NamedActionKey};

#[test]
fn radio_group_selects_one_and_skips_disabled_options() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Settings", 240.0, 240.0).unwrap();
    let group = radio_group(&mut tree, root, "Size").unwrap();
    let first = radio(&mut tree, group, "Small", true).unwrap();
    let disabled = radio(&mut tree, group, "Medium", false).unwrap();
    let last = radio(&mut tree, group, "Large", false).unwrap();
    tree.patch(
        disabled,
        NodePatch {
            disabled: Some(true),
            ..NodePatch::default()
        },
    )
    .unwrap();
    layout_tree(&mut tree, 240.0, 240.0);
    let dump = accesskit_dump(&tree).unwrap();
    let group_node = dump.nodes.iter().find(|n| n.id == group.raw()).unwrap();
    assert_eq!(group_node.role, "RadioGroup");
    assert_eq!(group_node.label.as_deref(), Some("Size"));
    let first_node = dump.nodes.iter().find(|n| n.id == first.raw()).unwrap();
    assert_eq!(first_node.role, "RadioButton");
    assert_eq!(first_node.toggled, Some(true));
    assert!(tree.traverse_focus(false));
    assert_eq!(tree.focus, Some(first));
    tree.set_focus(first).unwrap();
    let mut before = CpuRenderer::new(240, 240).unwrap();
    paint_tree(&mut before, &tree, Color::PAGE);
    assert!(route_named_key(&mut tree, NamedActionKey::ArrowDown, false).0);
    assert_eq!(tree.focus, Some(last));
    assert!(!tree.get(first).unwrap().checked);
    assert!(tree.get(last).unwrap().checked);
    assert!(tree.traverse_focus(false));
    assert_eq!(tree.focus, Some(last)); // The group contributes one tab stop.
    let mut after = CpuRenderer::new(240, 240).unwrap();
    paint_tree(&mut after, &tree, Color::PAGE);
    assert_ne!(before.rgba8().unwrap(), after.rgba8().unwrap());
    assert!(tree.click(disabled).is_err());
    tree.click(first).unwrap();
    assert!(tree.get(first).unwrap().checked);
    assert!(!tree.get(last).unwrap().checked);
    tree.patch(
        last,
        NodePatch {
            checked: Some(true),
            ..NodePatch::default()
        },
    )
    .unwrap();
    assert!(!tree.get(first).unwrap().checked);
    assert!(tree.get(last).unwrap().checked);
    assert!(tree.add(tekto::NodeKind::Radio, Some(root)).is_err());
}
