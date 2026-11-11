use tekto::{layout_tree, text_field, window, Tree};

fn field_tree() -> (Tree, tekto::NodeId) {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Caret", 320.0, 240.0).unwrap();
    let field = text_field(&mut tree, root, "Name", "").unwrap();
    layout_tree(&mut tree, 320.0, 240.0);
    tree.set_focus(field).unwrap();
    (tree, field)
}

#[test]
fn caret_follows_typed_text() {
    let (mut tree, field) = field_tree();
    tree.type_keyboard_text("georg").unwrap();
    layout_tree(&mut tree, 320.0, 240.0);
    let (caret, caret_x) = {
        let node = tree.get(field).unwrap();
        (node.caret, node.caret_x)
    };
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("georg"));
    assert_eq!(caret, 5);
    assert!(caret_x > 0.0, "caret x stayed at {caret_x}");

    // Backspace moves the caret back with the text.
    tree.apply_field_key(tekto::FieldKey::Backspace).unwrap();
    layout_tree(&mut tree, 320.0, 240.0);
    assert_eq!(tree.get(field).unwrap().caret, 4);
    let short = tree.get(field).unwrap().caret_x;
    assert!(short < caret_x, "{short} is not left of {caret_x}");
}

#[test]
fn caret_blinks_and_typing_restores_it() {
    let (mut tree, _field) = field_tree();
    assert!(tree.caret_blinking());
    assert!(tree.caret_visible());
    tree.advance(0.0);
    assert!(tree.caret_visible());
    tree.advance(Tree::CARET_BLINK_MS);
    assert!(
        !tree.caret_visible(),
        "caret should be off after one half period"
    );
    tree.advance(2.0 * Tree::CARET_BLINK_MS);
    assert!(tree.caret_visible(), "caret should be on again");
    tree.advance(3.0 * Tree::CARET_BLINK_MS);
    assert!(!tree.caret_visible());

    // Typing makes it solid immediately and restarts the clock.
    tree.type_keyboard_text("a").unwrap();
    assert!(tree.caret_visible());
    let next = tree.next_caret_toggle_ms().expect("still blinking");
    assert!(
        (next - Tree::CARET_BLINK_MS).abs() < 1.0,
        "next toggle in {next}"
    );

    // Blurring stops the blink.
    let root = tree.root().unwrap();
    tree.set_focus(root).unwrap();
    assert!(!tree.caret_blinking());
    assert!(!tree.caret_visible());
    assert_eq!(tree.next_caret_toggle_ms(), None);
}
