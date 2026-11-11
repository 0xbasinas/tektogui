use tekto::{
    app_bar, app_bar_action, app_bar_leading_button, app_bar_window_controls, layout_tree, window,
    Color, NodeKind, NodePatch, Tree, WindowControl,
};

#[test]
fn app_bar_lays_out_leading_title_and_actions() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Bar", 600.0, 400.0).unwrap();
    let bar = app_bar(&mut tree, root, "Notes").unwrap();
    let back = app_bar_leading_button(&mut tree, &bar, "Back").unwrap();
    let save = app_bar_action(&mut tree, &bar, "Save").unwrap();
    let controls = app_bar_window_controls(&mut tree, &bar).unwrap();
    layout_tree(&mut tree, 600.0, 400.0);

    let bar_node = tree.get(bar.root).unwrap();
    assert_eq!(bar_node.fill, Some(Color::BANNER));
    assert_eq!(bar_node.layout.height, tekto::APP_BAR_HEIGHT);
    assert!(bar_node.drag_window, "the bar is a window drag region");
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

    // The title area drags the window; a button inside the bar does not.
    let title = tree.get(bar.title).unwrap().bounds;
    assert!(tree.drag_region_at(title.x + 4.0, title.y + 4.0));
    let save_bounds = tree.get(save).unwrap().bounds;
    assert!(!tree.drag_region_at(save_bounds.x + 2.0, save_bounds.y + 2.0));

    // Window controls carry the native action for the platform host.
    assert_eq!(
        tree.get(controls[0]).unwrap().window_control,
        WindowControl::Minimize
    );
    assert_eq!(
        tree.get(controls[1]).unwrap().window_control,
        WindowControl::Maximize
    );
    assert_eq!(
        tree.get(controls[2]).unwrap().window_control,
        WindowControl::Close
    );

    tree.click(save).unwrap();
    assert_eq!(tree.focus, Some(save));

    // A click on a control records the request once for the desktop host.
    tree.click(controls[2]).unwrap();
    assert_eq!(tree.take_window_control(), Some(WindowControl::Close));
    assert_eq!(tree.take_window_control(), None);
}

#[test]
fn window_chrome_can_be_patched() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Bar", 400.0, 300.0).unwrap();
    let bar = app_bar(&mut tree, root, "Notes").unwrap();
    tree.patch(
        bar.root,
        NodePatch {
            window_control: Some("close".into()),
            ..NodePatch::default()
        },
    )
    .unwrap();
    assert_eq!(
        tree.get(bar.root).unwrap().window_control,
        WindowControl::Close
    );
    tree.patch(
        bar.root,
        NodePatch {
            window_control: Some("none".into()),
            drag_window: Some(false),
            ..NodePatch::default()
        },
    )
    .unwrap();
    assert_eq!(
        tree.get(bar.root).unwrap().window_control,
        WindowControl::None
    );
    assert!(!tree.get(bar.root).unwrap().drag_window);
    assert!(tree
        .patch(
            bar.root,
            NodePatch {
                window_control: Some("explode".into()),
                ..NodePatch::default()
            },
        )
        .is_err());
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
