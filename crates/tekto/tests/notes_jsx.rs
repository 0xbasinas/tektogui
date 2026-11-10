//! The TSX Notes sketch (`packages/jsx-runtime/notes-app.js`) lowers to
//! the same scene tree as `notes_tree` (stable ids, banner, title, body).

use tekto::{notes_tree, Color, NodeId, NodeKind, BANNER_HEIGHT, BODY_SIZE, TITLE_SIZE};
use tekto_js::{apply_ops, HostOp, TreeHost};

fn notes_jsx_ops() -> Vec<HostOp> {
    // Order matches `mount()` / `renderToOps()` in @tektogui/jsx-runtime.
    vec![
        HostOp::Create {
            kind: "window".into(),
        },
        HostOp::SetProp {
            id: 1,
            key: "name".into(),
            value: "Notes".into(),
        },
        HostOp::SetProp {
            id: 1,
            key: "fill".into(),
            value: "#f4f3f0".into(),
        },
        HostOp::Create {
            kind: "rect".into(),
        },
        HostOp::SetProp {
            id: 2,
            key: "name".into(),
            value: "banner".into(),
        },
        HostOp::SetProp {
            id: 2,
            key: "fill".into(),
            value: "#3a424a".into(),
        },
        HostOp::Create {
            kind: "text".into(),
        },
        HostOp::SetProp {
            id: 3,
            key: "fontSize".into(),
            value: "22".into(),
        },
        HostOp::SetProp {
            id: 3,
            key: "color".into(),
            value: "#f4f3f0".into(),
        },
        HostOp::SetText {
            id: 3,
            text: "Notes".into(),
        },
        HostOp::Append {
            parent: 2,
            child: 3,
        },
        HostOp::Append {
            parent: 1,
            child: 2,
        },
        HostOp::Create {
            kind: "text".into(),
        },
        HostOp::SetProp {
            id: 4,
            key: "fontSize".into(),
            value: "20".into(),
        },
        HostOp::SetProp {
            id: 4,
            key: "color".into(),
            value: "#2c333a".into(),
        },
        HostOp::SetText {
            id: 4,
            text: "Hello from Tekto".into(),
        },
        HostOp::Append {
            parent: 1,
            child: 4,
        },
    ]
}

fn id(n: u64) -> NodeId {
    NodeId::from_raw(n)
}

#[test]
fn notes_jsx_ops_match_the_rust_notes_tree() {
    let host = TreeHost::new();
    let created = apply_ops(&mut host.tree.borrow_mut(), &notes_jsx_ops()).expect("ops");
    assert_eq!(created, vec![1, 2, 3, 4]);

    let jsx = host.tree.borrow();
    let rust = notes_tree(640.0, 400.0).expect("rust notes");

    let pairs = [
        (1, NodeKind::Window, Some("Notes"), None),
        (2, NodeKind::Rect, Some("banner"), None),
        (3, NodeKind::Text, Some("Notes"), Some("Notes")),
        (
            4,
            NodeKind::Text,
            Some("Hello from Tekto"),
            Some("Hello from Tekto"),
        ),
    ];
    for (raw, kind, name, text) in pairs {
        let j = jsx.get(id(raw)).expect("jsx node");
        let r = rust.get(id(raw)).expect("rust node");
        assert_eq!(j.kind, kind, "jsx #{raw} kind");
        assert_eq!(r.kind, kind, "rust #{raw} kind");
        assert_eq!(j.kind, r.kind);
        if let Some(expected) = text {
            assert_eq!(j.text.as_deref(), Some(expected));
            assert_eq!(r.text.as_deref(), Some(expected));
        }
        if let Some(expected) = name {
            // Window / banner use `name`; text nodes prefer `text` in the pretty tree.
            if kind != NodeKind::Text {
                assert_eq!(j.name.as_deref(), Some(expected));
                assert_eq!(r.name.as_deref(), Some(expected));
            }
        }
        assert_eq!(j.parent, r.parent, "parent of #{raw}");
        assert_eq!(
            rust.get(id(raw)).unwrap().children,
            jsx.get(id(raw)).unwrap().children,
            "children of #{raw}"
        );
    }

    assert_eq!(jsx.get(id(1)).unwrap().fill, Some(Color::PAGE));
    assert_eq!(jsx.get(id(2)).unwrap().fill, Some(Color::BANNER));
    assert_eq!(jsx.get(id(3)).unwrap().color, Some(Color::ON_BANNER));
    assert_eq!(jsx.get(id(3)).unwrap().font_size, TITLE_SIZE);
    assert_eq!(jsx.get(id(4)).unwrap().color, Some(Color::INK));
    assert_eq!(jsx.get(id(4)).unwrap().font_size, BODY_SIZE);
    assert_eq!(rust.get(id(1)).unwrap().fill, Some(Color::PAGE));
    assert_eq!(rust.get(id(2)).unwrap().fill, Some(Color::BANNER));
    assert_eq!(rust.get(id(2)).unwrap().layout.height, BANNER_HEIGHT);
    assert_eq!(rust.get(id(3)).unwrap().font_size, TITLE_SIZE);
    assert_eq!(rust.get(id(4)).unwrap().font_size, BODY_SIZE);

    let pretty = jsx.to_pretty_text();
    assert!(pretty.contains("window#1"), "{pretty}");
    assert!(pretty.contains("banner"), "{pretty}");
    assert!(pretty.contains("Hello from Tekto"), "{pretty}");
}
