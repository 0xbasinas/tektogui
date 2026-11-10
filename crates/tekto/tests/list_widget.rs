//! Variable-height list: stable ids, scroll when taller than the viewport,
//! AccessKit list / list-item roles on those same ids.

use tekto::{accesskit_dump, layout_tree, list, list_item, window, ListRow, NodeKind};

#[test]
fn variable_rows_scroll_and_match_accesskit_ids() {
    let mut tree = tekto::Tree::new();
    let root = window(&mut tree, "List", 240.0, 320.0).unwrap();
    tree.get_mut(root).unwrap().layout.padding = 0.0;
    tree.get_mut(root).unwrap().layout.gap = 0.0;
    let rows = [
        ListRow {
            label: "Short",
            height: 40.0,
        },
        ListRow {
            label: "Tall note",
            height: 90.0,
        },
        ListRow {
            label: "Last",
            height: 50.0,
        },
    ];
    let built = list(&mut tree, root, 240.0, 100.0, &rows).unwrap();
    let scroll_id = built.scroll.expect("content taller than 100");
    assert_eq!(tree.get(scroll_id).unwrap().kind, NodeKind::Scroll);

    let items = tree.get(built.list).unwrap().children.clone();
    layout_tree(&mut tree, 240.0, 320.0);

    let h0 = tree.get(items[0]).unwrap().bounds.height;
    let h1 = tree.get(items[1]).unwrap().bounds.height;
    let h2 = tree.get(items[2]).unwrap().bounds.height;
    assert!((h0 - 40.0).abs() < 1.0, "short row {h0}");
    assert!((h1 - 90.0).abs() < 1.0, "tall row {h1}");
    assert!((h2 - 50.0).abs() < 1.0, "last row {h2}");
    assert!(h1 > h0 && h1 > h2);

    let extent = tree.get(scroll_id).unwrap().scroll_extent;
    assert!(extent > 100.0, "extent {extent}");

    let dump = accesskit_dump(&tree).expect("accesskit");
    let list_node = dump
        .nodes
        .iter()
        .find(|n| n.id == built.list.raw())
        .unwrap();
    assert_eq!(list_node.role, "List");
    assert_eq!(
        list_node.children,
        items.iter().map(|id| id.raw()).collect::<Vec<_>>()
    );
    for (id, label) in items.iter().zip(["Short", "Tall note", "Last"]) {
        let row = dump.nodes.iter().find(|n| n.id == id.raw()).unwrap();
        assert_eq!(row.role, "ListItem");
        assert_eq!(row.label.as_deref(), Some(label));
    }
    let scroll_node = dump.nodes.iter().find(|n| n.id == scroll_id.raw()).unwrap();
    assert_eq!(scroll_node.role, "ScrollView");
}

/// A short or empty list has no scroll parent until `list_item` makes the
/// stack taller than the viewport.
#[test]
fn appended_rows_overflow_into_scroll() {
    let mut tree = tekto::Tree::new();
    let root = window(&mut tree, "List", 240.0, 320.0).unwrap();
    tree.get_mut(root).unwrap().layout.padding = 0.0;
    tree.get_mut(root).unwrap().layout.gap = 0.0;

    let built = list(&mut tree, root, 240.0, 100.0, &[]).unwrap();
    assert!(built.scroll.is_none());
    assert_eq!(tree.get(built.list).unwrap().parent, Some(root));

    let one = list_item(&mut tree, built.list, "One", 50.0).unwrap();
    assert_eq!(
        tree.get(built.list).unwrap().parent,
        Some(root),
        "one 50px row fits in 100px"
    );

    let two = list_item(&mut tree, built.list, "Two", 50.0).unwrap();
    let three = list_item(&mut tree, built.list, "Three", 50.0).unwrap();
    let items = [one, two, three];

    let scroll_id = tree.get(built.list).unwrap().parent.expect("scroll parent");
    assert_eq!(tree.get(scroll_id).unwrap().kind, NodeKind::Scroll);
    assert_eq!(tree.get(scroll_id).unwrap().parent, Some(root));
    assert!((tree.get(scroll_id).unwrap().layout.height - 100.0).abs() < 0.1);
    assert_eq!(tree.get(built.list).unwrap().children, items);

    layout_tree(&mut tree, 240.0, 320.0);
    let extent = tree.get(scroll_id).unwrap().scroll_extent;
    assert!(extent > 100.0, "extent {extent}");

    let y_before = tree.get(three).unwrap().bounds.y;
    tree.scroll_by(scroll_id, 40.0).unwrap();
    layout_tree(&mut tree, 240.0, 320.0);
    let y_after = tree.get(three).unwrap().bounds.y;
    assert!(
        y_before - y_after > 30.0,
        "row should move up when the viewport scrolls, before {y_before} after {y_after}"
    );
    assert_eq!(tree.get(one).unwrap().id, one);
    assert_eq!(tree.get(built.list).unwrap().kind, NodeKind::List);

    let dump = accesskit_dump(&tree).expect("accesskit");
    let list_node = dump
        .nodes
        .iter()
        .find(|n| n.id == built.list.raw())
        .unwrap();
    assert_eq!(list_node.role, "List");
    assert_eq!(
        list_node.children,
        items.iter().map(|id| id.raw()).collect::<Vec<_>>()
    );
    for (id, label) in items.iter().zip(["One", "Two", "Three"]) {
        let row = dump.nodes.iter().find(|n| n.id == id.raw()).unwrap();
        assert_eq!(row.role, "ListItem");
        assert_eq!(row.label.as_deref(), Some(label));
        assert!((tree.get(*id).unwrap().bounds.height - 50.0).abs() < 1.0);
    }
    let scroll_node = dump.nodes.iter().find(|n| n.id == scroll_id.raw()).unwrap();
    assert_eq!(scroll_node.role, "ScrollView");
}
