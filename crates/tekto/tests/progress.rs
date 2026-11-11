use tekto::{
    accesskit_dump, layout_tree, paint_tree, progress, window, Color, CpuRenderer, NodePatch,
    Renderer, Tree,
};

#[test]
fn progress_exposes_range_clamps_and_paints() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Tasks", 240.0, 100.0).unwrap();
    let id = progress(&mut tree, root, "Upload", 0.25).unwrap();
    assert!(progress(&mut tree, root, "Invalid", f32::NAN).is_err());
    layout_tree(&mut tree, 240.0, 100.0);
    let dump = accesskit_dump(&tree).unwrap();
    let node = dump.nodes.iter().find(|node| node.id == id.raw()).unwrap();
    assert_eq!(node.role, "ProgressIndicator");
    assert_eq!(node.label.as_deref(), Some("Upload"));
    assert_eq!(node.numeric_value, Some(0.25));
    assert_eq!(
        (node.min_numeric_value, node.max_numeric_value),
        (Some(0.0), Some(1.0))
    );
    let mut before = CpuRenderer::new(240, 100).unwrap();
    paint_tree(&mut before, &tree, Color::PAGE);
    tree.patch(
        id,
        NodePatch {
            value: Some(2.0),
            ..NodePatch::default()
        },
    )
    .unwrap();
    assert_eq!(tree.get(id).unwrap().progress, 1.0);
    let mut after = CpuRenderer::new(240, 100).unwrap();
    paint_tree(&mut after, &tree, Color::PAGE);
    assert_ne!(before.rgba8().unwrap(), after.rgba8().unwrap());
    assert!(tree
        .patch(
            id,
            NodePatch {
                value: Some(f32::NAN),
                ..NodePatch::default()
            }
        )
        .is_err());
    assert_eq!(tree.get(id).unwrap().progress, 1.0);
    assert_eq!(tree.node_dump(id).unwrap().progress, 1.0);
}
