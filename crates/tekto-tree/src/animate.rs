//! Short style transitions. Quiet smoothstep only — no bounce.
//!
//! Call [`Tree::transition`] to start, then [`Tree::advance`] with a
//! millisecond clock the desktop loop or a headless test owns. Layout
//! writes bounds on its own; call `advance` after layout so a running
//! position or size tween is what gets painted.

use crate::{NodeId, Rect, Tree, TreeError};

/// Default length of a style transition, in milliseconds.
pub const DEFAULT_TRANSITION_MS: f32 = 180.0;

/// Props a node can move. Unset fields stay put.
///
/// `duration_ms: None` uses [`DEFAULT_TRANSITION_MS`]. Zero snaps to the end.
#[derive(Clone, Copy, Debug, Default)]
pub struct StyleTransition {
    pub opacity: Option<f32>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub duration_ms: Option<f32>,
}

impl StyleTransition {
    fn any_prop(self) -> bool {
        self.opacity.is_some()
            || self.x.is_some()
            || self.y.is_some()
            || self.width.is_some()
            || self.height.is_some()
    }
}

/// Smoothstep. Flat at both ends, no overshoot.
pub fn ease_smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Copy, Debug)]
enum ChannelKind {
    Opacity,
    X,
    Y,
    Width,
    Height,
}

