use std::sync::{Arc, Mutex};
use tekto::{
    accesskit_dump, button, layout_tree, menu, menu_item, paint_tree, window, Color, CpuRenderer,
    NodePatch, OverlaySide, Renderer, Tree, TreeEvent,
};
use tekto_tree::{route_named_key, NamedActionKey};

#[test]
fn menu_navigates_selects_and_restores_focus() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Main", 320.0, 240.0).unwrap();
    let anchor = button(&mut tree, root, "Actions").unwrap();
    tree.set_focus(anchor).unwrap();
    let popup = menu(&mut tree, "Actions").unwrap();
    let first = menu_item(&mut tree, popup, "Open").unwrap();
    let disabled = menu_item(&mut tree, popup, "Unavailable").unwrap();
    let last = menu_item(&mut tree, popup, "Close").unwrap();
    tree.patch(
        disabled,
        NodePatch {
            disabled: Some(true),
            ..NodePatch::default()
        },
    )
    .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let observed = events.clone();
    tree.set_event_handler(move |_, event| observed.lock().unwrap().push(event));
    layout_tree(&mut tree, 320.0, 240.0);
    tree.open_overlay(popup, anchor, OverlaySide::Below)
        .unwrap();
    layout_tree(&mut tree, 320.0, 240.0);
    assert_eq!(tree.focus, Some(first));
    let dump = accesskit_dump(&tree).unwrap();
    assert_eq!(
        dump.nodes
            .iter()
            .find(|n| n.id == popup.raw())
            .unwrap()
            .role,
        "Menu"
    );
    assert_eq!(
        dump.nodes
            .iter()
            .find(|n| n.id == anchor.raw())
            .unwrap()
            .expanded,
        Some(true)
    );
    let item = dump.nodes.iter().find(|n| n.id == last.raw()).unwrap();
    assert_eq!(item.role, "MenuItem");
    assert_eq!(item.label.as_deref(), Some("Close"));
    assert!(tree.click(disabled).is_err());
    assert!(route_named_key(&mut tree, NamedActionKey::ArrowDown, false).0);
    assert_eq!(tree.focus, Some(last));
    let mut renderer = CpuRenderer::new(320, 240).unwrap();
    paint_tree(&mut renderer, &tree, Color::PAGE);
    assert!(renderer.rgba8().unwrap().iter().any(|byte| *byte != 0));
    let (_, activate) = route_named_key(&mut tree, NamedActionKey::Enter, false);
    tree.click(activate.unwrap()).unwrap();
    assert_eq!(*events.lock().unwrap(), vec![TreeEvent::Click(last)]);
    assert!(tree.overlay().is_none());
    assert_eq!(tree.focus, Some(anchor));
    assert!(tree.get(last).is_none());
}

#[test]
fn menu_keys_skip_disabled_items_and_escape_restores_focus() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Main", 320.0, 240.0).unwrap();
    let anchor = button(&mut tree, root, "Actions").unwrap();
    tree.set_focus(anchor).unwrap();
    let popup = menu(&mut tree, "Actions").unwrap();
    let first = menu_item(&mut tree, popup, "Open").unwrap();
    let skipped = menu_item(&mut tree, popup, "Unavailable").unwrap();
    let last = menu_item(&mut tree, popup, "Close").unwrap();
    tree.patch(
        skipped,
        NodePatch {
            disabled: Some(true),
            ..NodePatch::default()
        },
    )
    .unwrap();
    layout_tree(&mut tree, 320.0, 240.0);
    tree.open_overlay(popup, anchor, OverlaySide::Below)
        .unwrap();
    assert_eq!(tree.focus, Some(first));
    route_named_key(&mut tree, NamedActionKey::ArrowDown, false);
    assert_eq!(tree.focus, Some(last), "down skips the disabled item");
    route_named_key(&mut tree, NamedActionKey::ArrowUp, false);
    assert_eq!(tree.focus, Some(first), "up skips the disabled item");
    // Space and Enter on a focused item activate it only if it is enabled.
    tree.patch(
        first,
        NodePatch {
            disabled: Some(true),
            ..NodePatch::default()
        },
    )
    .unwrap();
    assert_ne!(
        tree.focus,
        Some(first),
        "focus leaves a newly disabled item"
    );
    route_named_key(&mut tree, NamedActionKey::Escape, false);
    assert!(tree.overlay().is_none());
    assert_eq!(tree.focus, Some(anchor));
}
