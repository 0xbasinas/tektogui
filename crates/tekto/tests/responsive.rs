use tekto::{layout_tree, responsive_axis, split_pane, text_field, window, Axis, Tree};

#[test]
fn split_pane_collapses_the_secondary_pane_and_restores_it() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Responsive", 800.0, 400.0).unwrap();
    let split = split_pane(&mut tree, root, 600.0, 16.0).unwrap();
    let field = text_field(&mut tree, split.secondary, "Search", "").unwrap();

    layout_tree(&mut tree, 800.0, 400.0);
    let primary = tree.get(split.primary).unwrap().bounds;
    let secondary = tree.get(split.secondary).unwrap().bounds;
    assert!(tree.is_visible(split.secondary));
    assert!(!tree.get(split.secondary).unwrap().collapsed);
    assert!(
        (primary.width - secondary.width).abs() < 1.0,
        "{primary:?} {secondary:?}"
    );
    assert!(primary.width > 300.0, "{primary:?}");

    tree.set_focus(field).unwrap();
    assert_eq!(tree.focus, Some(field));

    // Narrowing past the breakpoint hides the pane and rescues focus.
    layout_tree(&mut tree, 500.0, 400.0);
    assert!(!tree.is_visible(split.secondary));
    assert!(tree.get(split.secondary).unwrap().collapsed);
    assert_eq!(tree.focus, Some(split.root));
    let primary = tree.get(split.primary).unwrap().bounds;
    assert!(primary.width > 400.0, "{primary:?}");
    assert!(tree.get(field).is_some(), "collapsed state keeps its nodes");

    // The pane and its state come back when the window widens again.
    layout_tree(&mut tree, 800.0, 400.0);
    assert!(tree.is_visible(split.secondary));
    assert!(!tree.get(split.secondary).unwrap().collapsed);
    assert_eq!(tree.get(field).unwrap().name.as_deref(), Some("Search"));
}

#[test]
fn responsive_axis_picks_a_column_below_the_breakpoint() {
    assert_eq!(responsive_axis(400.0, 600.0), Axis::Column);
    assert_eq!(responsive_axis(600.0, 600.0), Axis::Row);
    assert_eq!(responsive_axis(900.0, 600.0), Axis::Row);
    assert_eq!(responsive_axis(f32::NAN, 600.0), Axis::Row);
}

#[test]
fn split_pane_rejects_an_invalid_breakpoint() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Responsive", 800.0, 400.0).unwrap();
    assert!(split_pane(&mut tree, root, 0.0, 8.0).is_err());
    assert!(split_pane(&mut tree, root, f32::NAN, 8.0).is_err());
}
