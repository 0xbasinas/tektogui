use tekto::{layout_tree, window, KeyedList, NodeId, NodeKind, Tree};
use tekto_widgets::WidgetError;

fn keys(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("row-{index:04}")).collect()
}

fn build_row(tree: &mut Tree, rows: NodeId, key: &str) -> Result<NodeId, WidgetError> {
    let id = tree.add(NodeKind::ListItem, Some(rows))?;
    if let Some(node) = tree.get_mut(id) {
        node.name = Some(key.into());
        node.layout.height = 20.0;
    }
    Ok(id)
}

fn visible_count(list: &KeyedList) -> usize {
    list.visible_rows()
}

#[test]
fn keyed_list_keeps_only_rows_near_the_viewport() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Lazy", 400.0, 260.0).unwrap();
    let mut list = KeyedList::new(&mut tree, root, 200.0, 20.0).unwrap();
    let keys = keys(1000);
    layout_tree(&mut tree, 400.0, 260.0);
    let mut build = |tree: &mut Tree, rows: NodeId, key: &str| build_row(tree, rows, key);

    let live = list.sync(&mut tree, &keys, 40.0, &mut build).unwrap();
    assert!(live <= 16, "live {live}");
    assert!(tree.len() < 40, "nodes {}", tree.len());
    let first = tree.get(list.rows).unwrap().children[1];
    assert_eq!(tree.get(first).unwrap().name.as_deref(), Some("row-0000"));
    // Layout turns the spacers into a scroll extent, so scrolling can clamp.
    layout_tree(&mut tree, 400.0, 260.0);

    // A tiny scroll keeps most rows and their ids.
    let kept = list.row("row-0005").unwrap();
    tree.set_scroll_y(list.scroll, 40.0).unwrap();
    list.sync(&mut tree, &keys, 40.0, &mut build).unwrap();
    assert_eq!(list.row("row-0005"), Some(kept));
    layout_tree(&mut tree, 400.0, 260.0);

    // A far scroll drops the old rows and materializes new ones.
    assert!(list.row("row-0005").is_some());
    tree.set_scroll_y(list.scroll, 4000.0).unwrap();
    let live = list.sync(&mut tree, &keys, 40.0, &mut build).unwrap();
    assert!(live <= 16, "live {live}");
    assert!(tree.len() < 40, "nodes {}", tree.len());
    assert!(list.row("row-0005").is_none());
    assert!(list.row("row-0200").is_some());
    assert!(tree.get(kept).is_none(), "old row node is removed");
}

#[test]
fn keyed_list_can_scroll_to_and_materialize_an_offscreen_key() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Lazy", 400.0, 260.0).unwrap();
    let mut list = KeyedList::new(&mut tree, root, 200.0, 20.0).unwrap();
    let keys = keys(1000);
    layout_tree(&mut tree, 400.0, 260.0);
    let mut build = |tree: &mut Tree, rows: NodeId, key: &str| build_row(tree, rows, key);
    list.sync(&mut tree, &keys, 40.0, &mut build).unwrap();
    assert!(list.row("row-0900").is_none());
    layout_tree(&mut tree, 400.0, 260.0);

    assert!(list.scroll_to_key(&mut tree, &keys, "row-0900"));
    list.sync(&mut tree, &keys, 40.0, &mut build).unwrap();
    let row = list.row("row-0900").expect("materialized row");
    assert_eq!(tree.get(row).unwrap().name.as_deref(), Some("row-0900"));
    assert!(list.row("row-0000").is_none());
    assert!(!list.scroll_to_key(&mut tree, &keys, "missing"));
}

#[test]
fn keyed_list_rejects_duplicate_keys_and_bad_geometry() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Lazy", 400.0, 260.0).unwrap();
    assert!(KeyedList::new(&mut tree, root, 0.0, 20.0).is_err());
    assert!(KeyedList::new(&mut tree, root, 200.0, f32::NAN).is_err());

    let mut list = KeyedList::new(&mut tree, root, 200.0, 20.0).unwrap();
    let dupes = vec!["a".to_string(), "a".to_string()];
    layout_tree(&mut tree, 400.0, 260.0);
    let mut build = |tree: &mut Tree, rows: NodeId, key: &str| build_row(tree, rows, key);
    let error = list.sync(&mut tree, &dupes, 0.0, &mut build).unwrap_err();
    assert!(error.to_string().contains("duplicate keyed row"));

    // A smaller key set removes rows that no longer exist at all.
    let small = keys(10);
    list.sync(&mut tree, &small, 0.0, &mut build).unwrap();
    let live = visible_count(&list);
    assert_eq!(live, 10);
}
