use tekto::{
    layout_tree, slider, text_field, window, Color, Easing, NodeId, NodeKind, StyleTransition, Tree,
};

#[test]
fn only_layout_affecting_changes_ask_for_a_pass() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Dirty", 400.0, 300.0).unwrap();
    let field = text_field(&mut tree, root, "Name", "").unwrap();
    let slider = slider(&mut tree, root, "Volume", 0.5).unwrap();
    layout_tree(&mut tree, 400.0, 300.0);
    assert!(!tree.layout_dirty(), "a layout pass clears the flag");

    // Paint-only changes do not need layout.
    tree.patch(
        slider,
        tekto::NodePatch {
            fill: Some("#112233".into()),
            ..tekto::NodePatch::default()
        },
    )
    .unwrap();
    assert!(tree.layout_dirty(), "patch can change layout");
    layout_tree(&mut tree, 400.0, 300.0);
    assert!(!tree.layout_dirty());

    tree.transition(
        field,
        StyleTransition {
            fill: Some(Color::rgb(1, 2, 3)),
            opacity: Some(0.5),
            easing: Some(Easing::Linear),
            duration_ms: Some(100.0),
            ..StyleTransition::default()
        },
    )
    .unwrap();
    tree.advance(0.0);
    tree.advance(50.0);
    assert!(
        !tree.layout_dirty(),
        "colour and opacity tweens must not force layout"
    );

    // A position tween does need layout.
    tree.transition(
        field,
        StyleTransition {
            x: Some(20.0),
            duration_ms: Some(100.0),
            ..StyleTransition::default()
        },
    )
    .unwrap();
    tree.advance(100.0);
    assert!(tree.layout_dirty(), "a position tween needs layout");
    layout_tree(&mut tree, 400.0, 300.0);
    assert!(!tree.layout_dirty());
}

#[test]
fn structural_and_field_changes_mark_layout() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Dirty", 400.0, 300.0).unwrap();
    let field = text_field(&mut tree, root, "Name", "").unwrap();
    layout_tree(&mut tree, 400.0, 300.0);
    assert!(!tree.layout_dirty());

    tree.insert_text(field, "hi").unwrap();
    assert!(tree.layout_dirty());
    layout_tree(&mut tree, 400.0, 300.0);

    let extra: NodeId = tree.add(NodeKind::Rect, Some(root)).unwrap();
    assert!(tree.layout_dirty());
    layout_tree(&mut tree, 400.0, 300.0);
    assert!(!tree.layout_dirty());

    tree.remove(extra).unwrap();
    assert!(tree.layout_dirty());
}
