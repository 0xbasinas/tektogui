//! Recorded safe-area and IME insets.
//!
//! The Android shell fills these from `WindowInsets` (physical pixels, then
//! divided by the window scale). Tests inject logical pixels and never need
//! a device. Layout pads the viewport by the safe area. The IME uses the
//! larger of the safe bottom and the keyboard bottom, because the keyboard
//! covers the navigation bar. A focused text field that still sits under
//! that bottom edge is scrolled into the remaining area when a scroll
//! parent can move it. There is no extra animation.

use tekto_layout::layout_tree;
use tekto_tree::{NodeId, NodeKind, Tree};

/// One edge of the window, in logical pixels. Negative values are treated as 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl EdgeInsets {
    pub fn is_zero(self) -> bool {
        self.left <= 0.0 && self.top <= 0.0 && self.right <= 0.0 && self.bottom <= 0.0
    }

    fn non_negative(self) -> Self {
        Self {
            left: self.left.max(0.0),
            top: self.top.max(0.0),
            right: self.right.max(0.0),
            bottom: self.bottom.max(0.0),
        }
    }
}

/// Safe area (system bars, notch, display cutout) plus the soft keyboard.
///
/// Values are logical pixels. The device path converts physical `WindowInsets`
/// with [`insets_from_physical`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowInsets {
    pub safe: EdgeInsets,
    pub ime: EdgeInsets,
}

impl WindowInsets {
    /// Padding that keeps content out of the safe area and out of the IME.
    ///
    /// The bottom edge is the larger of the safe bottom and the IME bottom.
    /// Adding both would leave a gap, because the keyboard covers the nav bar.
    pub fn content_padding(self) -> EdgeInsets {
        let safe = self.safe.non_negative();
        let ime = self.ime.non_negative();
        EdgeInsets {
            left: safe.left,
            top: safe.top,
            right: safe.right,
            bottom: safe.bottom.max(ime.bottom),
        }
    }
}

/// Eight physical pixels from `FieldOverlay.insets`: safe LTRB, then IME LTRB.
pub fn insets_from_physical(px: [i32; 8], scale: f32) -> WindowInsets {
    let scale = if scale >= 0.01 { scale } else { 1.0 };
    let logical = |index: usize| (px[index].max(0) as f32) / scale;
    WindowInsets {
        safe: EdgeInsets {
            left: logical(0),
            top: logical(1),
            right: logical(2),
            bottom: logical(3),
        },
        ime: EdgeInsets {
            left: logical(4),
            top: logical(5),
            right: logical(6),
            bottom: logical(7),
        },
    }
}

/// Lay out `tree` in `width` × `height` logical pixels, inside `insets`.
///
/// A zero inset is the same call as [`layout_tree`]. Otherwise the root is
/// laid out in the padded box and then shifted so it does not sit under the
/// system bars or the keyboard. If the focused text field is still below the
/// visible bottom, the nearest scroll parent moves just enough to show it.
pub fn layout_with_insets(tree: &mut Tree, width: f32, height: f32, insets: WindowInsets) {
    let pad = insets.content_padding();
    if pad.is_zero() {
        layout_tree(tree, width, height);
        return;
    }
    layout_padded(tree, width, height, pad);
    // A scroll box can be taller than the band above the keyboard. Shrink
    // that viewport for this frame so `scroll_y` has room to move, then put
    // the style height back. The next frame starts from the app's height.
    let saved = clamp_focused_scroll(tree, width, height, pad);
    if reveal_focused_field(tree, height, pad) {
        layout_padded(tree, width, height, pad);
    }
    for (id, height) in saved {
        if let Some(node) = tree.get_mut(id) {
            node.layout.height = height;
        }
    }
}

/// If the focused field's scroll parent hangs below the visible bottom, give
/// it a viewport no taller than the visible band and lay out again.
fn clamp_focused_scroll(
    tree: &mut Tree,
    width: f32,
    height: f32,
    pad: EdgeInsets,
) -> Vec<(NodeId, f32)> {
    let Some(focus) = tree.focus else {
        return Vec::new();
    };
    if tree
        .get(focus)
        .is_none_or(|node| node.kind != NodeKind::TextField)
    {
        return Vec::new();
    }
    let Some(scroll_id) = scroll_ancestor(tree, focus) else {
        return Vec::new();
    };
    let Some(scroll) = tree.get(scroll_id) else {
        return Vec::new();
    };
    let visible_bottom = height - pad.bottom;
    let view_top = scroll.bounds.y.max(pad.top);
    let available = (visible_bottom - view_top).max(1.0);
    if scroll.bounds.height <= available + 0.5 {
        return Vec::new();
    }
    let previous = scroll.layout.height;
    if let Some(node) = tree.get_mut(scroll_id) {
        node.layout.height = available;
    }
    layout_padded(tree, width, height, pad);
    vec![(scroll_id, previous)]
}

fn layout_padded(tree: &mut Tree, width: f32, height: f32, pad: EdgeInsets) {
    let inner_w = (width - pad.left - pad.right).max(0.0);
    let inner_h = (height - pad.top - pad.bottom).max(0.0);
    layout_tree(tree, inner_w, inner_h);
    if let Some(root) = tree.root() {
        offset_bounds(tree, root, pad.left, pad.top);
    }
}

