//! One headless check: a scroll container clips a tall child, and a scroll
//! offset (a short wheel step, not a page) changes the pixels.

use tekto::{
    layout_tree, paint_tree, scroll, text, window, Color, CpuRenderer, LayoutStyle, NodeKind,
    Renderer, Tree, SCROLL_WHEEL_LINE,
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

/// An inner scroll wholly outside its outer viewport must paint nothing,
/// including text. Rects already skip an empty clip; glyphs must too.
#[test]
fn nested_disjoint_scroll_viewports_clip_text() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Nested", 200.0, 200.0).unwrap();
    {
        let n = tree.get_mut(root).unwrap();
        n.layout.padding = 0.0;
        n.layout.gap = 0.0;
        n.fill = Some(Color::PAGE);
    }
    let outer = scroll(&mut tree, root, 200.0, 40.0).unwrap();
    let spacer = tree.add(NodeKind::Rect, Some(outer)).unwrap();
    tree.get_mut(spacer).unwrap().layout = LayoutStyle::sized(200.0, 120.0);
    let inner = scroll(&mut tree, outer, 180.0, 48.0).unwrap();
    let label = text(&mut tree, inner, "MMMMMM", 28.0, Color::INK).unwrap();
    let swatch = tree.add(NodeKind::Rect, Some(inner)).unwrap();
    tree.get_mut(swatch).unwrap().fill = Some(Color::BANNER);
    tree.get_mut(swatch).unwrap().layout = LayoutStyle::sized(180.0, 16.0);

    layout_tree(&mut tree, 200.0, 200.0);
    let outer_b = tree.get(outer).unwrap().bounds;
    let inner_b = tree.get(inner).unwrap().bounds;
    let text_b = tree.get(label).unwrap().bounds;
    assert!(
        outer_b.y + outer_b.height <= inner_b.y,
        "inner viewport must sit wholly below the outer one: outer={outer_b:?} inner={inner_b:?}"
    );
    assert!(
        text_b.y + text_b.height <= 200.0 && text_b.width > 8.0 && text_b.height > 8.0,
        "the label must land on the canvas, outside the outer viewport: {text_b:?}"
    );

    // Without the outer clip the same glyphs do paint, so a blank frame
    // later is the clip and not a missing font.
    tree.get_mut(outer).unwrap().kind = NodeKind::Rect;
    let mut renderer = CpuRenderer::new(200, 200).unwrap();
    paint_tree(&mut renderer, &tree, Color::PAGE);
    let open = renderer.rgba8().unwrap();
    let ink = off_page_pixels(&open, 200, 200, text_b);
    assert!(ink > 0, "unclipped label should cover some pixels");
    let swatch_b = tree.get(swatch).unwrap().bounds;
    let banner = [Color::BANNER.r, Color::BANNER.g, Color::BANNER.b];
    assert_eq!(
        sample(&open, 200, swatch_b.x as u32 + 4, swatch_b.y as u32 + 4),
        banner
    );

    tree.get_mut(outer).unwrap().kind = NodeKind::Scroll;
    paint_tree(&mut renderer, &tree, Color::PAGE);
    let clipped = renderer.rgba8().unwrap();
    assert_eq!(
        off_page_pixels(&clipped, 200, 200, text_b),
        0,
        "text inside a disjoint inner scroll must not paint"
    );
    assert_eq!(
        sample(&clipped, 200, swatch_b.x as u32 + 4, swatch_b.y as u32 + 4),
        [Color::PAGE.r, Color::PAGE.g, Color::PAGE.b],
        "a rect in that inner scroll stays clipped too"
    );
}

fn off_page_pixels(rgba: &[u8], width: u32, height: u32, rect: tekto::Rect) -> usize {
    let page = [Color::PAGE.r, Color::PAGE.g, Color::PAGE.b];
    let x0 = rect.x.max(0.0) as u32;
    let y0 = rect.y.max(0.0) as u32;
    let x1 = ((rect.x + rect.width).ceil() as u32).min(width);
    let y1 = ((rect.y + rect.height).ceil() as u32).min(height);
    let mut n = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            if sample(rgba, width, x, y) != page {
                n += 1;
            }
        }
    }
    n
}
