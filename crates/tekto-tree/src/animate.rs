//! Short style transitions. Quiet smoothstep only — no bounce.
//!
//! Call [`Tree::transition`] to start, then [`Tree::advance`] with a
//! millisecond clock the desktop loop or a headless test owns. Layout
//! writes bounds on its own; call `advance` after layout so a running
//! position or size tween is what gets painted.
//!
//! A new tween starts on the first `advance` after it is requested, so a
//! stale clock from an idle gap does not finish it immediately. Later
//! `transition` calls on the same node merge channels. When a position or
//! size channel finishes (including a snap), the target is written into
//! [`crate::LayoutStyle`] so the next layout keeps it.

use crate::{Color, NodeId, Rect, Tree, TreeError};

/// Default length of a style transition, in milliseconds.
pub const DEFAULT_TRANSITION_MS: f32 = 180.0;

/// Curve applied to a transition's progress. Defaults to [`Easing::Smoothstep`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Easing {
    #[default]
    Smoothstep,
    Linear,
    EaseIn,
    EaseOut,
}

impl Easing {
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Easing::Smoothstep => ease_smoothstep(t),
            Easing::Linear => t.clamp(0.0, 1.0),
            Easing::EaseIn => {
                let t = t.clamp(0.0, 1.0);
                t * t
            }
            Easing::EaseOut => {
                let t = t.clamp(0.0, 1.0);
                1.0 - (1.0 - t) * (1.0 - t)
            }
        }
    }
}

/// Props a node can move. Unset fields stay put.
///
/// `duration_ms: None` uses [`DEFAULT_TRANSITION_MS`]. Zero snaps to the end.
/// `repeat` swaps the endpoints and runs again whenever the last channel
/// finishes, which gives a quiet pulse or back-and-forth motion. Reduced
/// motion still snaps once and does not repeat.
#[derive(Clone, Copy, Debug, Default)]
pub struct StyleTransition {
    pub opacity: Option<f32>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// Fill colour, including alpha.
    pub fill: Option<Color>,
    /// Text and glyph colour.
    pub color: Option<Color>,
    pub easing: Option<Easing>,
    pub repeat: bool,
    pub duration_ms: Option<f32>,
}

impl StyleTransition {
    fn any_prop(self) -> bool {
        self.opacity.is_some()
            || self.x.is_some()
            || self.y.is_some()
            || self.width.is_some()
            || self.height.is_some()
            || self.fill.is_some()
            || self.color.is_some()
    }
}