fn offset_bounds(tree: &mut Tree, id: NodeId, dx: f32, dy: f32) {
    if let Some(node) = tree.get_mut(id) {
        node.bounds.x += dx;
        node.bounds.y += dy;
    }
    let children = tree
        .get(id)
        .map(|node| node.children.clone())
        .unwrap_or_default();
    for child in children {
        offset_bounds(tree, child, dx, dy);
    }
}

/// Scroll the focused text field into the band above the bottom inset.
/// Returns true when `scroll_y` changed and layout must run again.
fn reveal_focused_field(tree: &mut Tree, window_height: f32, pad: EdgeInsets) -> bool {
    let Some(focus) = tree.focus else {
        return false;
    };
    let Some(node) = tree.get(focus) else {
        return false;
    };
    if node.kind != NodeKind::TextField {
        return false;
    }
    let field_top = node.bounds.y;
    let field_bottom = node.bounds.y + node.bounds.height;
    let field_h = node.bounds.height;
    let visible_top = pad.top;
    let visible_bottom = window_height - pad.bottom;
    let Some(scroll_id) = scroll_ancestor(tree, focus) else {
        return false;
    };
    let Some(scroll) = tree.get(scroll_id) else {
        return false;
    };
    let view_top = scroll.bounds.y.max(visible_top);
    let view_bottom = (scroll.bounds.y + scroll.bounds.height).min(visible_bottom);
    if view_bottom <= view_top {
        return false;
    }
    let mut dy = 0.0;
    if field_h >= view_bottom - view_top {
        dy = field_top - view_top;
    } else {
        if field_bottom > view_bottom {
            dy += field_bottom - view_bottom;
        }
        if field_top - dy < view_top {
            dy -= view_top - (field_top - dy);
        }
    }
    if dy.abs() < 0.5 {
        return false;
    }
    let before = scroll.scroll_y;
    if tree.scroll_by(scroll_id, dy).is_err() {
        return false;
    }
    let after = tree
        .get(scroll_id)
        .map(|node| node.scroll_y)
        .unwrap_or(before);
    (after - before).abs() >= 0.5
}

