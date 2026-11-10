use tekto::{layout_tree, text_field, window, Action, Recording, Tree};

fn app() -> Tree {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Notes", 320.0, 200.0).unwrap();
    text_field(&mut tree, root, "Title", "Hi").unwrap();
    layout_tree(&mut tree, 320.0, 200.0);
    tree
}

#[test]
fn recorded_session_replays_to_the_same_tree() {
    let fixture = include_str!("fixtures/session.json");
    let script: Recording = serde_json::from_str(fixture).unwrap();
    let mut original = app();
    let mut recorded = Recording::default();
    for action in script.events.clone() {
        recorded.record(&mut original, action).unwrap();
        layout_tree(&mut original, 320.0, 200.0);
    }
    let json = serde_json::to_string(&recorded).unwrap();
    let loaded: Recording = serde_json::from_str(&json).unwrap();
    let mut replayed = app();
    loaded
        .replay(&mut replayed, |tree| layout_tree(tree, 320.0, 200.0))
        .unwrap();
    assert_eq!(original.to_pretty_text(), replayed.to_pretty_text());
    assert!(replayed
        .to_pretty_text()
        .contains("text_field#2 name=\"Title\" value=\"Hello?\""));
    assert_eq!(original.focus, replayed.focus);
    assert_eq!(original.last_click, replayed.last_click);
    assert_eq!(
        replayed
            .get(tekto::NodeId::from_raw(2))
            .unwrap()
            .text
            .as_deref(),
        Some("Hello?")
    );
    assert!(matches!(
        loaded.events.last(),
        Some(Action::PointerClick { .. })
    ));
    let mut unsupported = loaded;
    unsupported.version = 2;
    assert!(unsupported.replay(&mut replayed, |_| {}).is_err());
}
