use tekto::{
    layout_tree, paint_tree, text_field, window, Color, CpuRenderer, FieldKey, ImeEvent, NodePatch,
    Renderer, Tree,
};

#[test]
fn selection_replaces_graphemes_and_ime_commit() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Edit", 320.0, 120.0).unwrap();
    let field = text_field(&mut tree, root, "Name", "Ae\u{301}👩‍💻Z").unwrap();
    tree.set_focus(field).unwrap();
    tree.apply_field_key(FieldKey::End).unwrap();
    tree.apply_field_key(FieldKey::SelectLeft).unwrap();
    assert_eq!(tree.selected_text(field).unwrap(), "Z");
    tree.apply_field_key(FieldKey::SelectLeft).unwrap();
    assert_eq!(tree.selected_text(field).unwrap(), "👩‍💻Z");
    tree.insert_text(field, "!").unwrap();
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Ae\u{301}!"));
    tree.apply_field_key(FieldKey::Backspace).unwrap();
    tree.apply_field_key(FieldKey::Backspace).unwrap();
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("A"));
    tree.insert_text(field, "BC").unwrap();
    tree.set_field_selection(field, 1, 3).unwrap();
    tree.apply_ime(ImeEvent::Preedit {
        text: "あ".into(),
        cursor: Some((3, 3)),
    })
    .unwrap();
    assert_eq!(tree.get(field).unwrap().composed_text(), "Aあ");
    layout_tree(&mut tree, 320.0, 120.0);
    assert_eq!(tree.get(field).unwrap().caret, 3);
    tree.apply_ime(ImeEvent::Commit("字".into())).unwrap();
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("A字"));
    tree.patch(
        field,
        NodePatch {
            disabled: Some(true),
            ..NodePatch::default()
        },
    )
    .unwrap();
    assert!(tree.set_field_selection(field, 0, 1).is_err());
}

#[test]
fn selection_highlight_changes_pixels_and_select_all_replaces() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Edit", 320.0, 120.0).unwrap();
    let field = text_field(&mut tree, root, "Name", "Hello").unwrap();
    tree.set_focus(field).unwrap();
    layout_tree(&mut tree, 320.0, 120.0);
    let mut before = CpuRenderer::new(320, 120).unwrap();
    paint_tree(&mut before, &tree, Color::PAGE);
    tree.apply_field_key(FieldKey::SelectAll).unwrap();
    layout_tree(&mut tree, 320.0, 120.0);
    assert_eq!(tree.selected_text(field).unwrap(), "Hello");
    let mut after = CpuRenderer::new(320, 120).unwrap();
    paint_tree(&mut after, &tree, Color::PAGE);
    assert_ne!(before.rgba8().unwrap(), after.rgba8().unwrap());
    tree.insert_text(field, "Hi").unwrap();
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hi"));
}

#[test]
fn composition_is_marked_and_cancel_does_not_change_value() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Edit", 320.0, 120.0).unwrap();
    let field = text_field(&mut tree, root, "Name", "Alpha").unwrap();
    tree.set_focus(field).unwrap();
    tree.set_field_selection(field, 1, 4).unwrap();
    tree.apply_ime(ImeEvent::Preedit {
        text: "xy".into(),
        cursor: None,
    })
    .unwrap();
    layout_tree(&mut tree, 320.0, 120.0);
    let (start, end) = tree.get(field).unwrap().preedit_x.unwrap();
    assert!(start > 0.0 && end > start);
    let mut marked = CpuRenderer::new(320, 120).unwrap();
    paint_tree(&mut marked, &tree, Color::PAGE);
    let mut without_mark = tree.clone();
    without_mark.get_mut(field).unwrap().preedit_x = None;
    let mut plain = CpuRenderer::new(320, 120).unwrap();
    paint_tree(&mut plain, &without_mark, Color::PAGE);
    assert_ne!(marked.rgba8().unwrap(), plain.rgba8().unwrap());
    tree.apply_ime(ImeEvent::Disabled).unwrap();
    layout_tree(&mut tree, 320.0, 120.0);
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Alpha"));
    assert!(tree.get(field).unwrap().preedit_x.is_none());
}

