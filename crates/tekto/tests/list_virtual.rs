//! Long lists lay out a window of rows. Scroll extent stays the full stack.

use tekto::{
    layout_tree, list, list_item, paint_tree, window, Color, CpuRenderer, ListRow, NodeKind,
    Renderer, LIST_OVERSCAN_PX,
};

const ROW_H: f32 = 40.0;
const VIEW_H: f32 = 120.0;
const ROWS: usize = 200;

fn build() -> (
    tekto::Tree,
    tekto::NodeId,
    tekto::NodeId,
    Vec<tekto::NodeId>,
) {
    let mut tree = tekto::Tree::new();
    let root = window(&mut tree, "Long", 240.0, 320.0).unwrap();
    tree.get_mut(root).unwrap().layout.padding = 0.0;
    tree.get_mut(root).unwrap().layout.gap = 0.0;
    let rows: Vec<ListRow> = (0..ROWS)
        .map(|index| ListRow {
            label: leak_label(index),
            height: ROW_H,
        })
        .collect();
    let built = list(&mut tree, root, 240.0, VIEW_H, &rows).unwrap();
    let scroll_id = built.scroll.expect("rows overflow the viewport");
    let ids = tree.get(built.list).unwrap().children.clone();
    assert_eq!(ids.len(), ROWS);
    (tree, built.list, scroll_id, ids)
}

fn leak_label(index: usize) -> &'static str {
    Box::leak(format!("Row {index}").into_boxed_str())
}

fn row_start(index: usize) -> f32 {
    index as f32 * (ROW_H + tekto::LIST_GAP)
}

#[test]
fn long_list_lays_out_a_window_and_keeps_a_jump() {
    let (mut tree, list_id, scroll_id, ids) = build();
    layout_tree(&mut tree, 240.0, 320.0);

    let shown = list_items(&tree, list_id);
    assert!(
        shown.len() < 20,
        "expected a small window, laid out {}",
        shown.len()
    );
    assert!(shown.len() > 2, "window {}", shown.len());
    assert!(shown.contains(&ids[0]));
    assert!(!shown.contains(&ids[ROWS - 1]));
    assert!(
        tree.get(ids[ROWS - 1]).is_some(),
        "detached rows keep their ids"
    );
    assert_ne!(tree.get(ids[ROWS - 1]).unwrap().parent, Some(list_id));

    let extent = tree.get(scroll_id).unwrap().scroll_extent;
    let full = row_start(ROWS - 1) + ROW_H;
    assert!(
        (extent - full).abs() < 1.0,
        "extent {extent} should match the full stack {full}"
    );

    let jump = 4800.0_f32;
    let applied = tree.set_scroll_y(scroll_id, jump).unwrap();
    assert!(
        (applied - jump).abs() < 1.0,
        "jump clamped to {applied}, extent {extent}"
    );
    layout_tree(&mut tree, 240.0, 320.0);
    let after = tree.get(scroll_id).unwrap().scroll_y;
    assert!(
        (after - applied).abs() < 0.5,
        "scroll moved from {applied} to {after}"
    );
    layout_tree(&mut tree, 240.0, 320.0);
    let again = tree.get(scroll_id).unwrap().scroll_y;
    assert!(
        (again - after).abs() < 0.5,
        "second layout drifted to {again}"
    );

    let shown = list_items(&tree, list_id);
    assert!(shown.len() < 20, "window after jump {}", shown.len());
    let target = 100usize;
    assert!(
        shown.contains(&ids[target]),
        "row {target} should be in the window"
    );
    assert!(!shown.contains(&ids[0]));
    assert!(!shown.contains(&ids[ROWS - 1]));

    let bounds = tree.get(ids[target]).unwrap().bounds;
    let scroll = tree.get(scroll_id).unwrap();
    let view_top = scroll.bounds.y;
    let view_bottom = view_top + scroll.bounds.height;
    assert!(
        bounds.y < view_bottom && bounds.y + bounds.height > view_top,
        "row {target} bounds {:?} outside viewport {view_top}..{view_bottom}",
        bounds
    );
    let content_y = bounds.y - view_top + again;
    assert!(
        (content_y - row_start(target)).abs() < 1.5,
        "row content y {content_y}, expected {}",
        row_start(target)
    );

    let pretty = tree.to_pretty_text();
    assert!(pretty.contains("Row 100"), "{pretty}");
    assert!(!pretty.contains("Row 0\""), "{pretty}");
    assert!(!pretty.contains(&format!("Row {}", ROWS - 1)), "{pretty}");

    let mut renderer = CpuRenderer::new(240, 320).unwrap();
    paint_tree(&mut renderer, &tree, Color::PAGE);
    let png = renderer.to_png().unwrap();
    assert!(png.starts_with(b"\x89PNG"));

    let reach = VIEW_H + LIST_OVERSCAN_PX;
    assert!(
        shown.iter().all(|id| {
            let index = ids.iter().position(|row| row == id).unwrap();
            let start = row_start(index);
            start < jump + reach && start + ROW_H > jump - LIST_OVERSCAN_PX
        }),
        "a laid-out row sits outside the overscan window"
    );
}

fn list_items(tree: &tekto::Tree, list_id: tekto::NodeId) -> Vec<tekto::NodeId> {
    tree.get(list_id)
        .unwrap()
        .children
        .iter()
        .copied()
        .filter(|id| tree.get(*id).unwrap().kind == NodeKind::ListItem)
        .collect()
}

#[test]
fn appended_row_joins_the_window_on_the_next_layout() {
    let (mut tree, list_id, scroll_id, _) = build();
    layout_tree(&mut tree, 240.0, 320.0);
    let extra = list_item(&mut tree, list_id, "Tail", ROW_H).unwrap();
    layout_tree(&mut tree, 240.0, 320.0);
    assert_eq!(tree.get(extra).unwrap().kind, NodeKind::ListItem);
    let extent = tree.get(scroll_id).unwrap().scroll_extent;
    assert!(extent > row_start(ROWS), "extent {extent}");
    let max = extent - VIEW_H;
    let applied = tree.set_scroll_y(scroll_id, max).unwrap();
    layout_tree(&mut tree, 240.0, 320.0);
    assert!((tree.get(scroll_id).unwrap().scroll_y - applied).abs() < 0.5);
    let shown = list_items(&tree, list_id);
    assert!(shown.contains(&extra), "tail row missing from {shown:?}");
}
