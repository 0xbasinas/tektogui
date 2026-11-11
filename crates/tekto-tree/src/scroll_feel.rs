//! Touch scroll: 1:1 tracking, a short fling, and a stop that yields to the finger.
//!
//! There is no rubber-band and no decorative bounce. The offset stays inside
//! the content from the last layout. A new press on that scroll cancels the
//! fling before the next move, so inertia does not fight the finger.
//!
//! [`Tree::advance`] integrates the fling on the same clock as style tweens.
//! Reduced motion keeps tracking and skips the fling.

use crate::{NodeId, Tree, TreeError};

/// Samples used to estimate release speed, in milliseconds.
const VELOCITY_WINDOW_MS: f64 = 80.0;
/// A finger that has rested this long before lift does not fling.
const STILL_MS: f64 = 48.0;
/// Minimum release speed that starts a fling, in logical pixels per millisecond.
const FLING_MIN_PX_PER_MS: f32 = 0.35;
/// Fling ends under this speed.
const FLING_STOP_PX_PER_MS: f32 = 0.05;
/// Exponential decay time constant, in milliseconds.
const FLING_TAU_MS: f32 = 220.0;

#[derive(Clone, Copy, Debug)]
struct Sample {
    t_ms: f64,
    y: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct Gesture {
    id: NodeId,
    last_y: f32,
    last_t_ms: f64,
    samples: Vec<Sample>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Fling {
    id: NodeId,
    /// Change in `scroll_y` per millisecond. Positive reveals content below.
    velocity: f32,
    last_ms: f64,
}

impl Tree {
    pub(crate) fn scroll_motion_active(&self) -> bool {
        !self.flings.is_empty()
    }

    pub(crate) fn stop_fling(&mut self, id: NodeId) {
        self.flings.retain(|fling| fling.id != id);
    }

    pub(crate) fn drop_scroll_motion(&mut self, id: NodeId) {
        self.stop_fling(id);
        if self.gesture.as_ref().is_some_and(|g| g.id == id) {
            self.gesture = None;
        }
    }

    /// Press on the innermost scroll under `(x, y)`.
    ///
    /// Any fling on that scroll stops immediately. Returns the scroll id when
    /// tracking starts.
    pub fn scroll_touch_begin(&mut self, x: f32, y: f32, time_ms: f64) -> Option<NodeId> {
        let id = self.scroll_target_at(x, y)?;
        self.stop_fling(id);
        let time_ms = finite_ms(time_ms);
        self.gesture = Some(Gesture {
            id,
            last_y: y,
            last_t_ms: time_ms,
            samples: vec![Sample { t_ms: time_ms, y }],
        });
        Some(id)
    }

    /// Move the active touch. The content follows the finger one pixel to one.
    ///
    /// Positive pointer motion (down) decreases `scroll_y`.
    pub fn scroll_touch_move(&mut self, y: f32, time_ms: f64) -> Result<f32, TreeError> {
        let Some(gesture) = self.gesture.as_mut() else {
            return Err(TreeError {
                message: "no active scroll touch".into(),
            });
        };
        let id = gesture.id;
        let delta = gesture.last_y - y;
        let time_ms = finite_ms(time_ms);
        gesture.last_y = y;
        gesture.last_t_ms = time_ms;
        gesture.samples.push(Sample { t_ms: time_ms, y });
        let cutoff = time_ms - VELOCITY_WINDOW_MS;
        gesture.samples.retain(|sample| sample.t_ms >= cutoff);
        if delta == 0.0 || !delta.is_finite() {
            return self.scroll_y_of(id);
        }
        self.scroll_by(id, delta)
    }

    /// Lift. A fast recent move starts a fling; a held finger does not.
    ///
    /// Returns true while a fling is running.
    pub fn scroll_touch_end(&mut self, time_ms: f64) -> bool {
        let Some(gesture) = self.gesture.take() else {
            return self.scroll_motion_active();
        };
        if self.reduced_motion {
            return false;
        }
        let time_ms = finite_ms(time_ms);
        if let Some(velocity) = release_velocity(&gesture, time_ms) {
            self.flings.retain(|fling| fling.id != gesture.id);
            self.flings.push(Fling {
                id: gesture.id,
                velocity,
                last_ms: time_ms,
            });
        }
        self.scroll_motion_active()
    }

    /// Cancel the gesture without a fling (touch cancelled).
    pub fn scroll_touch_cancel(&mut self) {
        self.gesture = None;
    }

    pub(crate) fn integrate_flings(&mut self, now_ms: f64) {
        if self.reduced_motion {
            self.flings.clear();
            return;
        }
        if !now_ms.is_finite() {
            return;
        }
        let mut still = Vec::new();
        let pending = std::mem::take(&mut self.flings);
        for mut fling in pending {
            if !self.nodes.contains_key(&fling.id) {
                continue;
            }
            let dt = (now_ms - fling.last_ms).max(0.0) as f32;
            fling.last_ms = now_ms;
            if dt > 0.0 && fling.velocity.is_finite() {
                let tau = FLING_TAU_MS;
                let decay = (-dt / tau).exp();
                let delta = fling.velocity * tau * (1.0 - decay);
                let before = self.nodes.get(&fling.id).map(|n| n.scroll_y).unwrap_or(0.0);
                let after = self
                    .write_scroll(fling.id, before + delta, true)
                    .unwrap_or(before);
                let moved = after - before;
                let hit_edge = delta.abs() > 0.5 && (moved - delta).abs() > 0.5;
                fling.velocity *= decay;
                let slow = fling.velocity.abs() < FLING_STOP_PX_PER_MS;
                if hit_edge || !fling.velocity.is_finite() || slow {
                    continue;
                }
            }
            still.push(fling);
        }
        self.flings = still;
    }

    fn scroll_y_of(&self, id: NodeId) -> Result<f32, TreeError> {
        self.nodes
            .get(&id)
            .map(|node| node.scroll_y)
            .ok_or_else(|| TreeError {
                message: format!("unknown node {id}"),
            })
    }
}

fn finite_ms(time_ms: f64) -> f64 {
    if time_ms.is_finite() {
        time_ms
    } else {
        0.0
    }
}

fn release_velocity(gesture: &Gesture, time_ms: f64) -> Option<f32> {
    if time_ms - gesture.last_t_ms > STILL_MS {
        return None;
    }
    let samples = &gesture.samples;
    let last = samples.last()?;
    let first = samples
        .iter()
        .find(|sample| last.t_ms - sample.t_ms <= VELOCITY_WINDOW_MS)?;
    let dt = (last.t_ms - first.t_ms) as f32;
    if dt < 8.0 {
        return None;
    }
    // Pointer moving up increases scroll_y.
    let velocity = (first.y - last.y) / dt;
    if velocity.is_finite() && velocity.abs() >= FLING_MIN_PX_PER_MS {
        Some(velocity)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeKind, Rect, Tree};

    fn tall_scroll() -> (Tree, NodeId) {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().bounds = Rect::new(0.0, 0.0, 200.0, 400.0);
        let scroll = tree.add(NodeKind::Scroll, Some(root)).unwrap();
        let node = tree.get_mut(scroll).unwrap();
        node.bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
        node.scroll_extent = 800.0;
        (tree, scroll)
    }

    #[test]
    fn finger_tracks_one_to_one_and_a_held_lift_does_not_fling() {
        let (mut tree, scroll) = tall_scroll();
        tree.scroll_touch_begin(10.0, 40.0, 0.0).unwrap();
        let y = tree.scroll_touch_move(10.0, 30.0).unwrap();
        assert!(
            (y - 30.0).abs() < 0.1,
            "upward finger raises scroll_y, got {y}"
        );
        let y = tree.scroll_touch_move(40.0, 400.0).unwrap();
        assert!(
            (y - 0.0).abs() < 0.1,
            "downward finger pulls content back, got {y}"
        );
        assert!(!tree.scroll_touch_end(900.0));
        let parked = tree.get(scroll).unwrap().scroll_y;
        assert!(!tree.advance(1200.0));
        assert!((tree.get(scroll).unwrap().scroll_y - parked).abs() < 0.1);
    }

    #[test]
    fn fling_coasts_then_stops_at_the_edge_without_overshoot() {
        let (mut tree, scroll) = tall_scroll();
        tree.set_scroll_y(scroll, 100.0).unwrap();
        tree.scroll_touch_begin(10.0, 80.0, 1_000.0).unwrap();
        tree.scroll_touch_move(40.0, 1_016.0).unwrap();
        tree.scroll_touch_move(0.0, 1_032.0).unwrap();
        assert!(tree.scroll_touch_end(1_040.0));
        let at_release = tree.get(scroll).unwrap().scroll_y;
        assert!(tree.advance(1_040.0));
        assert!(tree.advance(1_160.0));
        let mid = tree.get(scroll).unwrap().scroll_y;
        assert!(
            mid > at_release + 5.0,
            "fling should keep going, release {at_release} mid {mid}"
        );
        assert!(!tree.advance(8_000.0));
        let end = tree.get(scroll).unwrap().scroll_y;
        let max = 700.0;
        assert!(end <= max + 0.1, "no overshoot past {max}, got {end}");
        assert!(end + 0.1 >= mid, "coast does not reverse");
    }

    #[test]
    fn a_new_press_stops_the_fling() {
        let (mut tree, scroll) = tall_scroll();
        tree.scroll_touch_begin(10.0, 90.0, 0.0).unwrap();
        tree.scroll_touch_move(50.0, 16.0).unwrap();
        tree.scroll_touch_move(10.0, 32.0).unwrap();
        assert!(tree.scroll_touch_end(40.0));
        assert!(tree.advance(40.0));
        assert!(tree.advance(80.0));
        let stopped = tree.get(scroll).unwrap().scroll_y;
        tree.scroll_touch_begin(10.0, 40.0, 90.0).unwrap();
        assert!(!tree.advance(400.0));
        let held = tree.get(scroll).unwrap().scroll_y;
        assert!(
            (held - stopped).abs() < 0.1,
            "finger down must not be carried by the old fling, {stopped} -> {held}"
        );
    }
}