/// Smoothstep. Flat at both ends, no overshoot.
pub fn ease_smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Linear progress, for motion that should not ease at the ends.
pub fn ease_linear(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChannelKind {
    Opacity,
    X,
    Y,
    Width,
    Height,
    Fill,
    Color,
}

#[derive(Clone, Copy, Debug)]
enum ChannelValue {
    Number { from: f32, to: f32 },
    Rgba { from: Color, to: Color },
}

impl ChannelValue {
    fn number(self, eased: f32) -> Option<f32> {
        match self {
            ChannelValue::Number { from, to } => Some(from + (to - from) * eased),
            ChannelValue::Rgba { .. } => None,
        }
    }

    fn color(self, eased: f32) -> Option<Color> {
        match self {
            ChannelValue::Rgba { from, to } => Some(Color::rgba(
                mix_channel(from.r, to.r, eased),
                mix_channel(from.g, to.g, eased),
                mix_channel(from.b, to.b, eased),
                mix_channel(from.a, to.a, eased),
            )),
            ChannelValue::Number { .. } => None,
        }
    }

    fn swap(self) -> Self {
        match self {
            ChannelValue::Number { from, to } => ChannelValue::Number { from: to, to: from },
            ChannelValue::Rgba { from, to } => ChannelValue::Rgba { from: to, to: from },
        }
    }
}

fn mix_channel(from: u8, to: u8, eased: f32) -> u8 {
    let value = f32::from(from) + (f32::from(to) - f32::from(from)) * eased;
    value.round().clamp(0.0, 255.0) as u8
}

#[derive(Clone, Copy, Debug)]
struct Channel {
    kind: ChannelKind,
    value: ChannelValue,
    easing: Easing,
    /// `None` until the first [`Tree::advance`] after this channel was requested.
    start_ms: Option<f64>,
    duration_ms: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveTransition {
    node: NodeId,
    channels: Vec<Channel>,
    repeat: bool,
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
        !self.transitions.is_empty() || self.scroll_motion_active()
    }

    /// Start or merge a transition on `id`. Samples current style as the start.
    ///
    /// Channels named in `spec` replace the same channel on this node. Other
    /// running channels stay. The clock for a new channel is the next
    /// [`Tree::advance`], not the last one, so an idle gap cannot skip the tween.
    pub fn transition(&mut self, id: NodeId, spec: StyleTransition) -> Result<(), TreeError> {
        if !spec.any_prop() {
            return Err(TreeError {
                message: "transition needs opacity, x, y, width, height, fill, or color".into(),
            });
        }
        let node = self.nodes.get(&id).ok_or_else(|| TreeError {
            message: format!("unknown node {id}"),
        })?;
        let bounds = node.bounds;
        let opacity = node.opacity;
        let fill = node.fill;
        let color = node.color;
        let easing = spec.easing.unwrap_or_default();
        let mut channels = Vec::new();
        push_number(
            &mut channels,
            ChannelKind::Opacity,
            spec.opacity.map(|v| v.clamp(0.0, 1.0)),
            opacity,
            easing,
        );
        push_number(&mut channels, ChannelKind::X, spec.x, bounds.x, easing);
        push_number(&mut channels, ChannelKind::Y, spec.y, bounds.y, easing);
        push_number(
            &mut channels,
            ChannelKind::Width,
            spec.width.map(|v| v.max(0.0)),
            bounds.width,
            easing,
        );
        push_number(
            &mut channels,
            ChannelKind::Height,
            spec.height.map(|v| v.max(0.0)),
            bounds.height,
            easing,
        );
        push_color(&mut channels, ChannelKind::Fill, spec.fill, fill, easing);
        push_color(&mut channels, ChannelKind::Color, spec.color, color, easing);
        let duration = spec.duration_ms.unwrap_or(DEFAULT_TRANSITION_MS);
        if self.reduced_motion || !duration.is_finite() || duration <= 0.0 {
            apply_channels(self, id, &channels, 1.0);
            commit_layout_targets(self, id, &channels);
            self.drop_channel_kinds(id, &channels);
            return Ok(());
        }
        let duration_ms = duration as f64;
        for channel in &mut channels {
            channel.duration_ms = duration_ms;
            channel.start_ms = None;
        }
        self.merge_channels(id, channels, spec.repeat);
        Ok(())
    }

    /// Move the clock to `time_ms` and write interpolated style.
    ///
    /// Returns true while any transition is still running. Time may jump
    /// forward. A channel's start is the first advance after it was requested.
    /// A clock earlier than that start holds the start values.
    pub fn advance(&mut self, time_ms: f64) -> bool {
        if time_ms.is_finite() {
            self.time_ms = time_ms;
        }
        if self.reduced_motion {
            self.finish_transitions();
            self.integrate_flings(self.time_ms);
            return false;
        }
        let now = self.time_ms;
        let mut still = Vec::new();
        let mut pending = std::mem::take(&mut self.transitions);
        // Parents first so a child with its own tween overwrites the shift.
        pending.sort_by_key(|anim| node_depth(self, anim.node));
        for mut anim in pending {
            if !self.nodes.contains_key(&anim.node) {
                continue;
            }
            let mut keep = Vec::new();
            let mut done_channels = Vec::new();
            for channel in &mut anim.channels {
                if channel.start_ms.is_none() {
                    channel.start_ms = Some(now);
                }
                let start = channel.start_ms.unwrap_or(now);
                let dur = channel.duration_ms;
                let raw = if dur <= 0.0 || now + f64::EPSILON < start {
                    if now + f64::EPSILON < start {
                        0.0
                    } else {
                        1.0
                    }
                } else {
                    ((now - start) / dur) as f32
                };
                let done = raw >= 1.0;
                apply_channels(
                    self,
                    anim.node,
                    std::slice::from_ref(channel),
                    channel.easing.apply(raw),
                );
                if done {
                    if anim.repeat {
                        // Ping-pong: run the same span back, one cycle per advance.
                        channel.value = channel.value.swap();
                        channel.start_ms = Some(now);
                        keep.push(*channel);
                    } else {
                        done_channels.push(*channel);
                    }
                } else {
                    keep.push(*channel);
                }
            }
            if !done_channels.is_empty() {
                commit_layout_targets(self, anim.node, &done_channels);
            }
            if !keep.is_empty() && self.nodes.contains_key(&anim.node) {
                anim.channels = keep;
                still.push(anim);
            }
        }
        self.transitions = still;
        self.integrate_flings(self.time_ms);
        self.animations_running()
    }

    pub(crate) fn drop_transitions(&mut self, id: NodeId) {
        self.transitions.retain(|t| t.node != id);
    }

    fn finish_transitions(&mut self) {
        let mut pending = std::mem::take(&mut self.transitions);
        pending.sort_by_key(|anim| node_depth(self, anim.node));
        for anim in pending {
            apply_channels(self, anim.node, &anim.channels, 1.0);
            commit_layout_targets(self, anim.node, &anim.channels);
        }
    }

    fn merge_channels(&mut self, id: NodeId, channels: Vec<Channel>, repeat: bool) {
        if channels.is_empty() {
            return;
        }
        if let Some(existing) = self.transitions.iter_mut().find(|t| t.node == id) {
            for channel in channels {
                existing.channels.retain(|old| old.kind != channel.kind);
                existing.channels.push(channel);
            }
            existing.repeat = repeat;
            return;
        }
        self.transitions.push(ActiveTransition {
            node: id,
            channels,
            repeat,
        });
    }

    fn drop_channel_kinds(&mut self, id: NodeId, channels: &[Channel]) {
        if let Some(existing) = self.transitions.iter_mut().find(|t| t.node == id) {
            existing
                .channels
                .retain(|old| !channels.iter().any(|channel| channel.kind == old.kind));
        }
        self.transitions.retain(|t| !t.channels.is_empty());
    }
}

fn push_number(
    out: &mut Vec<Channel>,
    kind: ChannelKind,
    to: Option<f32>,
    from: f32,
    easing: Easing,
) {
    let Some(to) = to else {
        return;
    };
    if !to.is_finite() {
        return;
    }
    let from = if from.is_finite() { from } else { to };
    out.push(Channel {
        kind,
        value: ChannelValue::Number { from, to },
        easing,
        start_ms: None,
        duration_ms: DEFAULT_TRANSITION_MS as f64,
    });
}

fn push_color(
    out: &mut Vec<Channel>,
    kind: ChannelKind,
    to: Option<Color>,
    from: Option<Color>,
    easing: Easing,
) {
    let Some(to) = to else {
        return;
    };
    // A node with no colour yet starts from the target, so the first frame
    // does not flash an invented colour.
    let from = from.unwrap_or(to);
    out.push(Channel {
        kind,
        value: ChannelValue::Rgba { from, to },
        easing,
        start_ms: None,
        duration_ms: DEFAULT_TRANSITION_MS as f64,
    });
}

/// Finished position and size stay in layout so the next layout pass keeps them.
///
/// Persisted descendant positions are absolute. A parent translation has to
/// move those stored coordinates by the same delta, or the next layout puts
/// the child back at the pre-move point.
fn commit_layout_targets(tree: &mut Tree, id: NodeId, channels: &[Channel]) {
    let Some(node) = tree.nodes.get_mut(&id) else {
        return;
    };
    let mut dx = 0.0;
    let mut dy = 0.0;
    for channel in channels {
        let ChannelValue::Number { from, to } = channel.value else {
            continue;
        };
        match channel.kind {
            ChannelKind::Opacity | ChannelKind::Fill | ChannelKind::Color => {}
            ChannelKind::X => {
                node.layout.x = to;
                dx += to - from;
            }
            ChannelKind::Y => {
                node.layout.y = to;
                dy += to - from;
            }
            ChannelKind::Width => node.layout.width = to,
            ChannelKind::Height => node.layout.height = to,
        }
    }
    if dx != 0.0 || dy != 0.0 {
        shift_persisted_descendant_positions(tree, id, dx, dy);
    }
}

fn apply_channels(tree: &mut Tree, id: NodeId, channels: &[Channel], eased: f32) {
    let Some(node) = tree.nodes.get_mut(&id) else {
        return;
    };
    let mut bounds: Rect = node.bounds;
    for channel in channels {
        match channel.kind {
            ChannelKind::Opacity => {
                if let Some(value) = channel.value.number(eased) {
                    node.opacity = value.clamp(0.0, 1.0);
                }
            }
            ChannelKind::X => {
                if let Some(value) = channel.value.number(eased) {
                    bounds.x = value;
                    tree.layout_dirty = true;
                }
            }
            ChannelKind::Y => {
                if let Some(value) = channel.value.number(eased) {
                    bounds.y = value;
                    tree.layout_dirty = true;
                }
            }
            ChannelKind::Width => {
                if let Some(value) = channel.value.number(eased) {
                    bounds.width = value.max(0.0);
                    tree.layout_dirty = true;
                }
            }
            ChannelKind::Height => {
                if let Some(value) = channel.value.number(eased) {
                    bounds.height = value.max(0.0);
                    tree.layout_dirty = true;
                }
            }
            ChannelKind::Fill => node.fill = channel.value.color(eased),
            ChannelKind::Color => node.color = channel.value.color(eased),
        }
    }
    let dx = bounds.x - node.bounds.x;
    let dy = bounds.y - node.bounds.y;
    node.bounds = bounds;
    // Layout writes absolute bounds before this runs. Move the subtree with
    // the node so a child is not left at the pre-animation position.
    shift_descendants(tree, id, dx, dy);
}

fn node_depth(tree: &Tree, mut id: NodeId) -> u32 {
    let mut depth = 0;
    while let Some(parent) = tree.nodes.get(&id).and_then(|node| node.parent) {
        depth += 1;
        id = parent;
        if depth > 10_000 {
            break;
        }
    }
    depth
}

fn shift_descendants(tree: &mut Tree, id: NodeId, dx: f32, dy: f32) {
    if !dx.is_finite() || !dy.is_finite() || (dx == 0.0 && dy == 0.0) {
        return;
    }
    let children = tree
        .nodes
        .get(&id)
        .map(|node| node.children.clone())
        .unwrap_or_default();
    for child in children {
        if let Some(node) = tree.nodes.get_mut(&child) {
            node.bounds.x += dx;
            node.bounds.y += dy;
        }
        shift_descendants(tree, child, dx, dy);
    }
}

fn shift_persisted_descendant_positions(tree: &mut Tree, id: NodeId, dx: f32, dy: f32) {
    if !dx.is_finite() || !dy.is_finite() || (dx == 0.0 && dy == 0.0) {
        return;
    }
    let children = tree
        .nodes
        .get(&id)
        .map(|node| node.children.clone())
        .unwrap_or_default();
    for child in children {
        if let Some(node) = tree.nodes.get_mut(&child) {
            if node.layout.x.is_finite() {
                node.layout.x += dx;
            }
            if node.layout.y.is_finite() {
                node.layout.y += dy;
            }
        }
        shift_persisted_descendant_positions(tree, child, dx, dy);
    }
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

    #[test]
    fn fill_and_text_colour_mix_between_endpoints() {
        let mut tree = Tree::new();
        let id = box_node(&mut tree);
        tree.get_mut(id).unwrap().fill = Some(Color::rgb(200, 0, 0));
        tree.get_mut(id).unwrap().color = Some(Color::rgb(0, 0, 0));
        tree.transition(
            id,
            StyleTransition {
                fill: Some(Color::rgb(0, 0, 200)),
                color: Some(Color::rgb(200, 200, 200)),
                easing: Some(Easing::Linear),
                duration_ms: Some(100.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();
        assert!(tree.advance(0.0));
        assert!(tree.advance(50.0));
        let mid = tree.get(id).unwrap();
        let fill = mid.fill.unwrap();
        assert!(
            (95..=105).contains(&fill.r) && (95..=105).contains(&fill.b),
            "{fill:?}"
        );
        assert_eq!(fill.a, 255);
        let color = mid.color.unwrap();
        assert!((95..=105).contains(&color.r), "{color:?}");
        assert!(!tree.advance(100.0));
        assert_eq!(tree.get(id).unwrap().fill, Some(Color::rgb(0, 0, 200)));
        assert_eq!(tree.get(id).unwrap().color, Some(Color::rgb(200, 200, 200)));
    }

    #[test]
    fn named_easings_are_distinct_and_stay_in_range() {
        assert_eq!(ease_linear(0.25), 0.25);
        assert!((Easing::EaseIn.apply(0.25) - 0.0625).abs() < 0.001);
        assert!((Easing::EaseOut.apply(0.25) - 0.4375).abs() < 0.001);
        for easing in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::Smoothstep,
        ] {
            for step in 0..=10 {
                let value = easing.apply(step as f32 / 10.0);
                assert!((0.0..=1.0).contains(&value), "{value}");
            }
        }
    }

    #[test]
    fn repeat_ping_pongs_and_keeps_running() {
        let mut tree = Tree::new();
        let id = box_node(&mut tree);
        tree.transition(
            id,
            StyleTransition {
                opacity: Some(0.0),
                easing: Some(Easing::Linear),
                repeat: true,
                duration_ms: Some(100.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();
        assert!(tree.advance(0.0));
        assert!(tree.advance(100.0));
        assert!(tree.get(id).unwrap().opacity < 0.01);
        // The endpoint swaps, so the next cycle goes back toward the start.
        assert!(tree.advance(150.0));
        let mid = tree.get(id).unwrap().opacity;
        assert!(mid > 0.3 && mid < 0.7, "{mid}");
        assert!(tree.advance(200.0));
        assert!(tree.get(id).unwrap().opacity > 0.99);
        assert!(tree.animations_running());
    }

    #[test]
    fn repeat_snaps_once_when_motion_is_reduced() {
        let mut tree = Tree::new();
        let id = box_node(&mut tree);
        tree.set_reduced_motion(true);
        tree.transition(
            id,
            StyleTransition {
                opacity: Some(0.2),
                repeat: true,
                duration_ms: Some(100.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();
        assert!((tree.get(id).unwrap().opacity - 0.2).abs() < 0.001);
        assert!(!tree.animations_running());
        assert!(!tree.advance(10_000.0));
        assert!((tree.get(id).unwrap().opacity - 0.2).abs() < 0.001);
    }

    #[test]
    fn transition_after_idle_gap_starts_on_the_next_advance() {
        let mut tree = Tree::new();
        let id = box_node(&mut tree);
        assert!(!tree.advance(0.0));
        assert!(!tree.advance(5_000.0));
        tree.transition(
            id,
            StyleTransition {
                opacity: Some(0.0),
                duration_ms: Some(100.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();
        // The next tick is the start, even though the clock is already past the duration.
        assert!(tree.advance(5_000.0));
        assert!((tree.get(id).unwrap().opacity - 1.0).abs() < 0.01);
        assert!(tree.advance(5_050.0));
        let mid = tree.get(id).unwrap().opacity;
        assert!(mid > 0.3 && mid < 0.7, "{mid}");
        assert!(!tree.advance(5_100.0));
        assert!(tree.get(id).unwrap().opacity < 0.01);
    }

    #[test]
    fn later_transition_merges_channels_on_the_same_node() {
        let mut tree = Tree::new();
        let id = box_node(&mut tree);
        tree.transition(
            id,
            StyleTransition {
                opacity: Some(0.0),
                duration_ms: Some(100.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();
        tree.transition(
            id,
            StyleTransition {
                x: Some(50.0),
                duration_ms: Some(100.0),
                ..StyleTransition::default()
            },
        )
        .unwrap();
        assert!(tree.advance(0.0));
        assert!(tree.advance(50.0));
        let mid = tree.get(id).unwrap();
        assert!(mid.opacity > 0.3 && mid.opacity < 0.7, "{}", mid.opacity);
        assert!(
            mid.bounds.x > 20.0 && mid.bounds.x < 40.0,
            "{}",
            mid.bounds.x
        );
    }
}
