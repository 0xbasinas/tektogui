//! Headless snapshot: layout a Notes tree, paint with tiny-skia, compare
//! pixels to a blessed reference PNG, and assert the scene / AccessKit tree.

use std::path::PathBuf;
use tekto::{
    accesskit_dump, accesskit_tree_update, layout_tree, notes_tree, render_headless_to_file,
    BANNER_PADDING, PAGE_PADDING, STACK_GAP,
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

fn reference_phone_png() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/notes-phone-headless.png")
}

/// Phone-sized viewport (the logical size we saw on AVD beddcode30).
/// The banner used to keep a 592px width from the 640px construction size
/// and overflow a 393px window.
#[test]
fn notes_phone_size_stays_inside_the_window() {
    const W: u32 = 393;
    const H: u32 = 807;
    // Build at the desktop default, then relayout at phone size — this is
    // the Android path (tree is created at 640x400, then the window resizes).
    let mut tree = notes_tree(640.0, 400.0).expect("notes tree");
    layout_tree(&mut tree, W as f32, H as f32);

    let root = tree.root().expect("root");
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

    let window_b = tree.get(root).unwrap().bounds;
    let banner_b = tree.get(banner).unwrap().bounds;
    let title_b = tree.get(title).unwrap().bounds;
    let body_b = tree.get(body).unwrap().bounds;

    assert_eq!(window_b.width, W as f32);
    assert_eq!(window_b.height, H as f32);
    for (name, b) in [("banner", banner_b), ("title", title_b), ("body", body_b)] {
        assert!(
            b.x >= -0.5 && b.x + b.width <= window_b.width + 0.5,
            "{name} overflowed horizontally: {b:?}"
        );
        assert!(
            b.y >= -0.5 && b.y + b.height <= window_b.height + 0.5,
            "{name} overflowed vertically: {b:?}"
        );
    }
    let expected_banner_w = (W as f32 - PAGE_PADDING * 2.0).max(0.0);
    assert!(
        (banner_b.width - expected_banner_w).abs() < 0.5,
        "banner should fill the content box ({expected_banner_w}), got {}",
        banner_b.width
    );
    assert!(
        title_b.x + 0.5 >= banner_b.x + BANNER_PADDING,
        "title should stay inset: title.x={} banner.x={}",
        title_b.x,
        banner_b.x
    );

    let pretty = tree.to_pretty_text();
    assert!(pretty.contains("Hello from Tekto"), "{pretty}");

    let dir = shot_dir();
    std::fs::create_dir_all(&dir).expect("shot dir");
    let path = dir.join("notes-phone-headless.png");
    let png = render_headless_to_file(&tree, W, H, Some(&path)).expect("render");
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    compare_png(&png, &reference_phone_png());
}

#[test]
fn button_is_visible_and_clickable() {
    let mut tree = tekto::Tree::new();
    let root = tekto::window(&mut tree, "Example", 320.0, 160.0).unwrap();
    let button = tekto::button(&mut tree, root, "Save").unwrap();
    layout_tree(&mut tree, 320.0, 160.0);
    let bounds = tree.get(button).unwrap().bounds;
    assert!(bounds.width > 24.0 && bounds.height > 24.0);
    assert!(tree
        .to_pretty_text()
        .contains(&format!("button#{}", button.raw())));
    assert_eq!(
        tree.click_at(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0
        ),
        Some(button)
    );
    assert_eq!(tree.to_dump().last_click, Some(button.raw()));
    let dump = accesskit_dump(&tree).unwrap();
    assert!(dump
        .nodes
        .iter()
        .any(|n| n.id == button.raw() && n.role == "Button" && n.label.as_deref() == Some("Save")));
    let png = render_headless_to_file(&tree, 320, 160, None).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[test]
fn text_field_value_patch_keys_and_accesskit() {
    let mut tree = tekto::Tree::new();
    let root = tekto::window(&mut tree, "Notes", 320.0, 200.0).unwrap();
    let field = tekto::text_field(&mut tree, root, "Title", "Hi").unwrap();
    layout_tree(&mut tree, 320.0, 200.0);
    let node = tree.get(field).unwrap();
    assert!(node.bounds.width > 40.0 && node.bounds.height > 16.0);
    assert!(node.caret_x > 0.0, "caret should sit after Hi");
    assert!(tree
        .to_pretty_text()
        .contains(&format!("text_field#{}", field.raw())));
    assert_eq!(tree.node_dump(field).unwrap().text.as_deref(), Some("Hi"));
    assert_eq!(
        tree.node_dump(field).unwrap().name.as_deref(),
        Some("Title")
    );

    tree.patch(
        field,
        tekto::NodePatch {
            text: Some("Hello".into()),
            ..tekto::NodePatch::default()
        },
    )
    .unwrap();
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hello"));
    assert_eq!(tree.get(field).unwrap().name.as_deref(), Some("Title"));

    let bounds = tree.get(field).unwrap().bounds;
    assert_eq!(
        tree.click_at(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0
        ),
        Some(field)
    );
    assert_eq!(tree.focus, Some(field));
    assert!(tree.apply_field_key(tekto::FieldKey::Backspace).unwrap());
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hell"));
    assert!(tree.apply_field_key(tekto::FieldKey::Insert('o')).unwrap());
    layout_tree(&mut tree, 320.0, 200.0);
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hello"));
    assert!(tree.get(field).unwrap().caret_x > 0.0);

    let dump = accesskit_dump(&tree).unwrap();
    let ak = dump.nodes.iter().find(|n| n.id == field.raw()).unwrap();
    assert_eq!(ak.role, "TextInput");
    assert_eq!(ak.label.as_deref(), Some("Title"));
    assert_eq!(ak.value.as_deref(), Some("Hello"));
    assert_eq!(dump.focus, field.raw());

    let png = render_headless_to_file(&tree, 320, 200, None).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[test]
fn text_field_ime_preedit_commit_updates_accesskit() {
    let mut tree = tekto::Tree::new();
    let root = tekto::window(&mut tree, "Notes", 320.0, 200.0).unwrap();
    let field = tekto::text_field(&mut tree, root, "Title", "Hi").unwrap();
    tree.set_focus(field).unwrap();
    layout_tree(&mut tree, 320.0, 200.0);

    tree.apply_ime(tekto::ImeEvent::Preedit {
        text: "`".into(),
        cursor: Some((0, 0)),
    })
    .unwrap();
    layout_tree(&mut tree, 320.0, 200.0);
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hi"));
    assert_eq!(tree.get(field).unwrap().composed_text(), "Hi`");
    let dump = accesskit_dump(&tree).unwrap();
    let ak = dump.nodes.iter().find(|n| n.id == field.raw()).unwrap();
    assert_eq!(ak.value.as_deref(), Some("Hi`"));
    assert_eq!(ak.label.as_deref(), Some("Title"));

    tree.apply_ime(tekto::ImeEvent::Preedit {
        text: String::new(),
        cursor: None,
    })
    .unwrap();
    tree.apply_ime(tekto::ImeEvent::Commit("é".into())).unwrap();
    tree.apply_ime(tekto::ImeEvent::Commit("你".into()))
        .unwrap();
    layout_tree(&mut tree, 320.0, 200.0);
    assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hié你"));
    assert!(tree.get(field).unwrap().caret_x > 0.0);
    let dump = accesskit_dump(&tree).unwrap();
    let ak = dump.nodes.iter().find(|n| n.id == field.raw()).unwrap();
    assert_eq!(ak.role, "TextInput");
    assert_eq!(ak.value.as_deref(), Some("Hié你"));
    assert_eq!(dump.focus, field.raw());
}
