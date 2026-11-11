//! Desktop pointer routing shared by mouse and touch.
//!
//! A touch owns the gesture. Mouse events that copy that finger are ignored
//! until the touch ends or is cancelled, and also until a leftover left-button
//! release. Ending or cancelling the touch clears that swallow so the next
//! real mouse event works even when the backend never sends a synthetic
//! release.
//!
//! A completed undragged tap clicks, on a scroll or off it. The click waits
//! until that end. A cancelled touch does not click.

use tekto_tree::{NodeId, Tree};

/// A few pixels of movement before a press becomes a pan, so a tap still clicks.
const DRAG_SLOP: f32 = 4.0;

/// Phase of one finger. Mapped from winit in the desktop loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FingerPhase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

#[derive(Clone, Debug, Default)]
pub struct PointerSession {
    pub cursor: Option<(f32, f32)>,
    pub pointer_down: bool,
    press_origin: Option<(f32, f32)>,
    drag_scroll: Option<NodeId>,
    dragged: bool,
    pub active_touch: Option<u64>,
    /// While set, cursor motion and left-button presses are dropped.
    pub swallow_mouse: bool,
}

/// What the window loop should do after one pointer event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerOp {
    Ignore,
    Begin {
        x: f32,
        y: f32,
    },
    Drag {
        x: f32,
        y: f32,
    },
    /// `commit` is false when the system cancelled the touch.
    End {
        commit: bool,
    },
}

/// Scroll bookkeeping plus an optional click the caller must deliver.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEffect {
    pub redraw: bool,
    pub click_at: Option<(f32, f32)>,
}

impl PointerEffect {
    fn quiet() -> Self {
        Self {
            redraw: false,
            click_at: None,
        }
    }
}

impl PointerSession {
    pub fn on_cursor_moved(&mut self, x: f32, y: f32) -> PointerOp {
        if self.swallow_mouse {
            return PointerOp::Ignore;
        }
        self.cursor = Some((x, y));
        if self.pointer_down {
            PointerOp::Drag { x, y }
        } else {
            PointerOp::Ignore
        }
    }

    pub fn on_left_button(&mut self, pressed: bool) -> PointerOp {
        if self.swallow_mouse {
            if !pressed {
                self.swallow_mouse = false;
            }
            return PointerOp::Ignore;
        }
        if pressed {
            if let Some((x, y)) = self.cursor {
                PointerOp::Begin { x, y }
            } else {
                PointerOp::Ignore
            }
        } else {
            PointerOp::End { commit: true }
        }
    }

    pub fn on_touch(&mut self, id: u64, phase: FingerPhase, x: f32, y: f32) -> PointerOp {
        self.cursor = Some((x, y));
        match phase {
            FingerPhase::Started => {
                if self.active_touch.is_some() {
                    return PointerOp::Ignore;
                }
                self.active_touch = Some(id);
                self.swallow_mouse = true;
                PointerOp::Begin { x, y }
            }
            FingerPhase::Moved => {
                if self.active_touch == Some(id) {
                    PointerOp::Drag { x, y }
                } else {
                    PointerOp::Ignore
                }
            }
            FingerPhase::Ended => self.finish_touch(id, true),
            FingerPhase::Cancelled => self.finish_touch(id, false),
        }
    }

    fn finish_touch(&mut self, id: u64, commit: bool) -> PointerOp {
        if self.active_touch != Some(id) {
            return PointerOp::Ignore;
        }
        self.active_touch = None;
        self.swallow_mouse = false;
        PointerOp::End { commit }
    }
}

