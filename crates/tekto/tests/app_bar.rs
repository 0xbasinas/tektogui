use tekto::{
    app_bar, app_bar_action, app_bar_leading_button, layout_tree, window, Color, NodeKind, Tree,
};

#[test]
fn app_bar_lays_out_leading_title_and_actions() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Bar", 600.0, 400.0).unwrap();
    let bar = app_bar(&mut tree, root, "Notes").unwrap();
    let back = app_bar_leading_button(&mut tree, &bar, "Back").unwrap();
    let save = app_bar_action(&mut tree, &bar, "Save").unwrap();
    layout_tree(&mut tree, 600.0, 400.0);

    let bar_node = tree.get(bar.root).unwrap();
    assert_eq!(bar_node.fill, Some(Color::BANNER));
    assert_eq!(bar_node.layout.height, tekto::APP_BAR_HEIGHT);
    assert_eq!(tree.get(bar.title).unwrap().text.as_deref(), Some("Notes"));

    let leading = tree.get(bar.leading).unwrap().bounds;
    let actions = tree.get(bar.actions).unwrap().bounds;
    assert!(
        actions.x > leading.x,
        "actions at {} are not right of leading at {}",
        actions.x,
        leading.x
    );

    let save_node = tree.get(save).unwrap();
    assert_eq!(save_node.kind, NodeKind::Button);
    assert_eq!(save_node.fill, None, "action buttons are quiet");
    assert_eq!(save_node.color, Some(Color::ON_BANNER));
    assert_eq!(save_node.name.as_deref(), Some("Save"));
    assert_eq!(tree.get(back).unwrap().kind, NodeKind::Button);

    tree.click(save).unwrap();
    assert_eq!(tree.focus, Some(save));
}

#[test]
fn app_bar_follows_the_tree_tokens() {
    let mut tree = Tree::new();
    let mut tokens = tree.tokens();
    tokens.banner = Color::rgb(0x20, 0x30, 0x40);
    tokens.on_banner = Color::rgb(0xff, 0xee, 0xdd);
    tree.set_tokens(tokens);
    let root = window(&mut tree, "Bar", 600.0, 400.0).unwrap();
    let bar = app_bar(&mut tree, root, "Tinted").unwrap();
    let action = app_bar_action(&mut tree, &bar, "Save").unwrap();
    assert_eq!(
        tree.get(bar.root).unwrap().fill,
        Some(Color::rgb(0x20, 0x30, 0x40))
    );
    assert_eq!(
        tree.get(bar.title).unwrap().color,
        Some(Color::rgb(0xff, 0xee, 0xdd))
    );
    assert_eq!(
        tree.get(action).unwrap().color,
        Some(Color::rgb(0xff, 0xee, 0xdd))
    );
}
