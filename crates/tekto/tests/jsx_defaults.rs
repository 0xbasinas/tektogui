use tekto_js::{apply_ops, HostOp};
use tekto_tree::{Color, Node, NodeId, Tree};
use tekto_widgets::{button, text_field, window};

fn same_style(rust: &Node, jsx: &Node) {
    assert_eq!(rust.fill, jsx.fill);
    assert_eq!(rust.color, jsx.color);
    assert_eq!(rust.font_size, jsx.font_size);
    assert_eq!(rust.layout.padding, jsx.layout.padding);
    assert_eq!(rust.layout.gap, jsx.layout.gap);
    assert_eq!(rust.layout.column, jsx.layout.column);
}

#[test]
fn jsx_controls_match_rust_helpers_and_keep_overrides() {
    let mut rust = Tree::new();
    let root = window(&mut rust, "Window", 640.0, 400.0).unwrap();
    let button_id = button(&mut rust, root, "Save").unwrap();
    let field_id = text_field(&mut rust, root, "Title", "Hello").unwrap();
    let mut jsx = Tree::new();
    for (kind, id) in [
        ("window", root),
        ("button", button_id),
        ("text_field", field_id),
    ] {
        let ids = apply_ops(&mut jsx, &[HostOp::Create { kind: kind.into() }]).unwrap();
        same_style(
            rust.get(id).unwrap(),
            jsx.get(NodeId::from_raw(ids[0])).unwrap(),
        );
    }
    for (kind, reverse) in [("window", false), ("button", true), ("text_field", false)] {
        let id = apply_ops(&mut jsx, &[HostOp::Create { kind: kind.into() }]).unwrap()[0];
        let mut props = vec![
            ("fill", "#112233"),
            ("color", "#445566"),
            ("fontSize", "31"),
            ("padding", "7"),
            ("gap", "9"),
            ("column", "false"),
        ];
        if reverse {
            props.reverse();
        }
        let ops: Vec<_> = props
            .into_iter()
            .map(|(key, value)| HostOp::SetProp {
                id,
                key: key.into(),
                value: value.into(),
            })
            .collect();
        apply_ops(&mut jsx, &ops).unwrap();
        let node = jsx.get(NodeId::from_raw(id)).unwrap();
        assert_eq!(node.fill, Some(Color::rgba(0x11, 0x22, 0x33, 0xff)));
        assert_eq!(node.color, Some(Color::rgba(0x44, 0x55, 0x66, 0xff)));
        assert_eq!(node.font_size, 31.0);
        assert_eq!(node.layout.padding, 7.0);
        assert_eq!(node.layout.gap, 9.0);
        assert!(!node.layout.column);
    }
}