pub fn apply_pointer_op(
    pointer: &mut PointerSession,
    tree: &mut Tree,
    op: PointerOp,
    now_ms: f64,
) -> PointerEffect {
    match op {
        PointerOp::Ignore => PointerEffect::quiet(),
        PointerOp::Begin { x, y } => {
            pointer.pointer_down = true;
            pointer.press_origin = Some((x, y));
            pointer.dragged = false;
            pointer.drag_scroll = tree.scroll_touch_begin(x, y, now_ms);
            // Click only after a completed undragged end. A press that is not
            // on a scroll must not click yet: a later cancel cannot undo it.
            PointerEffect::quiet()
        }
        PointerOp::Drag { x, y } => {
            if let Some((ox, oy)) = pointer.press_origin {
                if (x - ox).abs() + (y - oy).abs() >= DRAG_SLOP {
                    pointer.dragged = true;
                }
            }
            if pointer.drag_scroll.is_none() || !pointer.dragged {
                return PointerEffect::quiet();
            }
            let _ = tree.scroll_touch_move(y, now_ms);
            PointerEffect {
                redraw: true,
                click_at: None,
            }
        }
        PointerOp::End { commit } => {
            let was_down = pointer.pointer_down;
            pointer.pointer_down = false;
            pointer.drag_scroll = None;
            pointer.press_origin = None;
            let flung = if commit {
                tree.scroll_touch_end(now_ms)
            } else {
                tree.scroll_touch_cancel();
                false
            };
            let click_at = if commit && was_down && !pointer.dragged {
                pointer.cursor
            } else {
                None
            };
            pointer.dragged = false;
            PointerEffect {
                redraw: flung,
                click_at,
            }
        }
    }
}

