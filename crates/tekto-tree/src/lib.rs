//! Scene tree with stable node ids.
//!
//! Ids are monotonic and are never reused. That keeps AccessKit, the
//! agent MCP, and live patches pointed at the same node after a rebuild
//! of only part of the tree.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Stable id of a scene node. Never reused for the life of a [`Tree`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct NodeId(u64);

impl NodeId {
    pub const ROOT_RESERVED: NodeId = NodeId(0);

    pub fn raw(self) -> u64 {
        self.0
    }

    pub fn from_raw(id: u64) -> Self {
        NodeId(id)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What a node is. More kinds will be added as widgets land.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Window,
    Group,
    Rect,
    Text,
    Button,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Window => "window",
            NodeKind::Group => "group",
            NodeKind::Rect => "rect",
            NodeKind::Text => "text",
            NodeKind::Button => "button",
        }
    }
}

/// RGBA color, channels in 0..=255.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// Warm off-white default page background.
    pub const PAGE: Color = Color::rgb(0xf4, 0xf3, 0xf0);
    /// Charcoal for text and dark surfaces.
    pub const INK: Color = Color::rgb(0x2c, 0x33, 0x3a);
    /// Slightly lighter banner fill.
    pub const BANNER: Color = Color::rgb(0x3a, 0x42, 0x4a);
    /// Soft banner text.
    pub const ON_BANNER: Color = Color::rgb(0xf4, 0xf3, 0xf0);

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        let parse = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        match s.len() {
            6 => Some(Color::rgb(parse(0)?, parse(2)?, parse(4)?)),
            8 => Some(Color::rgba(parse(0)?, parse(2)?, parse(4)?, parse(6)?)),
            _ => None,
        }
    }
}

/// Axis-aligned rectangle in logical pixels.
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}

/// Layout hints. [`tekto_layout`] maps these to Taffy.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct LayoutStyle {
    pub width: f32,
    pub height: f32,
    pub padding: f32,
    pub gap: f32,
    pub column: bool,
    /// If true, pack children on the main axis (vertical when `column`).
    #[serde(default)]
    pub justify_center: bool,
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            width: f32::NAN,
            height: f32::NAN,
            padding: 0.0,
            gap: 0.0,
            column: true,
            justify_center: false,
        }
    }
}

impl LayoutStyle {
    pub fn sized(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            ..Self::default()
        }
    }

    pub fn is_width_set(self) -> bool {
        self.width.is_finite()
    }

    pub fn is_height_set(self) -> bool {
        self.height.is_finite()
    }
}

/// A node in the scene tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub name: Option<String>,
    pub text: Option<String>,
    pub fill: Option<Color>,
    pub color: Option<Color>,
    pub font_size: f32,
    pub layout: LayoutStyle,
    pub bounds: Rect,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

impl Node {
    fn new(id: NodeId, kind: NodeKind) -> Self {
        Self {
            id,
            kind,
            name: None,
            text: None,
            fill: None,
            color: None,
            font_size: 16.0,
            layout: LayoutStyle::default(),
            bounds: Rect::default(),
            parent: None,
            children: Vec::new(),
        }
    }

    pub fn display_name(&self) -> String {
        // Prefer visible text so live `ui.patch` of text shows up in the tree.
        if let Some(text) = &self.text {
            return text.clone();
        }
        if let Some(name) = &self.name {
            return name.clone();
        }
        self.kind.as_str().to_string()
    }
}

