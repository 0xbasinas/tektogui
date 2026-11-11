use tekto::{accesskit_dump, layout_tree, select, select_option, window, NodePatch, Tree};
use tekto_tree::{route_named_key, NamedActionKey};

#[test]
fn select_preserves_options_and_selection_across_open_and_close() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Settings", 320.0, 240.0).unwrap();
    let select = select(&mut tree, root, "Size").unwrap();
    let first = select_option(&mut tree, select, "Small", true).unwrap();
    let disabled = select_option(&mut tree, select, "Medium", false).unwrap();
    let last = select_option(&mut tree, select, "Large", false).unwrap();
    tree.patch(
        disabled,
        NodePatch {
            disabled: Some(true),
            ..NodePatch::default()
        },
    )
    .unwrap();
    layout_tree(&mut tree, 320.0, 240.0);
    assert!(tree.click(first).is_err());
    assert_eq!(tree.get(select).unwrap().text.as_deref(), Some("Small"));
    tree.click(select).unwrap();
    layout_tree(&mut tree, 320.0, 240.0);
    let dump = accesskit_dump(&tree).unwrap();
    let combo = dump.nodes.iter().find(|n| n.id == select.raw()).unwrap();
    assert_eq!(combo.role, "ComboBox");
    assert_eq!(combo.expanded, Some(true));
    assert_eq!(
        dump.nodes
            .iter()
            .find(|n| n.id == first.raw())
            .unwrap()
            .role,
        "ListBoxOption"
    );
    assert_eq!(tree.focus, Some(first));
    assert!(route_named_key(&mut tree, NamedActionKey::ArrowDown, false).0);
    assert_eq!(tree.focus, Some(last));
    let (_, action) = route_named_key(&mut tree, NamedActionKey::Enter, false);
    tree.click(action.unwrap()).unwrap();
    assert!(tree.overlay().is_none());
    assert_eq!(tree.focus, Some(select));
    assert_eq!(tree.get(select).unwrap().text.as_deref(), Some("Large"));
    assert!(!tree.get(first).unwrap().checked);
    assert!(tree.get(last).unwrap().checked);
    assert!(tree.get(last).is_some());
    tree.click(select).unwrap();
    assert!(tree.get(last).is_some());
    assert_eq!(tree.focus, Some(last));
    assert!(route_named_key(&mut tree, NamedActionKey::Escape, false).0);
    assert!(tree.get(last).is_some());
    assert_eq!(tree.get(select).unwrap().text.as_deref(), Some("Large"));
}

#[test]
fn disabling_open_select_dismisses_and_preserves_options() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Settings", 320.0, 240.0).unwrap();
    let select = select(&mut tree, root, "Size").unwrap();
    let option = select_option(&mut tree, select, "Small", true).unwrap();
    tree.click(select).unwrap();
    tree.patch(
        select,
        NodePatch {
            disabled: Some(true),
            ..NodePatch::default()
        },
    )
    .unwrap();
    assert!(tree.overlay().is_none());
    assert_eq!(tree.focus, Some(root));
    assert_eq!(tree.get(option).unwrap().parent, Some(select));
    assert!(tree.click(select).is_err());
}
