use tekto::{
    accesskit_dump, button, dialog, layout_tree, paint_tree, window, Color, CpuRenderer,
    OverlaySide, Renderer, Tree,
};
use tekto_tree::{route_named_key, NamedActionKey};

#[test]
fn dialog_is_named_modal_centered_and_restores_focus() {
    let mut tree = Tree::new();
    assert!(dialog(&mut tree, "Missing window").is_err());
    let root = window(&mut tree, "Main", 400.0, 300.0).unwrap();
    let background = button(&mut tree, root, "Open").unwrap();
    tree.set_focus(background).unwrap();
    assert!(dialog(&mut tree, " ").is_err());
    layout_tree(&mut tree, 400.0, 300.0);
    let mut plain = CpuRenderer::new(400, 300).unwrap();
    paint_tree(&mut plain, &tree, Color::PAGE);
    let popup = dialog(&mut tree, "Confirm").unwrap();
    let cancel = button(&mut tree, popup, "Cancel").unwrap();
    let accept = button(&mut tree, popup, "Accept").unwrap();
    tree.open_overlay(popup, root, OverlaySide::Center).unwrap();
    layout_tree(&mut tree, 400.0, 300.0);
    assert_eq!(tree.focus, Some(cancel));
    let bounds = tree.get(popup).unwrap().bounds;
    assert!((bounds.x * 2.0 + bounds.width - 400.0).abs() < 1.0);
    assert!((bounds.y * 2.0 + bounds.height - 300.0).abs() < 1.0);
    assert!(tree.click(background).is_err());
    assert!(tree.traverse_focus(false));
    assert_eq!(tree.focus, Some(accept));
    assert!(tree.traverse_focus(false));
    assert_eq!(tree.focus, Some(cancel));
    let dump = accesskit_dump(&tree).unwrap();
    let node = dump
        .nodes
        .iter()
        .find(|node| node.id == popup.raw())
        .unwrap();
    assert_eq!(node.role, "Dialog");
    assert_eq!(node.label.as_deref(), Some("Confirm"));
    assert!(node.modal);
    let mut modal = CpuRenderer::new(400, 300).unwrap();
    paint_tree(&mut modal, &tree, Color::PAGE);
    assert_ne!(&plain.rgba8().unwrap()[0..4], &modal.rgba8().unwrap()[0..4]);
    assert!(route_named_key(&mut tree, NamedActionKey::Escape, false).0);
    assert_eq!(tree.focus, Some(background));
    assert!(tree.get(popup).is_none());
    let second = dialog(&mut tree, "Again").unwrap();
    tree.open_overlay(second, root, OverlaySide::Center)
        .unwrap();
    layout_tree(&mut tree, 400.0, 300.0);
    assert_eq!(tree.click_at(0.0, 0.0), None);
    assert!(tree.overlay().is_none());
    assert_eq!(tree.focus, Some(background));
}
