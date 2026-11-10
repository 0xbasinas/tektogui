//! One headless check: a scroll container clips a tall child, and a scroll
//! offset (a short wheel step, not a page) changes the pixels.

use tekto::{
    layout_tree, paint_tree, scroll, window, Color, CpuRenderer, LayoutStyle, NodeKind, Renderer,
    Tree, SCROLL_WHEEL_LINE,
};

fn sample(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * width + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2]]
}

#[test]
fn scroll_clips_tall_child_and_offset_changes_pixels() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Scroll", 200.0, 200.0).unwrap();
    {
        let n = tree.get_mut(root).unwrap();
        n.layout.padding = 0.0;
        n.layout.gap = 0.0;
        n.fill = Some(Color::PAGE);
    }
    let viewport = scroll(&mut tree, root, 200.0, 100.0).unwrap();
    let top = tree.add(NodeKind::Rect, Some(viewport)).unwrap();
    tree.get_mut(top).unwrap().fill = Some(Color::INK);
    tree.get_mut(top).unwrap().layout = LayoutStyle::sized(200.0, 100.0);
    let below = tree.add(NodeKind::Rect, Some(viewport)).unwrap();
    tree.get_mut(below).unwrap().fill = Some(Color::BANNER);
    tree.get_mut(below).unwrap().layout = LayoutStyle::sized(200.0, 100.0);

    layout_tree(&mut tree, 200.0, 200.0);
    let extent = tree.get(viewport).unwrap().scroll_extent;
    assert!(
        extent > 100.0 + 50.0,
        "content should be taller than the viewport, extent={extent}"
    );

    let mut renderer = CpuRenderer::new(200, 200).unwrap();
    paint_tree(&mut renderer, &tree, Color::PAGE);
    let rgba = renderer.rgba8().unwrap();
    let ink = [Color::INK.r, Color::INK.g, Color::INK.b];
    let page = [Color::PAGE.r, Color::PAGE.g, Color::PAGE.b];
    let banner = [Color::BANNER.r, Color::BANNER.g, Color::BANNER.b];
    assert_eq!(sample(&rgba, 200, 40, 40), ink, "top of the viewport");
    assert_eq!(
        sample(&rgba, 200, 40, 150),
        page,
        "the tall child must be clipped below the scroll box"
    );

    let png = renderer.to_png().unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));

    let stepped = tree.scroll_by(viewport, SCROLL_WHEEL_LINE).unwrap();
    assert!(
        (stepped - SCROLL_WHEEL_LINE).abs() < 0.5,
        "a wheel notch should move {SCROLL_WHEEL_LINE}px, got {stepped}"
    );
    layout_tree(&mut tree, 200.0, 200.0);
    let below_y = tree.get(below).unwrap().bounds.y;
    assert!(
        (below_y - (100.0 - SCROLL_WHEEL_LINE)).abs() < 1.0,
        "child should slide by the offset, y={below_y}"
    );

    paint_tree(&mut renderer, &tree, Color::PAGE);
    let rgba = renderer.rgba8().unwrap();
    assert_eq!(sample(&rgba, 200, 40, 80), banner, "scrolled content");
    assert_eq!(sample(&rgba, 200, 40, 150), page, "still clipped");

    let parked = tree.scroll_by(viewport, 10_000.0).unwrap();
    assert!(
        (parked - 100.0).abs() < 1.0,
        "offset clamps at the end, got {parked}"
    );
    layout_tree(&mut tree, 200.0, 200.0);
    paint_tree(&mut renderer, &tree, Color::PAGE);
    let rgba = renderer.rgba8().unwrap();
    assert_eq!(sample(&rgba, 200, 40, 40), banner);
    assert_eq!(sample(&rgba, 200, 40, 150), page);

    tree.set_scroll_y(viewport, 0.0).unwrap();
    layout_tree(&mut tree, 200.0, 200.0);
    let from_wheel = tree.scroll_at(30.0, 20.0, SCROLL_WHEEL_LINE).unwrap();
    assert!((from_wheel - SCROLL_WHEEL_LINE).abs() < 0.5);
}
