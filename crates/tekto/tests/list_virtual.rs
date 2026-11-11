//! Long lists lay out a window of rows. Scroll extent stays the full stack.

use tekto::{
    layout_tree, list, list_item, paint_tree, scroll, text, window, Color, CpuRenderer, ListRow,
    NodeKind, Renderer, LIST_OVERSCAN_PX,
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

#[test]
fn agent_scroll_changes_virtual_rows_after_layout() {
    use std::sync::{Arc, Mutex};
    use tekto_mcp::{AppHandle, SharedTreeHandle};
    let (mut tree, list_id, scroll_id, ids) = build();
    layout_tree(&mut tree, 240.0, 320.0);
    let handle = SharedTreeHandle {
        tree: Arc::new(Mutex::new(tree)),
        render: Arc::new(|_| Ok(Vec::new())),
        logs: Arc::new(Mutex::new(Vec::new())),
        on_change: Mutex::new(None),
        pending_js: Mutex::new(None),
        apply_js: Mutex::new(None),
        last_writeback: Mutex::new(None),
        action_gate: Arc::new(Mutex::new(())),
        click_js: Mutex::new(None),
        history: Mutex::new(None),
    };
    assert!(list_items(&handle.tree(), list_id).contains(&ids[0]));
    assert_eq!(handle.scroll(scroll_id, 4800.0).unwrap(), 4800.0);
    let mut tree = handle.tree.lock().unwrap();
    layout_tree(&mut tree, 240.0, 320.0);
    let shown = list_items(&tree, list_id);
    assert!(shown.contains(&ids[100]));
    assert!(!shown.contains(&ids[0]));
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

#[test]
fn shared_scroll_shows_the_list_top_below_a_header() {
    let mut tree = tekto::Tree::new();
    let root = window(&mut tree, "Shared", 240.0, 400.0).unwrap();
    tree.get_mut(root).unwrap().layout.padding = 0.0;
    tree.get_mut(root).unwrap().layout.gap = 0.0;
    let scroll_id = scroll(&mut tree, root, 240.0, VIEW_H).unwrap();
    tree.get_mut(scroll_id).unwrap().layout.padding = 24.0;
    let header = tree.add(NodeKind::Rect, Some(scroll_id)).unwrap();
    tree.get_mut(header).unwrap().layout = tekto::LayoutStyle::sized(240.0, 456.0);
    let rows: Vec<ListRow> = (0..40)
        .map(|index| ListRow {
            label: leak_label(index),
            height: ROW_H,
        })
        .collect();
    let built = list(&mut tree, scroll_id, 240.0, f32::NAN, &rows).unwrap();
    assert_eq!(built.scroll, Some(scroll_id));
    assert_eq!(tree.get(built.list).unwrap().parent, Some(scroll_id));
    let ids = tree.get(built.list).unwrap().children.clone();
    // Padding 24 plus a 456px header puts the first row at content y 480.
    layout_tree(&mut tree, 240.0, 400.0);
    let applied = tree.set_scroll_y(scroll_id, 480.0).unwrap();
    assert!((applied - 480.0).abs() < 1.0, "scroll landed at {applied}");
    layout_tree(&mut tree, 240.0, 400.0);
    layout_tree(&mut tree, 240.0, 400.0);
    let offset = tree.get(scroll_id).unwrap().scroll_y;
    assert!((offset - 480.0).abs() < 0.5, "offset drifted to {offset}");

    let shown = list_items(&tree, built.list);
    assert!(shown.contains(&ids[0]), "first row missing: {shown:?}");
    assert!(
        !shown.contains(&ids[10]),
        "row 10 is 480px into the list and should be detached"
    );
    let row = tree.get(ids[0]).unwrap().bounds;
    let view = tree.get(scroll_id).unwrap().bounds;
    assert!(
        row.y < view.y + view.height && row.y + row.height > view.y,
        "first row {:?} outside {:?}",
        row,
        view
    );
    let mut renderer = CpuRenderer::new(240, 400).unwrap();
    paint_tree(&mut renderer, &tree, Color::PAGE);
    assert!(renderer.to_png().unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn shared_scroll_keeps_rows_under_a_wrapping_header() {
    let mut tree = tekto::Tree::new();
    let root = window(&mut tree, "Wrap", 240.0, 400.0).unwrap();
    tree.get_mut(root).unwrap().layout.padding = 0.0;
    tree.get_mut(root).unwrap().layout.gap = 0.0;
    let scroll_id = scroll(&mut tree, root, 240.0, VIEW_H).unwrap();
    let header_text = "A long header with many words ".repeat(50);
    let header = text(&mut tree, scroll_id, &header_text, 16.0, Color::INK).unwrap();
    assert!(
        !tree.get(header).unwrap().layout.width.is_finite(),
        "the header width stays automatic"
    );
    let rows: Vec<ListRow> = (0..40)
        .map(|index| ListRow {
            label: leak_label(index),
            height: ROW_H,
        })
        .collect();
    let built = list(&mut tree, scroll_id, 240.0, f32::NAN, &rows).unwrap();
    let ids = tree.get(built.list).unwrap().children.clone();

    layout_tree(&mut tree, 240.0, 400.0);
    let header_h = tree.get(header).unwrap().bounds.height;
    assert!(
        (header_h - 1120.0).abs() < 1.0,
        "wrapping header height {header_h}, expected 1120"
    );
    let applied = tree.set_scroll_y(scroll_id, header_h).unwrap();
    assert!(
        (applied - header_h).abs() < 1.0,
        "scroll landed at {applied}"
    );
    layout_tree(&mut tree, 240.0, 400.0);
    layout_tree(&mut tree, 240.0, 400.0);

    let shown = list_items(&tree, built.list);
    assert!(
        shown.contains(&ids[0]),
        "first row detached, window {shown:?}"
    );
    assert!(
        !shown.contains(&ids[20]),
        "row 20 is inside the blank region the bad origin used to show"
    );
    let children = &tree.get(built.list).unwrap().children;
    assert_eq!(
        children.first().copied(),
        Some(ids[0]),
        "the viewport should start on a row, not a spacer: {children:?}"
    );
    let row = tree.get(ids[0]).unwrap().bounds;
    let view = tree.get(scroll_id).unwrap().bounds;
    assert!(
        row.y < view.y + view.height && row.y + row.height > view.y,
        "first row {:?} outside {:?}",
        row,
        view
    );
}

#[test]
fn removing_a_virtualized_list_drops_offscreen_rows_and_spacers() {
    let (mut tree, list_id, scroll_id, ids) = build();
    layout_tree(&mut tree, 240.0, 320.0);
    let off = ids[ROWS - 1];
    assert_ne!(tree.get(off).unwrap().parent, Some(list_id));
    let pads: Vec<tekto::NodeId> = tree
        .ids()
        .filter(|id| {
            tree.get(*id)
                .and_then(|node| node.name.as_deref())
                .is_some_and(|name| name.starts_with("list-pad"))
        })
        .collect();
    assert!(!pads.is_empty(), "a tall list keeps a spacer");
    tree.remove(list_id).unwrap();
    assert!(tree.get(list_id).is_none());
    assert!(tree.get(scroll_id).is_some());
    for id in ids {
        assert!(tree.get(id).is_none(), "row {id} survived remove");
    }
    for id in pads {
        assert!(tree.get(id).is_none(), "spacer {id} survived remove");
    }
}

#[test]
fn reparenting_an_offscreen_row_does_not_reattach_it() {
    let (mut tree, list_id, scroll_id, ids) = build();
    layout_tree(&mut tree, 240.0, 320.0);
    let off = ids[ROWS - 1];
    assert!(tree.get(off).unwrap().parent.is_none());
    let root = tree.get(scroll_id).unwrap().parent.unwrap();
    tree.append(root, off).unwrap();
    assert!(!tree.get(list_id).unwrap().list_slots.contains(&off));
    let jump = row_start(ROWS - 1);
    tree.set_scroll_y(scroll_id, jump).unwrap();
    layout_tree(&mut tree, 240.0, 320.0);
    assert_eq!(tree.get(off).unwrap().parent, Some(root));
    assert!(!tree.get(list_id).unwrap().children.contains(&off));
    assert!(!tree.get(list_id).unwrap().list_slots.contains(&off));
    assert_eq!(
        tree.get(root)
            .unwrap()
            .children
            .iter()
            .filter(|id| **id == off)
            .count(),
        1
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
