use tekto::{accesskit_dump, button, layout_tree, menu, menu_item, window, Tree};
use tekto_tree::{route_named_key, NamedActionKey};

#[test]
fn context_menu_opens_at_pointer_reuses_items_and_cleans_up() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Main", 320.0, 240.0).unwrap();
    let target = button(&mut tree, root, "Actions").unwrap();
    let popup = menu(&mut tree, "Actions").unwrap();
    let item = menu_item(&mut tree, popup, "Open").unwrap();
    tree.bind_context_menu(target, popup).unwrap();
    layout_tree(&mut tree, 320.0, 240.0);
    let bounds = tree.get(target).unwrap().bounds;
    let x = bounds.x + 5.0;
    let y = bounds.y + 5.0;
    assert_eq!(tree.context_menu_at(x, y), Some(target));
    layout_tree(&mut tree, 320.0, 240.0);
    let bounds = tree.get(popup).unwrap().bounds;
    assert_eq!((bounds.x, bounds.y), (x, y));
    assert!(accesskit_dump(&tree)
        .unwrap()
        .nodes
        .iter()
        .any(|node| node.id == item.raw()));
    assert_eq!(tree.focus, Some(item));
    assert!(route_named_key(&mut tree, NamedActionKey::Escape, false).0);
    assert!(tree.get(item).is_some());
    assert!(!accesskit_dump(&tree)
        .unwrap()
        .nodes
        .iter()
        .any(|node| node.id == item.raw()));
    assert_eq!(tree.context_menu_at(x, y), Some(target));
    tree.click(item).unwrap();
    assert!(tree.overlay().is_none());
    assert!(tree.get(item).is_some());
    tree.remove(target).unwrap();
    assert!(tree.get(popup).is_none());
    assert!(tree.get(item).is_none());
}