#[derive(Clone, Copy, Debug)]
struct Channel {
    kind: ChannelKind,
    from: f32,
    to: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveTransition {
    node: NodeId,
    start_ms: f64,
    duration_ms: f64,
    channels: Vec<Channel>,
}

/// `TEKTO_REDUCE_MOTION=1` (also `true`, `yes`, `on`) asks the app to snap.
///
/// AccessKit and the OS "reduce motion" settings are not read. See the
/// architecture doc.
pub fn reduced_motion_requested() -> bool {
    match std::env::var("TEKTO_REDUCE_MOTION") {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

impl Tree {
    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    /// When set, running tweens jump to their targets and new ones snap.
    pub fn set_reduced_motion(&mut self, on: bool) {
        self.reduced_motion = on;
        if on {
            self.finish_transitions();
        }
    }

    /// Clock from the last [`Tree::advance`], in milliseconds.
    pub fn time_ms(&self) -> f64 {
        self.time_ms
    }

    pub fn animations_running(&self) -> bool {
        !self.transitions.is_empty()
    }

    /// Start or replace the transition on `id`. Samples current style as the start.
    pub fn transition(&mut self, id: NodeId, spec: StyleTransition) -> Result<(), TreeError> {
        if !spec.any_prop() {
            return Err(TreeError {
                message: "transition needs opacity, x, y, width, or height".into(),
            });
        }
        let node = self.nodes.get(&id).ok_or_else(|| TreeError {
            message: format!("unknown node {id}"),
        })?;
        let bounds = node.bounds;
        let opacity = node.opacity;
        let mut channels = Vec::new();
        push_channel(
            &mut channels,
            ChannelKind::Opacity,
            spec.opacity.map(|v| v.clamp(0.0, 1.0)),
            opacity,
        );
        push_channel(&mut channels, ChannelKind::X, spec.x, bounds.x);
        push_channel(&mut channels, ChannelKind::Y, spec.y, bounds.y);
        push_channel(
            &mut channels,
            ChannelKind::Width,
            spec.width.map(|v| v.max(0.0)),
            bounds.width,
        );
        push_channel(
            &mut channels,
            ChannelKind::Height,
            spec.height.map(|v| v.max(0.0)),
            bounds.height,
        );
        let duration = spec.duration_ms.unwrap_or(DEFAULT_TRANSITION_MS);
        if self.reduced_motion || !duration.is_finite() || duration <= 0.0 {
            apply_channels(self, id, &channels, 1.0);
            self.transitions.retain(|t| t.node != id);
            return Ok(());
        }
        self.transitions.retain(|t| t.node != id);
        self.transitions.push(ActiveTransition {
            node: id,
            start_ms: self.time_ms,
            duration_ms: duration as f64,
            channels,
        });
        Ok(())
    }

    /// Move the clock to `time_ms` and write interpolated style.
    ///
    /// Returns true while any transition is still running. Time may jump
    /// forward. A clock earlier than a transition's start holds the start values.
    pub fn advance(&mut self, time_ms: f64) -> bool {
        if time_ms.is_finite() {
            self.time_ms = time_ms;
        }
        if self.reduced_motion {
            self.finish_transitions();
            return false;
        }
        let now = self.time_ms;
        let mut still = Vec::new();
        let pending = std::mem::take(&mut self.transitions);
        for anim in pending {
            let dur = anim.duration_ms;
            let raw = if dur <= 0.0 {
                1.0
            } else {
                ((now - anim.start_ms) / dur) as f32
            };
            let done = raw >= 1.0;
            let eased = ease_smoothstep(raw);
            apply_channels(self, anim.node, &anim.channels, eased);
            if !done && self.nodes.contains_key(&anim.node) {
                still.push(anim);
            }
        }
        self.transitions = still;
        !self.transitions.is_empty()
    }

    pub(crate) fn drop_transitions(&mut self, id: NodeId) {
        self.transitions.retain(|t| t.node != id);
    }

    fn finish_transitions(&mut self) {
        let pending = std::mem::take(&mut self.transitions);
        for anim in pending {
            apply_channels(self, anim.node, &anim.channels, 1.0);
        }
    }
}

fn push_channel(out: &mut Vec<Channel>, kind: ChannelKind, to: Option<f32>, from: f32) {
    let Some(to) = to else {
        return;
    };
    if !to.is_finite() {
        return;
    }
    let from = if from.is_finite() { from } else { to };
    out.push(Channel { kind, from, to });
}

fn apply_channels(tree: &mut Tree, id: NodeId, channels: &[Channel], eased: f32) {
    let Some(node) = tree.nodes.get_mut(&id) else {
        return;
    };
    let mix = |from: f32, to: f32| from + (to - from) * eased;
    let mut bounds: Rect = node.bounds;
    for channel in channels {
        match channel.kind {
            ChannelKind::Opacity => {
                node.opacity = mix(channel.from, channel.to).clamp(0.0, 1.0);
            }
            ChannelKind::X => bounds.x = mix(channel.from, channel.to),
            ChannelKind::Y => bounds.y = mix(channel.from, channel.to),
            ChannelKind::Width => bounds.width = mix(channel.from, channel.to).max(0.0),
            ChannelKind::Height => bounds.height = mix(channel.from, channel.to).max(0.0),
        }
    }
    node.bounds = bounds;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeKind, Tree};

    fn box_node(tree: &mut Tree) -> NodeId {
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().bounds = Rect::new(0.0, 0.0, 200.0, 120.0);
        let id = tree.add(NodeKind::Rect, Some(root)).unwrap();
        let node = tree.get_mut(id).unwrap();
        node.bounds = Rect::new(10.0, 20.0, 40.0, 16.0);
        node.opacity = 1.0;
        id
    }

    #[test]
    fn midpoint_is_between_start_and_end() {
        let mut tree = Tree::new();
        let id = box_node(&mut tree);
        tree.transition(
            id,
            StyleTransition {
                opacity: Some(0.0),
                x: Some(50.0),
                width: Some(80.0),
                duration_ms: Some(100.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();
        assert!(tree.advance(0.0));
        let start = tree.get(id).unwrap();
        assert!((start.opacity - 1.0).abs() < 0.01);
        assert!((start.bounds.x - 10.0).abs() < 0.01);

        assert!(tree.advance(50.0));
        let mid = tree.get(id).unwrap();
        assert!(mid.opacity < 0.7 && mid.opacity > 0.3, "{}", mid.opacity);
        assert!(
            mid.bounds.x > 20.0 && mid.bounds.x < 40.0,
            "{}",
            mid.bounds.x
        );
        assert!(
            mid.bounds.width > 50.0 && mid.bounds.width < 70.0,
            "{}",
            mid.bounds.width
        );
        // Untouched axis stays.
        assert!((mid.bounds.y - 20.0).abs() < 0.01);
        assert!((mid.bounds.height - 16.0).abs() < 0.01);

        assert!(!tree.advance(100.0));
        let end = tree.get(id).unwrap();
        assert!(end.opacity < 0.01);
        assert!((end.bounds.x - 50.0).abs() < 0.01);
        assert!((end.bounds.width - 80.0).abs() < 0.01);
        assert!(!tree.animations_running());
    }

    #[test]
    fn reduced_motion_snaps() {
        let mut tree = Tree::new();
        let id = box_node(&mut tree);
        tree.set_reduced_motion(true);
        tree.transition(
            id,
            StyleTransition {
                opacity: Some(0.25),
                y: Some(80.0),
                duration_ms: Some(400.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();
        let node = tree.get(id).unwrap();
        assert!((node.opacity - 0.25).abs() < 0.001);
        assert!((node.bounds.y - 80.0).abs() < 0.01);
        assert!(!tree.animations_running());
    }

    #[test]
    fn smoothstep_does_not_overshoot() {
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            let e = ease_smoothstep(t);
            assert!((0.0..=1.0).contains(&e), "{e}");
        }
        assert!((ease_smoothstep(0.5) - 0.5).abs() < 0.001);
    }
}