#[test]
fn composition_is_cancelled_when_focus_moves_field_hides_or_text_is_empty() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Edit", 320.0, 240.0).unwrap();
    let one = text_field(&mut tree, root, "One", "Alpha").unwrap();
    let two = text_field(&mut tree, root, "Two", "Beta").unwrap();
    let preedit = |tree: &mut Tree, text: &str| {
        tree.apply_ime(ImeEvent::Preedit {
            text: text.into(),
            cursor: None,
        })
        .unwrap();
    };
    // Moving focus drops the old field's composition.
    tree.set_focus(one).unwrap();
    preedit(&mut tree, "xy");
    tree.set_focus(two).unwrap();
    assert!(tree.get(one).unwrap().preedit.is_empty());
    assert_eq!(tree.get(one).unwrap().text.as_deref(), Some("Alpha"));
    // An empty preedit clears the composition.
    preedit(&mut tree, "zz");
    preedit(&mut tree, "");
    assert!(tree.get(two).unwrap().preedit.is_empty());
    assert!(tree.get(two).unwrap().preedit_cursor.is_none());
    // Disabling a field drops its composition.
    preedit(&mut tree, "qq");
    tree.patch(
        two,
        NodePatch {
            disabled: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(tree.get(two).unwrap().preedit.is_empty());
    assert_eq!(tree.get(two).unwrap().text.as_deref(), Some("Beta"));
}

#[test]
fn composition_is_cancelled_when_its_tab_panel_is_hidden() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Edit", 320.0, 240.0).unwrap();
    let tabs = tekto::tab_list(&mut tree, root, "Sections").unwrap();
    let edit = tekto::tab(&mut tree, tabs, "Edit", true).unwrap();
    let other = tekto::tab(&mut tree, tabs, "Other", false).unwrap();
    let panel = tekto::tab_panel(&mut tree, root, edit, "Edit panel").unwrap();
    tekto::tab_panel(&mut tree, root, other, "Other panel").unwrap();
    let field = text_field(&mut tree, panel, "Name", "Alpha").unwrap();
    tree.set_focus(field).unwrap();
    tree.apply_ime(ImeEvent::Preedit {
        text: "xy".into(),
        cursor: None,
    })
    .unwrap();
    tree.patch(
        other,
        NodePatch {
            checked: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!tree.is_visible(field));
    assert!(tree.get(field).unwrap().preedit.is_empty());
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Alpha"));
}

#[test]
fn multiline_layout_wraps_selection_and_scrolls_to_caret() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Edit", 180.0, 130.0).unwrap();
    let field = text_field(&mut tree, root, "Notes", "").unwrap();
    {
        let node = tree.get_mut(field).unwrap();
        node.multiline = true;
        node.layout.width = 120.0;
        node.layout.height = 60.0;
    }
    tree.insert_text(
        field,
        "First line\nSecond line with words that wrap\nLast line",
    )
    .unwrap();
    layout_tree(&mut tree, 180.0, 130.0);
    let semantics = tekto::accesskit_dump(&tree).unwrap();
    assert!(semantics
        .nodes
        .iter()
        .any(|node| node.id == field.raw() && node.role == "MultilineTextInput"));
    let node = tree.get(field).unwrap();
    assert!(node.caret_y > 0.0 && node.caret_height > 0.0);
    assert!(node.text_scroll_y > 0.0);
    let end = node.caret;
    assert!(tekto_layout::field_move_lines(&mut tree, field, -1, false));
    assert!(tree.get(field).unwrap().caret < end);
    assert!(tekto_layout::field_move_lines(&mut tree, field, 1, true));
    assert!(tree.get(field).unwrap().selection().is_some());
    tree.apply_field_key(FieldKey::SelectAll).unwrap();
    layout_tree(&mut tree, 180.0, 130.0);
    assert!(tree.get(field).unwrap().selection_rects.len() > 1);
    let mut renderer = CpuRenderer::new(180, 130).unwrap();
    paint_tree(&mut renderer, &tree, Color::PAGE);
    assert!(renderer
        .rgba8()
        .unwrap()
        .iter()
        .any(|&channel| channel != 0));
}

#[test]
fn vertical_navigation_keeps_column_across_short_lines() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Edit", 320.0, 180.0).unwrap();
    let field = tekto::text_area(
        &mut tree,
        root,
        "Notes",
        "abcdefghij\nx\nabcdefghij",
        240.0,
        120.0,
    )
    .unwrap();
    tree.set_field_selection(field, 21, 21).unwrap();
    layout_tree(&mut tree, 320.0, 180.0);
    assert!(tekto_layout::field_move_lines(&mut tree, field, -1, false));
    assert!(tekto_layout::field_move_lines(&mut tree, field, -1, false));
    assert_eq!(tree.get(field).unwrap().caret, 8);
    tree.apply_field_key(FieldKey::Home).unwrap();
    assert!(tree.get(field).unwrap().vertical_goal_x.is_none());
}

