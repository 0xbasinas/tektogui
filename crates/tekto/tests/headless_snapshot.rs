//! Headless snapshot: layout a Notes tree, paint with tiny-skia, compare
//! pixels to a blessed reference PNG, and assert the scene / AccessKit tree.

use std::path::PathBuf;
use tekto::{
    accesskit_dump, accesskit_tree_update, layout_tree, notes_tree, render_headless_to_file,
    BANNER_PADDING, STACK_GAP,
};

fn shot_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TEKTO_SHOT_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tektogui-shots")
}

fn reference_png() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/notes-headless.png")
}

fn compare_png(actual: &[u8], expected_path: &std::path::Path) {
    let expected = std::fs::read(expected_path).unwrap_or_default();
    if actual == expected {
        return;
    }
    if std::env::var_os("TEKTO_BLESS").is_some() {
        std::fs::create_dir_all(expected_path.parent().unwrap()).expect("snapshot dir");
        std::fs::write(expected_path, actual).expect("bless snapshot");
        eprintln!("blessed {}", expected_path.display());
        return;
    }
    let debug = shot_dir().join("notes-headless-actual.png");
    let _ = std::fs::create_dir_all(shot_dir());
    let _ = std::fs::write(&debug, actual);
    panic!(
        "Notes snapshot pixels changed ({} actual bytes, {} reference bytes).\n\
         Wrote {} for inspection.\n\
         If the change is intentional, bless it with:\n\
         TEKTO_BLESS=1 cargo test -p tekto --test headless_snapshot",
        actual.len(),
        expected.len(),
        debug.display()
    );
}

#[test]
fn notes_headless_snapshot() {
    let mut tree = notes_tree(640.0, 400.0).expect("notes tree");
    layout_tree(&mut tree, 640.0, 400.0);

    let pretty = tree.to_pretty_text();
    assert!(pretty.contains("window#1"), "{pretty}");
    assert!(pretty.contains("Hello from Tekto"), "{pretty}");

    // Title is padded and not flush with the banner edges.
    let banner = tree
        .ids()
        .find(|id| tree.get(*id).and_then(|n| n.name.as_deref()) == Some("banner"))
        .expect("banner");
    let title = tree
        .ids()
        .find(|id| {
            tree.get(*id)
                .map(|n| n.text.as_deref() == Some("Notes") && n.parent == Some(banner))
                .unwrap_or(false)
        })
        .expect("title");
    let body = tree
        .ids()
        .find(|id| tree.get(*id).and_then(|n| n.text.as_deref()) == Some("Hello from Tekto"))
        .expect("body text");
    let banner_b = tree.get(banner).unwrap().bounds;
    let title_b = tree.get(title).unwrap().bounds;
    let body_b = tree.get(body).unwrap().bounds;
    assert!(
        title_b.x + 0.5 >= banner_b.x + BANNER_PADDING,
        "title should be inset: title.x={} banner.x={}",
        title_b.x,
        banner_b.x
    );
    assert!(
        title_b.y > banner_b.y + 1.0,
        "title should not sit on the banner's top edge: title.y={} banner.y={}",
        title_b.y,
        banner_b.y
    );
    assert!(
        body_b.y + 0.5 >= banner_b.y + banner_b.height + STACK_GAP,
        "body should sit a full gap under the banner: body.y={} banner bottom={}",
        body_b.y,
        banner_b.y + banner_b.height
    );

    // A click in the body text should hit that node (logical pixels, after layout).
    let b = body_b;
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
    compare_png(&png, &reference_png());
}
