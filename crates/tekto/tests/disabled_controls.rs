use tekto::{
    accesskit_dump, button, layout_tree, render_headless_to_file, text_field, window, FieldKey,
    ImeEvent, NodePatch, Tree,
};

#[test]
fn disabled_controls_share_state_across_input_accessibility_and_paint() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Test", 300.0, 200.0).unwrap();
    let button = button(&mut tree, root, "Save").unwrap();
    let field = text_field(&mut tree, root, "Title", "hello").unwrap();
    layout_tree(&mut tree, 300.0, 200.0);
    let enabled = render_headless_to_file(&tree, 300, 200, None).unwrap();
    tree.set_focus(field).unwrap();
    tree.apply_ime(ImeEvent::Preedit {
        text: "x".into(),
        cursor: None,
    })
    .unwrap();
    for id in [button, field] {
        tree.patch(
            id,
            NodePatch {
                disabled: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
    }
    assert_eq!(tree.focus, Some(root));
    assert!(tree.get(field).unwrap().preedit.is_empty());
    assert!(!tree.traverse_focus(false));
    assert!(tree.click(button).is_err());
    assert!(tree.click(field).is_err());
    assert!(tree.insert_text(field, "typed").is_err());
    let f = tree.get(field).unwrap().bounds;
    assert_eq!(
        tree.click_at(f.x + f.width / 2.0, f.y + f.height / 2.0),
        None
    );
    assert_eq!(tree.focus, Some(root));
    assert_eq!(
        tekto_tree::route_named_key(&mut tree, tekto_tree::NamedActionKey::Enter, false).1,
        None
    );
    assert!(tree.insert_text(field, "!").is_err());
    let b = tree.get(button).unwrap().bounds;
    assert_eq!(
        tree.click_at(b.x + b.width / 2.0, b.y + b.height / 2.0),
        None
    );
    assert_eq!(tree.last_click, None);
    assert!(!tree.apply_field_key(FieldKey::Backspace).unwrap());
    assert!(!tree.type_keyboard_text("!").unwrap());
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("hello"));
    assert!(tree.to_pretty_text().contains("disabled=true"));
    let a11y = accesskit_dump(&tree).unwrap();
    assert!(
        a11y.nodes
            .iter()
            .find(|n| n.id == button.raw())
            .unwrap()
            .disabled
    );
    assert!(
        a11y.nodes
            .iter()
            .find(|n| n.id == field.raw())
            .unwrap()
            .disabled
    );
    let muted = render_headless_to_file(&tree, 300, 200, None).unwrap();
    assert_ne!(enabled, muted);
    for id in [button, field] {
        tree.patch(
            id,
            NodePatch {
                disabled: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
    }
    assert_eq!(
        enabled,
        render_headless_to_file(&tree, 300, 200, None).unwrap()
    );
    assert!(tree.traverse_focus(false));
    assert_eq!(tree.focus, Some(button));
    tree.click(button).unwrap();
    tree.insert_text(field, "!").unwrap();
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("hello!"));
}

#[test]
fn focus_skips_disabled_middle_control_in_both_directions() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Test", 300.0, 200.0).unwrap();
    let first = button(&mut tree, root, "First").unwrap();
    let middle = text_field(&mut tree, root, "Middle", "").unwrap();
    let last = button(&mut tree, root, "Last").unwrap();
    tree.patch(
        middle,
        NodePatch {
            disabled: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    tree.set_focus(first).unwrap();
    assert!(tree.traverse_focus(false));
    assert_eq!(tree.focus, Some(last));
    assert!(tree.traverse_focus(true));
    assert_eq!(tree.focus, Some(first));
    tree.patch(
        first,
        NodePatch {
            disabled: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    // Simulate a stale focus to exercise the keyboard guard directly.
    tree.focus = Some(first);
    assert_eq!(
        tekto_tree::route_named_key(&mut tree, tekto_tree::NamedActionKey::Space, false).1,
        None
    );
    assert!(tree
        .patch(
            root,
            NodePatch {
                disabled: Some(true),
                ..Default::default()
            }
        )
        .is_err());
    assert!(!tree.get(root).unwrap().disabled);
}
