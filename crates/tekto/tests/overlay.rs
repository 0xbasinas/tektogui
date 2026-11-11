use tekto::{
    accesskit_dump, layout_tree, paint_tree, Color, CpuRenderer, NodeKind, OverlaySide, Renderer,
    Tree,
};

#[test]
fn anchored_overlay_paints_and_blocks_background() {
    let mut tree = Tree::new();
    let root = tree.add(NodeKind::Window, None).unwrap();
    let anchor = tree.add(NodeKind::Button, Some(root)).unwrap();
    {
        let n = tree.get_mut(anchor).unwrap();
        n.layout.width = 40.0;
        n.layout.height = 20.0;
        n.fill = Some(Color::INK);
    }
    let overlay = tree.add(NodeKind::Rect, None).unwrap();
    {
        let n = tree.get_mut(overlay).unwrap();
        n.layout.width = 40.0;
        n.layout.height = 20.0;
        n.fill = Some(Color::BANNER);
    }
    tree.set_focus(anchor).unwrap();
    tree.open_overlay(overlay, anchor, OverlaySide::Below)
        .unwrap();
    let later = tree.add(NodeKind::Rect, Some(root)).unwrap();
    {
        let n = tree.get_mut(later).unwrap();
        n.layout.width = 40.0;
        n.layout.height = 20.0;
        n.fill = Some(Color::INK);
    }
    layout_tree(&mut tree, 100.0, 100.0);
    let a = tree.get(anchor).unwrap().bounds;
    let b = tree.get(overlay).unwrap().bounds;
    assert_eq!((b.x, b.y), (a.x, a.y + a.height));
    assert_eq!(tree.hit(b.x + 5.0, b.y + 5.0), Some(overlay));
    assert!(tree.click(anchor).is_err());
    let dump = accesskit_dump(&tree).unwrap();
    assert!(dump.nodes.iter().any(|n| n.id == overlay.raw()));
    let mut renderer = CpuRenderer::new(100, 100).unwrap();
    paint_tree(&mut renderer, &tree, Color::PAGE);
    let pixels = renderer.rgba8().unwrap();
    let i = (((b.y + 5.0) as usize) * 100 + (b.x + 5.0) as usize) * 4;
    assert_eq!(
        &pixels[i..i + 3],
        &[Color::BANNER.r, Color::BANNER.g, Color::BANNER.b]
    );
    assert_eq!(tree.click_at(90.0, 90.0), None);
    assert!(tree.overlay().is_none());
    assert_eq!(tree.focus, Some(anchor));
    assert!(tree.get(overlay).is_none());
    let second = tree.add(NodeKind::Rect, None).unwrap();
    tree.open_overlay(second, anchor, OverlaySide::Right)
        .unwrap();
    assert!(tekto_tree::route_named_key(&mut tree, tekto_tree::NamedActionKey::Escape, false).0);
    assert_eq!(tree.focus, Some(anchor));
}

#[test]
fn overlay_tracks_anchor_and_closes_when_anchor_is_removed() {
    let mut tree = Tree::new();
    let root = tree.add(NodeKind::Window, None).unwrap();
    let anchor = tree.add(NodeKind::Rect, Some(root)).unwrap();
    tree.get_mut(anchor).unwrap().layout.width = 30.0;
    tree.get_mut(anchor).unwrap().layout.height = 20.0;
    let overlay = tree.add(NodeKind::Rect, None).unwrap();
    tree.get_mut(overlay).unwrap().layout.width = 10.0;
    tree.get_mut(overlay).unwrap().layout.height = 8.0;
    tree.open_overlay(overlay, anchor, OverlaySide::Above)
        .unwrap();
    layout_tree(&mut tree, 100.0, 100.0);
    let a = tree.get(anchor).unwrap().bounds;
    let b = tree.get(overlay).unwrap().bounds;
    assert_eq!((b.x, b.y), (a.x, a.y - b.height));
    assert!(tree.append(overlay, root).is_err());
    tree.remove(anchor).unwrap();
    assert!(tree.overlay().is_none());
    assert!(tree.get(overlay).is_none());
}