/// Apply a gesture and deliver a completed click through the live action gate.
pub fn deliver_pointer(
    handle: &tekto_mcp::SharedTreeHandle,
    pointer: &mut PointerSession,
    op: PointerOp,
    now_ms: f64,
) -> Result<bool, tekto_mcp::McpError> {
    handle.route_action(|tree| {
        let effect = apply_pointer_op(pointer, tree, op, now_ms);
        (
            effect.redraw,
            effect.click_at.and_then(|(x, y)| tree.hit(x, y)),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tekto_tree::{NodeKind, Rect, Tree};

    fn scroll_with_button() -> (Tree, NodeId) {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().bounds = Rect::new(0.0, 0.0, 200.0, 400.0);
        let scroll = tree.add(NodeKind::Scroll, Some(root)).unwrap();
        let node = tree.get_mut(scroll).unwrap();
        node.bounds = Rect::new(0.0, 0.0, 200.0, 120.0);
        node.scroll_extent = 400.0;
        let button = tree.add(NodeKind::Button, Some(scroll)).unwrap();
        tree.get_mut(button).unwrap().bounds = Rect::new(10.0, 20.0, 80.0, 32.0);
        (tree, button)
    }

    fn drive(pointer: &mut PointerSession, tree: &mut Tree, op: PointerOp, now_ms: f64) -> bool {
        let effect = apply_pointer_op(pointer, tree, op, now_ms);
        if let Some((x, y)) = effect.click_at {
            let _ = tree.click_at(x, y);
            return true;
        }
        false
    }

    #[test]
    fn touch_end_and_cancel_release_the_mouse() {
        let (mut tree, button) = scroll_with_button();
        let mut pointer = PointerSession::default();

        let start = pointer.on_touch(7, FingerPhase::Started, 20.0, 30.0);
        assert!(pointer.swallow_mouse);
        assert!(!drive(&mut pointer, &mut tree, start, 0.0));

        let moved = pointer.on_cursor_moved(40.0, 40.0);
        assert_eq!(moved, PointerOp::Ignore);
        assert_eq!(pointer.cursor, Some((20.0, 30.0)));
        let press = pointer.on_left_button(true);
        assert_eq!(press, PointerOp::Ignore);
        assert!(pointer.swallow_mouse);

        let end = pointer.on_touch(7, FingerPhase::Ended, 20.0, 30.0);
        assert!(!pointer.swallow_mouse);
        assert!(pointer.active_touch.is_none());
        assert!(drive(&mut pointer, &mut tree, end, 80.0));
        assert_eq!(tree.last_click, Some(button));

        tree.last_click = None;
        let hover = pointer.on_cursor_moved(24.0, 28.0);
        assert_eq!(hover, PointerOp::Ignore);
        assert_eq!(pointer.cursor, Some((24.0, 28.0)));
        let again = pointer.on_left_button(true);
        assert!(matches!(again, PointerOp::Begin { .. }));
        assert!(!drive(&mut pointer, &mut tree, again, 100.0));
        let release = pointer.on_left_button(false);
        assert!(drive(&mut pointer, &mut tree, release, 120.0));
        assert_eq!(tree.last_click, Some(button));
    }

    #[test]
    fn cancelled_touch_does_not_click_and_frees_the_mouse() {
        let (mut tree, button) = scroll_with_button();
        let mut pointer = PointerSession::default();
        let start = pointer.on_touch(3, FingerPhase::Started, 20.0, 30.0);
        assert!(!drive(&mut pointer, &mut tree, start, 0.0));
        let cancel = pointer.on_touch(3, FingerPhase::Cancelled, 22.0, 31.0);
        assert!(!pointer.swallow_mouse);
        assert!(!drive(&mut pointer, &mut tree, cancel, 40.0));
        assert_eq!(tree.last_click, None);
        assert!(!pointer.pointer_down);

        let hover = pointer.on_cursor_moved(20.0, 30.0);
        assert_eq!(hover, PointerOp::Ignore);
        let press = pointer.on_left_button(true);
        assert!(matches!(press, PointerOp::Begin { .. }));
        assert!(!drive(&mut pointer, &mut tree, press, 50.0));
        let release = pointer.on_left_button(false);
        assert!(drive(&mut pointer, &mut tree, release, 70.0));
        assert_eq!(tree.last_click, Some(button));
    }

    /// Same delivery as `deliver_pointer`: a `click_at` on the effect is a
    /// click, and it happens in this call, not later.
    #[test]
    fn cancelled_touch_off_scroll_does_not_click() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().bounds = Rect::new(0.0, 0.0, 200.0, 400.0);
        let button = tree.add(NodeKind::Button, Some(root)).unwrap();
        tree.get_mut(button).unwrap().bounds = Rect::new(10.0, 20.0, 80.0, 32.0);
        assert!(tree.scroll_target_at(20.0, 30.0).is_none());

        let mut pointer = PointerSession::default();
        let start = pointer.on_touch(9, FingerPhase::Started, 20.0, 30.0);
        let began = apply_pointer_op(&mut pointer, &mut tree, start, 0.0);
        assert_eq!(began.click_at, None);
        assert_eq!(tree.last_click, None);

        let cancel = pointer.on_touch(9, FingerPhase::Cancelled, 21.0, 30.0);
        assert!(!pointer.swallow_mouse);
        let cancelled = apply_pointer_op(&mut pointer, &mut tree, cancel, 30.0);
        assert_eq!(cancelled.click_at, None);
        assert_eq!(tree.last_click, None);
        assert!(!pointer.pointer_down);

        let start = pointer.on_touch(9, FingerPhase::Started, 20.0, 30.0);
        assert!(!drive(&mut pointer, &mut tree, start, 40.0));
        let end = pointer.on_touch(9, FingerPhase::Ended, 20.0, 30.0);
        assert!(drive(&mut pointer, &mut tree, end, 80.0));
        assert_eq!(tree.last_click, Some(button));

        tree.last_click = None;
        let start = pointer.on_touch(9, FingerPhase::Started, 20.0, 30.0);
        assert!(!drive(&mut pointer, &mut tree, start, 100.0));
        let drag = pointer.on_touch(9, FingerPhase::Moved, 40.0, 30.0);
        assert!(!drive(&mut pointer, &mut tree, drag, 120.0));
        let end = pointer.on_touch(9, FingerPhase::Ended, 40.0, 30.0);
        assert!(!drive(&mut pointer, &mut tree, end, 140.0));
        assert_eq!(tree.last_click, None);
    }

    #[test]
    fn a_touch_drag_tracks_and_does_not_click() {
        let (mut tree, _) = scroll_with_button();
        let scroll = tree.scroll_target_at(20.0, 30.0).unwrap();
        tree.set_scroll_y(scroll, 40.0).unwrap();
        let mut pointer = PointerSession::default();
        let start = pointer.on_touch(1, FingerPhase::Started, 20.0, 80.0);
        assert!(!drive(&mut pointer, &mut tree, start, 0.0));
        let drag = pointer.on_touch(1, FingerPhase::Moved, 20.0, 20.0);
        assert!(!drive(&mut pointer, &mut tree, drag, 16.0));
        let y = tree.get(scroll).unwrap().scroll_y;
        assert!(
            (y - 100.0).abs() < 0.1,
            "finger up raises scroll_y 1:1, got {y}"
        );
        let end = pointer.on_touch(1, FingerPhase::Ended, 20.0, 20.0);
        assert!(!pointer.swallow_mouse);
        assert!(!drive(&mut pointer, &mut tree, end, 400.0));
        assert_eq!(tree.last_click, None);
    }
}
