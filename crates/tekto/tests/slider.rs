use tekto::{
    accesskit_dump, layout_tree, paint_tree, slider, window, Color, CpuRenderer, NodePatch,
    Renderer, Tree,
};
use tekto_tree::{route_named_key, NamedActionKey};

#[test]
fn slider_value_keyboard_and_pixels_match_semantics() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Audio", 240.0, 100.0).unwrap();
    let id = slider(&mut tree, root, "Volume", 0.5).unwrap();
    layout_tree(&mut tree, 240.0, 100.0);
    tree.set_focus(id).unwrap();
    let dump = accesskit_dump(&tree).unwrap();
    let node = dump.nodes.iter().find(|n| n.id == id.raw()).unwrap();
    assert_eq!(node.role, "Slider");
    assert_eq!(node.label.as_deref(), Some("Volume"));
    assert_eq!(node.numeric_value, Some(0.5));
    let mut first = CpuRenderer::new(240, 100).unwrap();
    paint_tree(&mut first, &tree, Color::PAGE);
    assert!(route_named_key(&mut tree, NamedActionKey::End, false).0);
    assert_eq!(tree.get(id).unwrap().progress, 1.0);
    let mut last = CpuRenderer::new(240, 100).unwrap();
    paint_tree(&mut last, &tree, Color::PAGE);
    assert_ne!(first.rgba8().unwrap(), last.rgba8().unwrap());
    assert!(route_named_key(&mut tree, NamedActionKey::Home, false).0);
    assert_eq!(tree.get(id).unwrap().progress, 0.0);
    tree.patch(
        id,
        NodePatch {
            disabled: Some(true),
            ..NodePatch::default()
        },
    )
    .unwrap();
    assert!(tree.set_slider_value(id, 0.5).is_err());
}