#[test]
fn ime_selected_subrange_is_shaped_highlighted_and_cleared() {
    for multiline in [false, true] {
        let mut tree = Tree::new();
        let root = window(&mut tree, "Edit", 220.0, 160.0).unwrap();
        let field = tekto::text_area(&mut tree, root, "Notes", "ABCD", 110.0, 80.0).unwrap();
        tree.get_mut(field).unwrap().multiline = multiline;
        tree.set_field_selection(field, 1, 3).unwrap();
        tree.apply_ime(ImeEvent::Preedit {
            text: "aé界z".into(),
            cursor: Some((1, 6)),
        })
        .unwrap();
        assert_eq!(tree.get(field).unwrap().preedit_selection(), Some((2, 4)));
        layout_tree(&mut tree, 220.0, 160.0);
        assert!(!tree.get(field).unwrap().preedit_selection_rects.is_empty());
        let mut highlighted = CpuRenderer::new(220, 160).unwrap();
        paint_tree(&mut highlighted, &tree, Color::PAGE);
        let mut without_selection = tree.clone();
        without_selection
            .get_mut(field)
            .unwrap()
            .preedit_selection_rects
            .clear();
        let mut plain = CpuRenderer::new(220, 160).unwrap();
        paint_tree(&mut plain, &without_selection, Color::PAGE);
        assert_ne!(highlighted.rgba8().unwrap(), plain.rgba8().unwrap());
        tree.apply_ime(ImeEvent::Preedit {
            text: "aé界z".into(),
            cursor: Some((3, 3)),
        })
        .unwrap();
        layout_tree(&mut tree, 220.0, 160.0);
        assert!(tree.get(field).unwrap().preedit_selection_rects.is_empty());
        tree.apply_ime(ImeEvent::Disabled).unwrap();
        layout_tree(&mut tree, 220.0, 160.0);
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("ABCD"));
        assert!(tree.get(field).unwrap().preedit_selection_rects.is_empty());
    }
}

#[test]
fn caret_at_start_of_text_has_zero_offset() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Edit", 320.0, 120.0).unwrap();
    let field = text_field(&mut tree, root, "Name", "Groceries").unwrap();
    tree.set_field_selection(field, 0, 0).unwrap();
    layout_tree(&mut tree, 320.0, 120.0);
    assert_eq!(tree.get(field).unwrap().caret_x, 0.0);
    // A selection that starts at the first letter must also start at zero.
    tree.set_field_selection(field, 0, 3).unwrap();
    layout_tree(&mut tree, 320.0, 120.0);
    let (start, end) = tree.get(field).unwrap().selection_x.unwrap();
    assert_eq!(start, 0.0);
    assert!(end > 0.0);
}

#[test]
fn right_to_left_text_reports_direction_and_selection_moves_visually() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Edit", 320.0, 120.0).unwrap();
    let field = text_field(&mut tree, root, "Name", "\u{5d0}\u{5d1}\u{5d2}").unwrap();
    layout_tree(&mut tree, 320.0, 120.0);
    let chars = &tree.get(field).unwrap().text_chars;
    assert_eq!(chars.len(), 3);
    assert!(
        chars[0].x > chars[2].x,
        "first Hebrew letter sits at the right"
    );
    let update = tekto::accesskit_tree_update(&tree).unwrap();
    let parent = &update
        .nodes
        .iter()
        .find(|(id, _)| id.0 == field.raw())
        .unwrap()
        .1;
    let run = &update
        .nodes
        .iter()
        .find(|(id, _)| *id == parent.children()[0])
        .unwrap()
        .1;
    assert!(run.text_direction().is_some());
    let positions = run.character_positions().unwrap();
    assert_eq!(positions[0], 0.0, "offsets start at the right edge");
    assert!(positions[2] > positions[1]);
    // Selecting the first two letters covers a box on the right side.
    tree.set_field_selection(field, 0, 2).unwrap();
    layout_tree(&mut tree, 320.0, 120.0);
    let (start, end) = tree.get(field).unwrap().selection_x.unwrap();
    assert!(end > start);
}

#[test]
fn accesskit_runs_report_line_and_character_geometry() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Edit", 320.0, 240.0).unwrap();
    let field = tekto::text_area(&mut tree, root, "Body", "ab\ncd", f32::NAN, 120.0).unwrap();
    layout_tree(&mut tree, 320.0, 240.0);
    let update = tekto::accesskit_tree_update(&tree).unwrap();
    let parent = &update
        .nodes
        .iter()
        .find(|(id, _)| id.0 == field.raw())
        .unwrap()
        .1;
    let runs: Vec<_> = parent
        .children()
        .iter()
        .map(|id| &update.nodes.iter().find(|(n, _)| n == id).unwrap().1)
        .collect();
    assert_eq!(runs.len(), 2);
    let (first, second) = (runs[0].bounds().unwrap(), runs[1].bounds().unwrap());
    assert!(second.y0 >= first.y1 - 0.5, "lines must not overlap");
    assert!(first.y1 - first.y0 < 60.0, "run must be one line tall");
    let positions = runs[0].character_positions().unwrap();
    let widths = runs[0].character_widths().unwrap();
    assert_eq!(positions.len(), 3, "a, b, and the line break");
    assert_eq!(positions[0], 0.0);
    assert!(positions[1] > 0.0 && positions[2] >= positions[1]);
    assert_eq!(widths.len(), 3);
    assert!(widths[0] > 0.0);
}
