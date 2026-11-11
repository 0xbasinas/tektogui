use tekto::{
    accesskit_tree_update, image, layout_tree, paint_tree, window, Color, CpuRenderer, Image,
    ImageFit, Renderer, Tree,
};

#[test]
fn image_widget_paints_and_exposes_semantics() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Images", 100.0, 100.0).unwrap();
    let data = Image::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
    assert!(image(
        &mut tree,
        root,
        data.clone(),
        20.0,
        20.0,
        ImageFit::Contain,
        Some("")
    )
    .is_err());
    let id = image(
        &mut tree,
        root,
        data.clone(),
        20.0,
        20.0,
        ImageFit::Contain,
        Some("Logo"),
    )
    .unwrap();
    let decorative = image(&mut tree, root, data, 10.0, 10.0, ImageFit::Cover, None).unwrap();
    layout_tree(&mut tree, 100.0, 100.0);
    let metadata = tree.node_dump(id).unwrap().image.unwrap();
    assert_eq!((metadata.width, metadata.height), (Some(2), Some(1)));
    assert!(!metadata.decorative);
    assert!(
        tree.node_dump(decorative)
            .unwrap()
            .image
            .unwrap()
            .decorative
    );
    let update = accesskit_tree_update(&tree).unwrap();
    let node = &update
        .nodes
        .iter()
        .find(|(node_id, _)| node_id.0 == id.raw())
        .unwrap()
        .1;
    assert_eq!(format!("{:?}", node.role()), "Image");
    assert_eq!(node.label(), Some("Logo"));
    let hidden = &update
        .nodes
        .iter()
        .find(|(node_id, _)| node_id.0 == decorative.raw())
        .unwrap()
        .1;
    assert!(hidden.is_hidden());
    let mut renderer = CpuRenderer::new(100, 100).unwrap();
    paint_tree(&mut renderer, &tree, Color::PAGE);
    let pixels = renderer.rgba8().unwrap();
    let bounds = tree.get(id).unwrap().bounds;
    let x = bounds.x as usize + 2;
    let y = bounds.y as usize + 10;
    assert_eq!(
        &pixels[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4],
        &[255, 0, 0, 255]
    );
}