/// Partial update used by live edits and by MCP `ui.patch`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NodePatch {
    pub text: Option<String>,
    pub name: Option<String>,
    pub fill: Option<String>,
    pub color: Option<String>,
    pub font_size: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct TreeError {
    pub message: String,
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for TreeError {}

/// The scene tree.
#[derive(Clone, Debug)]
pub struct Tree {
    next_id: u64,
    root: Option<NodeId>,
    nodes: HashMap<NodeId, Node>,
    /// Keyboard / AccessKit focus. Defaults to the root once it exists.
    pub focus: Option<NodeId>,
    /// Last node that was clicked, if any.
    pub last_click: Option<NodeId>,
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            root: None,
            nodes: HashMap::new(),
            focus: None,
            last_click: None,
        }
    }

    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    pub fn ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.keys().copied()
    }

    /// Allocate a node and, if `parent` is set, append it as a child.
    pub fn add(&mut self, kind: NodeKind, parent: Option<NodeId>) -> Result<NodeId, TreeError> {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        let mut node = Node::new(id, kind);
        if let Some(pid) = parent {
            let parent_node = self.nodes.get_mut(&pid).ok_or_else(|| TreeError {
                message: format!("unknown parent {pid}"),
            })?;
            parent_node.children.push(id);
            node.parent = Some(pid);
        } else if self.root.is_some() {
            return Err(TreeError {
                message: "tree already has a root".into(),
            });
        } else {
            self.root = Some(id);
            if self.focus.is_none() {
                self.focus = Some(id);
            }
        }
        self.nodes.insert(id, node);
        Ok(id)
    }

    /// Set AccessKit / keyboard focus. The id must already be in the tree.
    pub fn set_focus(&mut self, id: NodeId) -> Result<(), TreeError> {
        if !self.nodes.contains_key(&id) {
            return Err(TreeError {
                message: format!("unknown node {id}"),
            });
        }
        self.focus = Some(id);
        Ok(())
    }

    /// Record a click on a known node: updates `last_click` and focus.
    pub fn click(&mut self, id: NodeId) -> Result<(), TreeError> {
        self.set_focus(id)?;
        self.last_click = Some(id);
        Ok(())
    }

    /// Hit-test `(x, y)` in logical pixels, then [`click`] the node.
    pub fn click_at(&mut self, x: f32, y: f32) -> Option<NodeId> {
        let id = self.hit(x, y)?;
        self.click(id).ok()?;
        Some(id)
    }

    pub fn patch(&mut self, id: NodeId, patch: NodePatch) -> Result<(), TreeError> {
        let node = self.nodes.get_mut(&id).ok_or_else(|| TreeError {
            message: format!("unknown node {id}"),
        })?;
        if let Some(text) = patch.text {
            if node.name.as_deref() == node.text.as_deref() {
                node.name = Some(text.clone());
            }
            node.text = Some(text);
        }
        if let Some(name) = patch.name {
            node.name = Some(name);
        }
        if let Some(fill) = patch.fill {
            node.fill = Some(Color::from_hex(&fill).ok_or_else(|| TreeError {
                message: format!("bad fill color {fill}"),
            })?);
        }
        if let Some(color) = patch.color {
            node.color = Some(Color::from_hex(&color).ok_or_else(|| TreeError {
                message: format!("bad color {color}"),
            })?);
        }
        if let Some(size) = patch.font_size {
            node.font_size = size;
        }
        Ok(())
    }

    /// Hit-test from the front (last child) backwards.
    pub fn hit(&self, x: f32, y: f32) -> Option<NodeId> {
        let root = self.root?;
        self.hit_in(root, x, y)
    }

    fn hit_in(&self, id: NodeId, x: f32, y: f32) -> Option<NodeId> {
        let node = self.nodes.get(&id)?;
        if !node.bounds.contains(x, y) && !matches!(node.kind, NodeKind::Window) {
            return None;
        }
        for child in node.children.iter().rev() {
            if let Some(hit) = self.hit_in(*child, x, y) {
                return Some(hit);
            }
        }
        Some(id)
    }

    /// Pretty tree for agents and `tekto tree`.
    pub fn to_pretty_text(&self) -> String {
        let mut out = String::new();
        if let Some(root) = self.root {
            self.write_pretty(&mut out, root, 0);
        }
        out
    }

    fn write_pretty(&self, out: &mut String, id: NodeId, depth: usize) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        let indent = "  ".repeat(depth);
        let name = node.display_name();
        let b = node.bounds;
        out.push_str(&format!(
            "{indent}- {}#{} \"{}\" [{:.0},{:.0} {:.0}x{:.0}]\n",
            node.kind.as_str(),
            node.id,
            escape_quotes(&name),
            b.x,
            b.y,
            b.width,
            b.height
        ));
        for child in &node.children {
            self.write_pretty(out, *child, depth + 1);
        }
    }

    pub fn to_dump(&self) -> TreeDump {
        let mut nodes = Vec::new();
        if let Some(root) = self.root {
            self.collect_dump(&mut nodes, root);
        }
        TreeDump {
            root: self.root.map(|id| id.raw()),
            focus: self.focus.map(|id| id.raw()),
            last_click: self.last_click.map(|id| id.raw()),
            nodes,
        }
    }

    fn collect_dump(&self, out: &mut Vec<NodeDump>, id: NodeId) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        out.push(NodeDump {
            id: node.id.raw(),
            kind: node.kind,
            name: node.name.clone(),
            text: node.text.clone(),
            fill: node.fill.map(|c| c.to_hex()),
            color: node.color.map(|c| c.to_hex()),
            font_size: node.font_size,
            bounds: node.bounds,
            children: node.children.iter().map(|c| c.raw()).collect(),
        });
        for child in &node.children {
            self.collect_dump(out, *child);
        }
    }
}

fn escape_quotes(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeDump {
    pub root: Option<u64>,
    pub focus: Option<u64>,
    pub last_click: Option<u64>,
    pub nodes: Vec<NodeDump>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeDump {
    pub id: u64,
    pub kind: NodeKind,
    pub name: Option<String>,
    pub text: Option<String>,
    pub fill: Option<String>,
    pub color: Option<String>,
    pub font_size: f32,
    pub bounds: Rect,
    pub children: Vec<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_never_reused() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let a = tree.add(NodeKind::Text, Some(root)).unwrap();
        assert_eq!(root.raw(), 1);
        assert_eq!(a.raw(), 2);
        assert_eq!(tree.get(a).unwrap().kind, NodeKind::Text);
    }

    #[test]
    fn patch_changes_text() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let t = tree.add(NodeKind::Text, Some(root)).unwrap();
        tree.patch(
            t,
            NodePatch {
                text: Some("Hi".into()),
                ..NodePatch::default()
            },
        )
        .unwrap();
        assert_eq!(tree.get(t).unwrap().text.as_deref(), Some("Hi"));
    }

    #[test]
    fn click_at_hits_frontmost_child_and_sets_focus() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        tree.get_mut(root).unwrap().bounds = Rect::new(0.0, 0.0, 200.0, 200.0);
        let back = tree.add(NodeKind::Rect, Some(root)).unwrap();
        tree.get_mut(back).unwrap().bounds = Rect::new(0.0, 0.0, 200.0, 80.0);
        let front = tree.add(NodeKind::Text, Some(root)).unwrap();
        tree.get_mut(front).unwrap().bounds = Rect::new(10.0, 10.0, 40.0, 20.0);
        // Front child is hit first (search last-child first).
        let hit = tree.click_at(20.0, 15.0).expect("hit");
        assert_eq!(hit, front);
        assert_eq!(tree.focus, Some(front));
        assert_eq!(tree.last_click, Some(front));
        // A point only on the back rect.
        let hit = tree.click_at(150.0, 40.0).expect("back");
        assert_eq!(hit, back);
        let dump = tree.to_dump();
        assert_eq!(dump.last_click, Some(back.raw()));
        assert_eq!(dump.focus, Some(back.raw()));
    }
}