fn scroll_ancestor(tree: &Tree, id: NodeId) -> Option<NodeId> {
    let mut current = tree.get(id).and_then(|node| node.parent);
    while let Some(id) = current {
        if tree
            .get(id)
            .is_some_and(|node| node.kind == NodeKind::Scroll)
        {
            return Some(id);
        }
        current = tree.get(id).and_then(|node| node.parent);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tekto_tree::{LayoutStyle, NodeKind, Tree};

    /// API 24–29 bottom pair from `getSystemWindowInsetBottom` and
    /// `getStableInsetBottom`. `FieldOverlay.insets` uses this same split.
    ///
    /// `system_bottom` is the full overlap from the window bottom. While the
    /// keyboard is open that overlap is taller than `stable_bottom` and
    /// includes the keyboard plus any system bar already inside it. The
    /// returned IME bottom is that full overlap, or 0 when the system inset
    /// is only the stable bar. Safe bottom stays the smaller of the two.
    /// [`WindowInsets::content_padding`] takes the max, so a full-from-bottom
    /// IME is not added to the nav bar. API 30+ passes `getInsets` through.
    fn pre30_bottom_insets(system_bottom: i32, stable_bottom: i32) -> (i32, i32) {
        let system_bottom = system_bottom.max(0);
        let stable_bottom = stable_bottom.max(0);
        let safe_bottom = system_bottom.min(stable_bottom);
        let ime_bottom = if system_bottom > stable_bottom {
            system_bottom
        } else {
            0
        };
        (safe_bottom, ime_bottom)
    }

    fn sized(width: f32, height: f32) -> LayoutStyle {
        LayoutStyle {
            width,
            height,
            column: true,
            ..LayoutStyle::default()
        }
    }

    #[test]
    fn safe_area_insets_pad_content() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().layout = sized(360.0, 640.0);
        let label = tree.add(NodeKind::Text, Some(root)).unwrap();
        tree.get_mut(label).unwrap().text = Some("Title".into());
        tree.get_mut(label).unwrap().font_size = 16.0;

        let insets = WindowInsets {
            safe: EdgeInsets {
                left: 20.0,
                top: 48.0,
                right: 10.0,
                bottom: 24.0,
            },
            ime: EdgeInsets::default(),
        };
        layout_with_insets(&mut tree, 360.0, 640.0, insets);

        let title = tree.get(label).unwrap().bounds;
        assert!(
            title.x >= 20.0 - 0.5,
            "title should clear the left inset, x={}",
            title.x
        );
        assert!(
            title.y >= 48.0 - 0.5,
            "title should clear the top inset, y={}",
            title.y
        );
        let root_bounds = tree.get(root).unwrap().bounds;
        assert!(
            (root_bounds.x - 20.0).abs() < 0.6,
            "root x {}",
            root_bounds.x
        );
        assert!(
            (root_bounds.y - 48.0).abs() < 0.6,
            "root y {}",
            root_bounds.y
        );
        assert!(
            root_bounds.x + root_bounds.width <= 360.0 - 10.0 + 0.6,
            "root right {} width {}",
            root_bounds.x,
            root_bounds.width
        );
        assert!(
            root_bounds.y + root_bounds.height <= 640.0 - 24.0 + 0.6,
            "root bottom {} height {}",
            root_bounds.y,
            root_bounds.height
        );
    }

    #[test]
    fn keyboard_inset_scrolls_focused_field_into_view() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().layout = sized(400.0, 800.0);
        let scroll = tree.add(NodeKind::Scroll, Some(root)).unwrap();
        tree.get_mut(scroll).unwrap().layout = sized(400.0, 800.0);
        let spacer = tree.add(NodeKind::Rect, Some(scroll)).unwrap();
        tree.get_mut(spacer).unwrap().layout = sized(400.0, 700.0);
        let field = tree.add(NodeKind::TextField, Some(scroll)).unwrap();
        tree.get_mut(field).unwrap().layout = sized(400.0, 48.0);
        tree.get_mut(field).unwrap().text = Some("note".into());
        tree.get_mut(field).unwrap().font_size = 16.0;
        tree.set_focus(field).unwrap();

        let insets = WindowInsets {
            safe: EdgeInsets {
                top: 24.0,
                ..EdgeInsets::default()
            },
            ime: EdgeInsets {
                bottom: 420.0,
                ..EdgeInsets::default()
            },
        };
        layout_with_insets(&mut tree, 400.0, 800.0, insets);

        let visible_bottom = 800.0 - 420.0;
        let bounds = tree.get(field).unwrap().bounds;
        assert!(
            bounds.y + bounds.height <= visible_bottom + 1.0,
            "field bottom {} should be above the keyboard at {}",
            bounds.y + bounds.height,
            visible_bottom
        );
        assert!(
            bounds.y + bounds.height > visible_bottom - bounds.height - 2.0,
            "field should stay in the visible band, y={}",
            bounds.y
        );
        assert!(
            tree.get(scroll).unwrap().scroll_y > 1.0,
            "scroll_y {}",
            tree.get(scroll).unwrap().scroll_y
        );
        assert!(
            bounds.y >= 24.0 - 1.0,
            "field should stay under the status inset, y={}",
            bounds.y
        );
    }

    #[test]
    fn pre30_keyboard_overlap_pads_the_full_system_bottom() {
        // system 300, stable 24 used to become ime 276 and pad 276.
        let (safe_bottom, ime_bottom) = pre30_bottom_insets(300, 24);
        assert_eq!(safe_bottom, 24);
        assert_eq!(ime_bottom, 300);
        let insets = insets_from_physical([0, 0, 0, safe_bottom, 0, 0, 0, ime_bottom], 1.0);
        assert_eq!(insets.content_padding().bottom, 300.0);

        let (safe_bottom, ime_bottom) = pre30_bottom_insets(24, 24);
        assert_eq!((safe_bottom, ime_bottom), (24, 0));
        let closed = insets_from_physical([0, 48, 0, safe_bottom, 0, 0, 0, ime_bottom], 1.0);
        assert_eq!(closed.content_padding().bottom, 24.0);
        assert_eq!(closed.content_padding().top, 48.0);

        // Both already measured from the window bottom: max, do not add.
        let both = WindowInsets {
            safe: EdgeInsets {
                bottom: 300.0,
                ..EdgeInsets::default()
            },
            ime: EdgeInsets {
                bottom: 300.0,
                ..EdgeInsets::default()
            },
        };
        assert_eq!(both.content_padding().bottom, 300.0);
    }

    #[test]
    fn physical_insets_convert_with_scale() {
        let insets = insets_from_physical([20, 40, 0, 30, 0, 0, 0, 100], 2.0);
        assert_eq!(insets.safe.left, 10.0);
        assert_eq!(insets.safe.top, 20.0);
        assert_eq!(insets.safe.bottom, 15.0);
        assert_eq!(insets.ime.bottom, 50.0);
        let pad = insets.content_padding();
        assert_eq!(pad.bottom, 50.0);
        assert_eq!(pad.top, 20.0);
    }

    #[test]
    fn zero_insets_match_plain_layout() {
        let mut plain = Tree::new();
        let mut inset = Tree::new();
        for tree in [&mut plain, &mut inset] {
            let root = tree.add(NodeKind::Window, None).unwrap();
            tree.get_mut(root).unwrap().layout = sized(200.0, 100.0);
            let label = tree.add(NodeKind::Text, Some(root)).unwrap();
            tree.get_mut(label).unwrap().text = Some("Hi".into());
            tree.get_mut(label).unwrap().font_size = 16.0;
        }
        layout_tree(&mut plain, 200.0, 100.0);
        layout_with_insets(&mut inset, 200.0, 100.0, WindowInsets::default());
        for id in plain.ids() {
            let a = plain.get(id).unwrap().bounds;
            let b = inset.get(id).unwrap().bounds;
            assert_eq!((a.x, a.y, a.width, a.height), (b.x, b.y, b.width, b.height));
        }
    }
}
