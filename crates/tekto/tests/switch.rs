use tekto::{
    accesskit_dump, layout_tree, paint_tree, switch, window, Color, CpuRenderer, NodePatch,
    Renderer, Tree,
};
use tekto_tree::{route_named_key, NamedActionKey};

#[test]
fn switch_has_matching_pointer_keyboard_and_accessibility_state() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Settings", 240.0, 100.0).unwrap();
    let toggle = switch(&mut tree, root, "Alerts", false).unwrap();
    layout_tree(&mut tree, 240.0, 100.0);
    let bounds = tree.get(toggle).unwrap().bounds;
    assert_eq!(tree.click_at(bounds.x + 8.0, bounds.y + 8.0), Some(toggle));
    assert!(tree.get(toggle).unwrap().checked);
    let dump = accesskit_dump(&tree).unwrap();
    let node = dump
        .nodes
        .iter()
        .find(|node| node.id == toggle.raw())
        .unwrap();
    assert_eq!(node.role, "Switch");
    assert_eq!(node.label.as_deref(), Some("Alerts"));
    assert_eq!(node.toggled, Some(true));
    let mut on = CpuRenderer::new(240, 100).unwrap();
    paint_tree(&mut on, &tree, Color::PAGE);
    let (_, action) = route_named_key(&mut tree, NamedActionKey::Space, false);
    tree.click(action.unwrap()).unwrap();
    assert!(!tree.get(toggle).unwrap().checked);
    let mut off = CpuRenderer::new(240, 100).unwrap();
    paint_tree(&mut off, &tree, Color::PAGE);
    assert_ne!(on.rgba8().unwrap(), off.rgba8().unwrap());
    tree.patch(
        toggle,
        NodePatch {
            disabled: Some(true),
            ..NodePatch::default()
        },
    )
    .unwrap();
    assert!(tree.click(toggle).is_err());
    assert!(!tree.get(toggle).unwrap().checked);
    assert!(
        accesskit_dump(&tree)
            .unwrap()
            .nodes
            .iter()
            .find(|node| node.id == toggle.raw())
            .unwrap()
            .disabled
    );
}
