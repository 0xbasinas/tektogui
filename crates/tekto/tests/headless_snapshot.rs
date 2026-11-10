//! Headless snapshot: layout a Notes tree, paint with tiny-skia, assert
//! the PNG and the scene / AccessKit tree.

use std::path::PathBuf;
use tekto::{
    accesskit_dump, accesskit_tree_update, layout_tree, notes_tree, render_headless_to_file,
};

fn shot_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TEKTO_SHOT_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tektogui-shots")
}

#[test]
fn notes_headless_snapshot() {
    let mut tree = notes_tree(640.0, 400.0).expect("notes tree");
    layout_tree(&mut tree, 640.0, 400.0);

    let pretty = tree.to_pretty_text();
    assert!(pretty.contains("window#1"), "{pretty}");
    assert!(pretty.contains("Hello from Tekto"), "{pretty}");

    // A click in the body text should hit that node (logical pixels, after layout).
    let body = tree
        .ids()
        .find(|id| tree.get(*id).and_then(|n| n.text.as_deref()) == Some("Hello from Tekto"))
        .expect("body text");
    let b = tree.get(body).unwrap().bounds;
    let hit = tree
        .click_at(b.x + b.width / 2.0, b.y + b.height / 2.0)
        .expect("hit body");
    assert_eq!(hit, body);
    assert_eq!(tree.last_click, Some(body));
    assert_eq!(tree.focus, Some(body));

    let update = accesskit_tree_update(&tree).expect("accesskit");
    assert!(update.nodes.len() >= 3);
    let root = tree.root().expect("root");
    assert_eq!(update.focus.0, tree.focus.unwrap_or(root).raw());
    let dump = accesskit_dump(&tree).expect("dump");
    assert!(dump
        .nodes
        .iter()
        .any(|n| n.role == "Window" && n.id == root.raw()));
    assert!(dump.nodes.iter().any(|n| {
        n.role == "Label"
            && n.value.as_deref() == Some("Hello from Tekto")
            && n.bounds[2] > n.bounds[0]
            && n.bounds[3] > n.bounds[1]
    }));

    let dir = shot_dir();
    std::fs::create_dir_all(&dir).expect("shot dir");
    let path = dir.join("notes-headless.png");
    let png = render_headless_to_file(&tree, 640, 400, Some(&path)).expect("render");
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(path.is_file(), "missing {}", path.display());
    assert!(png.len() > 400, "png too small: {}", png.len());
}
