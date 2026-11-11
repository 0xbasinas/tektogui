//! Scene tree with stable node ids.
//!
//! Ids are monotonic and are never reused. That keeps AccessKit, the
//! agent MCP, and live patches pointed at the same node after a rebuild
//! of only part of the tree.

mod animate;
mod recording;
mod scroll_feel;
mod store;
pub use animate::{
    ease_smoothstep, reduced_motion_requested, StyleTransition, DEFAULT_TRANSITION_MS,
};
pub use recording::{Action, RecordedKey, Recording};
pub use store::{StateInfo, Store, StoreError};

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

/// Named keys supported by agent input and desktop routing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedActionKey {
    Tab,
    ShiftTab,
    Enter,
    Space,
    Backspace,
}

impl std::str::FromStr for NamedActionKey {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "Tab" => Ok(Self::Tab),
            "Shift+Tab" => Ok(Self::ShiftTab),
            "Enter" => Ok(Self::Enter),
            "Space" => Ok(Self::Space),
            "Backspace" => Ok(Self::Backspace),
            _ => Err(format!(
                "unknown key {name:?}; expected Tab, Shift+Tab, Enter, Space, or Backspace"
            )),
        }
    }
}

/// Apply a named key and return any button that should be activated.
pub fn route_named_key(
    tree: &mut Tree,
    key: NamedActionKey,
    repeat: bool,
) -> (bool, Option<NodeId>) {
    match key {
        NamedActionKey::Tab => (tree.traverse_focus(false), None),
        NamedActionKey::ShiftTab => (tree.traverse_focus(true), None),
        NamedActionKey::Enter | NamedActionKey::Space => {
            let button = tree
                .focus
                .filter(|id| tree.get(*id).is_some_and(|n| n.kind == NodeKind::Button));
            (false, button.filter(|_| !repeat))
        }
        NamedActionKey::Backspace => (
            tree.apply_field_key(FieldKey::Backspace).unwrap_or(false),
            None,
        ),
    }
}

/// A committed user action on a tree node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeEvent {
    Click(NodeId),
    Changed(NodeId),
}

type EventCallback = dyn Fn(&mut Tree, TreeEvent) + Send + Sync;
#[derive(Clone)]
struct EventHandler(Arc<EventCallback>);
impl fmt::Debug for EventHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EventHandler")
    }
}

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
    /// Single-line editable value. `name` is the label; `text` is the value.
    TextField,
    /// Vertical clip of overflowing children. `scroll_y` shifts them up.
    Scroll,
    /// Vertical collection of [`NodeKind::ListItem`] rows.
    /// Layout keeps only the visible rows plus [`LIST_OVERSCAN_PX`] when the
    /// list sits in a scroll viewport.
    List,
    /// One row in a [`NodeKind::List`]. Height is per row.
    ListItem,
}

/// Logical pixels one mouse-wheel notch moves a scroll container.
/// A notch is a short step, not a page.
pub const SCROLL_WHEEL_LINE: f32 = 48.0;

/// Extra logical pixels laid out above and below a list viewport.
/// Layout keeps this band of rows so a small scroll does not pop the edges.
pub const LIST_OVERSCAN_PX: f32 = 64.0;

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Window => "window",
            NodeKind::Group => "group",
            NodeKind::Rect => "rect",
            NodeKind::Text => "text",
            NodeKind::Button => "button",
            NodeKind::TextField => "text_field",
            NodeKind::Scroll => "scroll",
            NodeKind::List => "list",
            NodeKind::ListItem => "list_item",
        }
    }

    /// Parse a kind name (`window`, `group`, `rect`, `text`, `button`, `text_field`, `scroll`, `list`, `list_item`).
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "window" => Some(Self::Window),
            "group" => Some(Self::Group),
            "rect" => Some(Self::Rect),
            "text" => Some(Self::Text),
            "button" => Some(Self::Button),
            "text_field" => Some(Self::TextField),
            "scroll" => Some(Self::Scroll),
            "list" => Some(Self::List),
            "list_item" => Some(Self::ListItem),
            _ => None,
        }
    }

    /// Every kind. `as_str` is exhaustive, so a new variant fails to compile
    /// until it has a name. This list must include that name: the component
    /// schema test requires `components.json` to match `all()`.
    pub fn all() -> &'static [NodeKind] {
        &[
            NodeKind::Window,
            NodeKind::Group,
            NodeKind::Rect,
            NodeKind::Text,
            NodeKind::Button,
            NodeKind::TextField,
            NodeKind::Scroll,
            NodeKind::List,
            NodeKind::ListItem,
        ]
    }

    /// Host actions an agent can use on this kind.
    ///
    /// `click` (`ui.click`) focuses any node. `type`, `key`, and `ime` apply
    /// only to a text field. `scroll` applies only to a scroll container.
    pub fn agent_events(self) -> &'static [&'static str] {
        match self {
            NodeKind::TextField => &["click", "type", "key", "ime"],
            NodeKind::Scroll => &["click", "scroll"],
            NodeKind::Window
            | NodeKind::Group
            | NodeKind::Rect
            | NodeKind::Text
            | NodeKind::Button
            | NodeKind::List
            | NodeKind::ListItem => &["click"],
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
    /// Quiet inset behind a single-line field.
    pub const FIELD: Color = Color::rgb(0xe4, 0xe2, 0xdc);

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
    /// When finite, layout keeps this x after a position transition.
    /// `NaN` means the flex position.
    #[serde(default = "nan_f32", skip_serializing_if = "is_nan_f32")]
    pub x: f32,
    /// When finite, layout keeps this y after a position transition.
    #[serde(default = "nan_f32", skip_serializing_if = "is_nan_f32")]
    pub y: f32,
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
            x: f32::NAN,
            y: f32::NAN,
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

/// Default window padding.
pub const PAGE_PADDING: f32 = 24.0;
/// Default space between window children.
pub const STACK_GAP: f32 = 20.0;
/// Default control text size.
pub const BODY_SIZE: f32 = 20.0;

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
    /// Character index in the committed `text` (Unicode scalar values).
    /// Used by text fields. A preedit string, if any, is inserted here for display.
    #[serde(default)]
    pub caret: usize,
    /// Caret x inside the content box, in logical pixels. Layout fills this in.
    #[serde(default)]
    pub caret_x: f32,
    /// Uncommitted IME composition, inserted at `caret` for display and AccessKit.
    /// Empty when the field is not composing. Not part of the committed value.
    #[serde(default)]
    pub preedit: String,
    /// Byte range inside `preedit` (winit). `None` hides the caret while composing.
    #[serde(default)]
    pub preedit_cursor: Option<(usize, usize)>,
    /// Vertical offset of a [`NodeKind::Scroll`] container, in logical pixels.
    /// Zero shows the top of the content. Layout clamps this to the content.
    #[serde(default)]
    pub scroll_y: f32,
    /// Distance from the scroll box top to the bottom of its content, before
    /// `scroll_y` is applied. Layout writes this. Zero until the first layout.
    #[serde(default)]
    pub scroll_extent: f32,
    /// Viewport height, in logical pixels, for a [`NodeKind::List`] that should
    /// scroll once its rows are taller than this. `NaN` means there is no
    /// viewport (the list stays a direct child). The list widget sets this and
    /// reads it when deciding whether to insert a scroll parent.
    #[serde(default = "nan_f32", skip_serializing_if = "is_nan_f32")]
    pub list_viewport: f32,
    /// Every list row in order, including rows layout has detached.
    /// Empty until the first layout of a list. Not an accessibility child list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_slots: Vec<NodeId>,
    /// Spacer above the virtual window. `0` means there is no spacer yet.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub list_pad_top: u64,
    /// Spacer below the virtual window. `0` means there is no spacer yet.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub list_pad_bottom: u64,
    /// Paint multiplier for this node and its children. 1 is solid, 0 is hidden.
    #[serde(default = "default_opacity")]
    pub opacity: f32,
}

/// A detached row or spacer still belongs to `list_id`. A row whose parent
/// lives outside the removed subtree does not.
fn claim_with_list(
    nodes: &HashMap<NodeId, Node>,
    queued: &HashSet<NodeId>,
    list_id: NodeId,
    id: NodeId,
) -> bool {
    if id.raw() == 0 || !nodes.contains_key(&id) {
        return false;
    }
    match nodes[&id].parent {
        None => true,
        Some(parent) => parent == list_id || queued.contains(&parent),
    }
}

fn default_opacity() -> f32 {
    1.0
}

fn nan_f32() -> f32 {
    f32::NAN
}

fn is_nan_f32(value: &f32) -> bool {
    value.is_nan()
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
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
            caret: 0,
            caret_x: 0.0,
            preedit: String::new(),
            preedit_cursor: None,
            scroll_y: 0.0,
            scroll_extent: 0.0,
            list_viewport: f32::NAN,
            list_slots: Vec::new(),
            list_pad_top: 0,
            list_pad_bottom: 0,
            opacity: 1.0,
        }
    }

    /// Apply the built-in defaults for a window or control before setting props.
    pub fn apply_widget_defaults(&mut self) {
        match self.kind {
            NodeKind::Window => {
                self.fill = Some(Color::PAGE);
                self.layout.padding = PAGE_PADDING;
                self.layout.gap = STACK_GAP;
                self.layout.column = true;
            }
            NodeKind::Button => {
                self.fill = Some(Color::BANNER);
                self.color = Some(Color::ON_BANNER);
                self.font_size = BODY_SIZE;
                self.layout.padding = 12.0;
            }
            NodeKind::TextField => {
                self.fill = Some(Color::FIELD);
                self.color = Some(Color::INK);
                self.font_size = BODY_SIZE;
                self.layout.padding = 10.0;
            }
            _ => {}
        }
    }

    pub fn display_name(&self) -> String {
        // Prefer visible text so live `ui.patch` of text shows up in the tree.
        // An empty text field keeps its label (`name`) so the tree stays readable.
        if self.kind == NodeKind::TextField {
            let shown = self.composed_text();
            if !shown.is_empty() {
                return shown;
            }
        } else if let Some(text) = &self.text {
            return text.clone();
        }
        if let Some(name) = &self.name {
            return name.clone();
        }
        self.kind.as_str().to_string()
    }

    /// Committed value with the IME preedit inserted at the caret.
    ///
    /// Non-fields return `text`. An empty text field with no preedit returns `""`.
    pub fn composed_text(&self) -> String {
        if self.kind != NodeKind::TextField || self.preedit.is_empty() {
            return self.text.clone().unwrap_or_default();
        }
        let committed = self.text.clone().unwrap_or_default();
        let mut chars: Vec<char> = committed.chars().collect();
        let caret = self.caret.min(chars.len());
        let mut out: String = chars.drain(..caret).collect();
        out.push_str(&self.preedit);
        out.extend(chars);
        out
    }

    /// Caret in [`composed_text`], in Unicode scalar values.
    ///
    /// `None` means the input method asked to hide the caret (winit preedit cursor).
    pub fn composed_caret(&self) -> Option<usize> {
        let committed_len = self.text.as_deref().unwrap_or("").chars().count();
        let caret = self.caret.min(committed_len);
        if self.kind != NodeKind::TextField || self.preedit.is_empty() {
            return Some(caret);
        }
        let (_, end) = self.preedit_cursor?;
        let end = clamp_char_boundary(&self.preedit, end);
        Some(caret + self.preedit[..end].chars().count())
    }
}

fn clamp_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut index = index;
    while index > 0 && !s.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// One desktop key applied to a focused text field.
///
/// IME composition uses [`ImeEvent`], not this enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKey {
    Insert(char),
    Backspace,
    Left,
    Right,
    Home,
    End,
    Delete,
}

/// One winit input-method event, without a winit dependency.
///
/// `Preedit` `cursor` is a byte range in `text`, matching `winit::event::Ime`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeEvent {
    Enabled,
    Disabled,
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    Commit(String),
}

/// Partial update used by live edits and by MCP `ui.patch`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NodePatch {
    pub text: Option<String>,
    pub name: Option<String>,
    pub fill: Option<String>,
    pub color: Option<String>,
    /// Also accepted as `fontSize` in JSON / MCP.
    #[serde(default, alias = "fontSize")]
    pub font_size: Option<f32>,
    /// Uniform padding in logical pixels. Also accepted as JSX `padding`.
    #[serde(default)]
    pub padding: Option<f32>,
    /// Gap between stacked children.
    #[serde(default)]
    pub gap: Option<f32>,
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
    event_handler: Option<EventHandler>,
    next_id: u64,
    root: Option<NodeId>,
    nodes: HashMap<NodeId, Node>,
    /// Keyboard / AccessKit focus. Defaults to the root once it exists.
    pub focus: Option<NodeId>,
    /// Last node that was clicked, if any.
    pub last_click: Option<NodeId>,
    pub(crate) transitions: Vec<animate::ActiveTransition>,
    /// Milliseconds last passed to [`Tree::advance`].
    pub(crate) time_ms: f64,
    pub(crate) reduced_motion: bool,
    pub(crate) gesture: Option<scroll_feel::Gesture>,
    pub(crate) flings: Vec<scroll_feel::Fling>,
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    pub fn new() -> Self {
        Self {
            event_handler: None,
            next_id: 1,
            root: None,
            nodes: HashMap::new(),
            focus: None,
            last_click: None,
            transitions: Vec::new(),
            time_ms: 0.0,
            reduced_motion: false,
            gesture: None,
            flings: Vec::new(),
        }
    }

    /// Keep the Rust event handler when replacing a tree.
    pub fn inherit_event_handler(&mut self, other: &Tree) {
        self.event_handler = other.event_handler.clone();
    }

    /// Install a synchronous handler for clicks and committed field changes.
    pub fn set_event_handler(
        &mut self,
        handler: impl Fn(&mut Tree, TreeEvent) + Send + Sync + 'static,
    ) {
        self.event_handler = Some(EventHandler(Arc::new(handler)));
    }

    fn emit(&mut self, event: TreeEvent) {
        if let Some(handler) = self.event_handler.clone() {
            (handler.0)(self, event);
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
        } else if self.root.is_none() {
            self.root = Some(id);
            if self.focus.is_none() {
                self.focus = Some(id);
            }
        }
        self.nodes.insert(id, node);
        Ok(id)
    }

    /// Attach an existing node as the last child of `parent`.
    /// The child may be an orphan, or it is moved from its previous parent.
    pub fn append(&mut self, parent: NodeId, child: NodeId) -> Result<(), TreeError> {
        if parent == child {
            return Err(TreeError {
                message: "cannot append a node to itself".into(),
            });
        }
        if self.root == Some(child) {
            return Err(TreeError {
                message: "cannot append the root".into(),
            });
        }
        if !self.nodes.contains_key(&parent) {
            return Err(TreeError {
                message: format!("unknown parent {parent}"),
            });
        }
        if !self.nodes.contains_key(&child) {
            return Err(TreeError {
                message: format!("unknown node {child}"),
            });
        }
        if let Some(old) = self.nodes[&child].parent {
            if let Some(old_node) = self.nodes.get_mut(&old) {
                old_node.children.retain(|c| *c != child);
            }
        }
        // A virtualized row can be off the child list and still sit in `list_slots`.
        // Drop that slot so the next layout does not attach it under the old list.
        for node in self.nodes.values_mut() {
            if node.kind == NodeKind::List {
                node.list_slots.retain(|id| *id != child);
            }
        }
        self.nodes.get_mut(&parent).unwrap().children.push(child);
        self.nodes.get_mut(&child).unwrap().parent = Some(parent);
        Ok(())
    }

    /// Drop a node and its descendants. Ids are never reused.
    pub fn remove(&mut self, id: NodeId) -> Result<(), TreeError> {
        if !self.nodes.contains_key(&id) {
            return Err(TreeError {
                message: format!("unknown node {id}"),
            });
        }
        if self.root == Some(id) {
            self.root = None;
        }
        if let Some(parent) = self.nodes[&id].parent {
            if let Some(p) = self.nodes.get_mut(&parent) {
                p.children.retain(|c| *c != id);
            }
        }
        // Offscreen list rows and spacers have no parent, so a child walk
        // would leave them in the tree. Take those that still belong to a
        // list inside this subtree. A row already reparented elsewhere stays.
        let mut queued = HashSet::new();
        let mut stack = vec![id];
        queued.insert(id);
        while let Some(cur) = stack.pop() {
            let (children, kind, slots, pad_top, pad_bottom) = {
                let Some(node) = self.nodes.get(&cur) else {
                    continue;
                };
                (
                    node.children.clone(),
                    node.kind,
                    node.list_slots.clone(),
                    node.list_pad_top,
                    node.list_pad_bottom,
                )
            };
            let mut extra = Vec::new();
            if kind == NodeKind::List {
                for slot in slots {
                    if claim_with_list(&self.nodes, &queued, cur, slot) {
                        extra.push(slot);
                    }
                }
                if pad_top != 0 {
                    let pad = NodeId::from_raw(pad_top);
                    if claim_with_list(&self.nodes, &queued, cur, pad) {
                        extra.push(pad);
                    }
                }
                if pad_bottom != 0 {
                    let pad = NodeId::from_raw(pad_bottom);
                    if claim_with_list(&self.nodes, &queued, cur, pad) {
                        extra.push(pad);
                    }
                }
            }
            for child in children.into_iter().chain(extra) {
                if queued.insert(child) {
                    stack.push(child);
                }
            }
        }
        for cur in &queued {
            self.nodes.remove(cur);
            if self.focus == Some(*cur) {
                self.focus = self.root;
            }
            if self.last_click == Some(*cur) {
                self.last_click = None;
            }
            self.drop_transitions(*cur);
            self.drop_scroll_motion(*cur);
        }
        for node in self.nodes.values_mut() {
            node.children.retain(|child| !queued.contains(child));
            if node.parent.is_some_and(|parent| queued.contains(&parent)) {
                node.parent = None;
            }
            if node.kind == NodeKind::List {
                node.list_slots.retain(|slot| !queued.contains(slot));
                if node.list_pad_top != 0 && queued.contains(&NodeId::from_raw(node.list_pad_top)) {
                    node.list_pad_top = 0;
                }
                if node.list_pad_bottom != 0
                    && queued.contains(&NodeId::from_raw(node.list_pad_bottom))
                {
                    node.list_pad_bottom = 0;
                }
            }
        }
        Ok(())
    }

    /// Move focus among attached buttons and text fields in depth-first tree order.
    /// From a non-eligible focus, start at the first (or last when reversed).
    /// Returns false if there are no eligible nodes.
    pub fn traverse_focus(&mut self, reverse: bool) -> bool {
        let mut stack: Vec<_> = self.root.into_iter().collect();
        let mut eligible = Vec::new();
        while let Some(id) = stack.pop() {
            if let Some(node) = self.get(id) {
                if matches!(node.kind, NodeKind::Button | NodeKind::TextField) {
                    eligible.push(id);
                }
                stack.extend(node.children.iter().rev().copied());
            }
        }
        if eligible.is_empty() {
            return false;
        }
        let index = eligible.iter().position(|id| Some(*id) == self.focus);
        let next = match (index, reverse) {
            (Some(i), false) => (i + 1) % eligible.len(),
            (Some(i), true) => (i + eligible.len() - 1) % eligible.len(),
            (None, false) => 0,
            (None, true) => eligible.len() - 1,
        };
        self.set_focus(eligible[next]).is_ok()
    }

    /// Set AccessKit / keyboard focus. The id must already be in the tree.
    ///
    /// Leaving a text field drops its preedit. The composition is not committed.
    pub fn set_focus(&mut self, id: NodeId) -> Result<(), TreeError> {
        if !self.nodes.contains_key(&id) {
            return Err(TreeError {
                message: format!("unknown node {id}"),
            });
        }
        if self.focus != Some(id) {
            if let Some(prev) = self.focus {
                self.clear_field_preedit(prev);
            }
        }
        self.focus = Some(id);
        Ok(())
    }

    /// Record a click on a known node: updates `last_click` and focus.
    pub fn click(&mut self, id: NodeId) -> Result<(), TreeError> {
        self.set_focus(id)?;
        self.last_click = Some(id);
        if let Some(node) = self.nodes.get_mut(&id) {
            if node.kind == NodeKind::TextField {
                node.preedit.clear();
                node.preedit_cursor = None;
                node.caret = node.text.as_deref().unwrap_or("").chars().count();
            }
        }
        self.emit(TreeEvent::Click(id));
        Ok(())
    }

    fn clear_field_preedit(&mut self, id: NodeId) {
        let Some(node) = self.nodes.get_mut(&id) else {
            return;
        };
        if node.kind != NodeKind::TextField {
            return;
        }
        node.preedit.clear();
        node.preedit_cursor = None;
    }

    /// Printable keyboard text for the focused text field.
    ///
    /// This is the desktop `KeyboardInput` path. Returns `Ok(false)` when
    /// focus is not a text field, `text` has no printable characters, or a
    /// preedit is showing. A preedit is left as it is (not committed, not
    /// cleared). [`Tree::insert_text`] is the agent path and does drop a preedit.
    pub fn type_keyboard_text(&mut self, text: &str) -> Result<bool, TreeError> {
        let Some(id) = self.focus else {
            return Ok(false);
        };
        if self.get(id).is_some_and(|n| n.kind != NodeKind::TextField) {
            return Ok(false);
        }
        if self.get(id).is_some_and(|n| !n.preedit.is_empty()) {
            return Ok(false);
        }
        let plain: String = text.chars().filter(|c| !c.is_control()).collect();
        if plain.is_empty() {
            return Ok(false);
        }
        for ch in plain.chars() {
            self.apply_on_field(id, FieldKey::Insert(ch))?;
        }
        Ok(true)
    }

    /// Insert plain text at the caret of a text field and focus it.
    ///
    /// Control characters are skipped. An in-progress preedit is dropped
    /// (not committed). Composition goes through [`Tree::apply_ime`].
    pub fn insert_text(&mut self, id: NodeId, text: &str) -> Result<(), TreeError> {
        self.ensure_field(id)?;
        self.set_focus(id)?;
        if let Some(node) = self.nodes.get_mut(&id) {
            node.preedit.clear();
            node.preedit_cursor = None;
        }
        for ch in text.chars().filter(|c| !c.is_control()) {
            self.apply_on_field(id, FieldKey::Insert(ch))?;
        }
        Ok(())
    }

    /// One key aimed at the focused text field.
    ///
    /// Returns `Ok(false)` when nothing is focused or focus is not a text field.
    pub fn apply_field_key(&mut self, key: FieldKey) -> Result<bool, TreeError> {
        let Some(id) = self.focus else {
            return Ok(false);
        };
        if self.get(id).is_some_and(|n| n.kind != NodeKind::TextField) {
            return Ok(false);
        }
        // The input method owns the keys while a composition is on screen.
        if self.get(id).is_some_and(|n| !n.preedit.is_empty()) {
            return Ok(false);
        }
        self.apply_on_field(id, key)?;
        Ok(true)
    }

    /// Replace the focused field's committed value. Used by the Android overlay.
    ///
    /// Control characters are dropped. A preedit is cleared and not committed.
    /// Returns `Ok(false)` when focus is not a text field or nothing changed.
    pub fn set_focused_field_value(&mut self, text: &str, caret: usize) -> Result<bool, TreeError> {
        let Some(id) = self.focus else {
            return Ok(false);
        };
        if self.get(id).is_none_or(|n| n.kind != NodeKind::TextField) {
            return Ok(false);
        }
        let plain: String = text.chars().filter(|c| !c.is_control()).collect();
        let caret = caret.min(plain.chars().count());
        let node = self.nodes.get_mut(&id).expect("field");
        if node.text.as_deref().unwrap_or("") == plain
            && node.caret == caret
            && node.preedit.is_empty()
            && node.preedit_cursor.is_none()
        {
            return Ok(false);
        }
        node.preedit.clear();
        node.preedit_cursor = None;
        node.text = Some(plain);
        node.caret = caret;
        self.emit(TreeEvent::Changed(id));
        Ok(true)
    }

    /// Apply one IME event to the focused text field.
    ///
    /// Returns `Ok(false)` when nothing is focused, focus is not a text field,
    /// or the event does not change the field (`Enabled`, or a repeated preedit).
    /// `Preedit` replaces the composition string. `Commit` inserts into the
    /// committed value and clears the preedit. `Disabled` drops an unfinished
    /// preedit without inserting it.
    pub fn apply_ime(&mut self, event: ImeEvent) -> Result<bool, TreeError> {
        let Some(id) = self.focus else {
            return Ok(false);
        };
        if self.get(id).is_some_and(|n| n.kind != NodeKind::TextField) {
            return Ok(false);
        }
        self.ensure_field(id)?;
        match event {
            ImeEvent::Enabled => Ok(false),
            ImeEvent::Disabled => {
                let node = self.nodes.get_mut(&id).expect("field");
                if node.preedit.is_empty() && node.preedit_cursor.is_none() {
                    return Ok(false);
                }
                node.preedit.clear();
                node.preedit_cursor = None;
                Ok(true)
            }
            ImeEvent::Preedit { text, cursor } => {
                let cursor = sanitize_preedit_cursor(&text, cursor);
                let node = self.nodes.get_mut(&id).expect("field");
                if node.preedit == text && node.preedit_cursor == cursor {
                    return Ok(false);
                }
                node.preedit = text;
                node.preedit_cursor = cursor;
                Ok(true)
            }
            ImeEvent::Commit(text) => {
                let node = self.nodes.get_mut(&id).expect("field");
                let had_preedit = !node.preedit.is_empty() || node.preedit_cursor.is_some();
                node.preedit.clear();
                node.preedit_cursor = None;
                let plain: String = text.chars().filter(|c| !c.is_control()).collect();
                if plain.is_empty() {
                    return Ok(had_preedit);
                }
                for ch in plain.chars() {
                    self.apply_on_field(id, FieldKey::Insert(ch))?;
                }
                Ok(true)
            }
        }
    }

    fn ensure_field(&self, id: NodeId) -> Result<(), TreeError> {
        match self.nodes.get(&id) {
            Some(node) if node.kind == NodeKind::TextField => Ok(()),
            Some(_) => Err(TreeError {
                message: format!("node {id} is not a text field"),
            }),
            None => Err(TreeError {
                message: format!("unknown node {id}"),
            }),
        }
    }

    fn apply_on_field(&mut self, id: NodeId, key: FieldKey) -> Result<(), TreeError> {
        let node = self.nodes.get_mut(&id).ok_or_else(|| TreeError {
            message: format!("unknown node {id}"),
        })?;
        if node.kind != NodeKind::TextField {
            return Err(TreeError {
                message: format!("node {id} is not a text field"),
            });
        }
        let current = node.text.clone().unwrap_or_default();
        let mut chars: Vec<char> = current.chars().collect();
        let mut caret = node.caret.min(chars.len());
        match key {
            FieldKey::Insert(ch) => {
                if ch.is_control() {
                    return Ok(());
                }
                chars.insert(caret, ch);
                caret += 1;
            }
            FieldKey::Backspace => {
                if caret > 0 {
                    chars.remove(caret - 1);
                    caret -= 1;
                }
            }
            FieldKey::Left => caret = caret.saturating_sub(1),
            FieldKey::Right => caret = (caret + 1).min(chars.len()),
            FieldKey::Home => caret = 0,
            FieldKey::End => caret = chars.len(),
            FieldKey::Delete => {
                if caret < chars.len() {
                    chars.remove(caret);
                }
            }
        }
        node.text = Some(chars.into_iter().collect());
        node.caret = caret;
        if current != node.text.as_deref().unwrap_or("") {
            self.emit(TreeEvent::Changed(id));
        }
        Ok(())
    }

    /// Set the vertical offset of a scroll container. Clamped to the content
    /// from the last layout. There is no overscroll.
    pub fn set_scroll_y(&mut self, id: NodeId, y: f32) -> Result<f32, TreeError> {
        self.stop_fling(id);
        self.write_scroll(id, y, false)
    }

    /// Move a scroll container by `delta_y` logical pixels (positive reveals
    /// content below). Clamped to the content. One wheel notch should pass
    /// about [`SCROLL_WHEEL_LINE`], not a page.
    pub fn scroll_by(&mut self, id: NodeId, delta_y: f32) -> Result<f32, TreeError> {
        let current = self
            .nodes
            .get(&id)
            .ok_or_else(|| TreeError {
                message: format!("unknown node {id}"),
            })?
            .scroll_y;
        // A wheel or programmatic step owns the offset. A running fling would
        // add itself on the next tick and fight that step.
        self.stop_fling(id);
        self.write_scroll(id, current + delta_y, true)
    }

    /// Scroll the innermost scroll container under `(x, y)`, if any.
    pub fn scroll_at(&mut self, x: f32, y: f32, delta_y: f32) -> Option<f32> {
        let id = self.scroll_target_at(x, y)?;
        self.scroll_by(id, delta_y).ok()
    }

    /// Innermost [`NodeKind::Scroll`] whose bounds contain `(x, y)`.
    pub fn scroll_target_at(&self, x: f32, y: f32) -> Option<NodeId> {
        let mut id = self.hit(x, y)?;
        loop {
            let node = self.nodes.get(&id)?;
            if node.kind == NodeKind::Scroll && node.bounds.contains(x, y) {
                return Some(id);
            }
            id = node.parent?;
        }
    }

    fn write_scroll(&mut self, id: NodeId, y: f32, add: bool) -> Result<f32, TreeError> {
        let node = self.nodes.get_mut(&id).ok_or_else(|| TreeError {
            message: format!("unknown node {id}"),
        })?;
        if node.kind != NodeKind::Scroll {
            return Err(TreeError {
                message: format!("node {id} is not a scroll container"),
            });
        }
        let next = if y.is_finite() {
            y
        } else if add {
            node.scroll_y
        } else {
            0.0
        };
        node.scroll_y = next;
        Ok(clamp_scroll(node))
    }

    /// Hit-test `(x, y)` in logical pixels, then [`click`] the node.
    pub fn click_at(&mut self, x: f32, y: f32) -> Option<NodeId> {
        let id = self.hit(x, y)?;
        self.click(id).ok()?;
        Some(id)
    }

    pub fn patch(&mut self, id: NodeId, patch: NodePatch) -> Result<(), TreeError> {
        let changed_text = patch.text.is_some();
        let node = self.nodes.get_mut(&id).ok_or_else(|| TreeError {
            message: format!("unknown node {id}"),
        })?;
        if let Some(text) = patch.text {
            // Buttons and labels start with name == text. A text field's name
            // is the accessible label and must stay put when the value changes.
            if node.kind != NodeKind::TextField && node.name.as_deref() == node.text.as_deref() {
                node.name = Some(text.clone());
            }
            let len = text.chars().count();
            if node.caret > len {
                node.caret = len;
            }
            if node.kind == NodeKind::TextField {
                node.preedit.clear();
                node.preedit_cursor = None;
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
        if let Some(padding) = patch.padding {
            node.layout.padding = padding;
        }
        if let Some(gap) = patch.gap {
            node.layout.gap = gap;
        }
        if changed_text {
            self.emit(TreeEvent::Changed(id));
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
        let label = if node.kind == NodeKind::TextField {
            let name = node.name.as_deref().map(pretty_field_string);
            let value = pretty_field_string(node.text.as_deref().unwrap_or_default());
            format!("name={} value={value}", name.as_deref().unwrap_or("null"))
        } else {
            format!("\"{}\"", escape_quotes(&node.display_name()))
        };
        let b = node.bounds;
        out.push_str(&format!(
            "{indent}- {}#{} {} [{:.0},{:.0} {:.0}x{:.0}]\n",
            node.kind.as_str(),
            node.id,
            label,
            b.x,
            b.y,
            b.width,
            b.height
        ));
        for child in &node.children {
            self.write_pretty(out, *child, depth + 1);
        }
    }

    /// One node as the JSON shape `ui.patch` returns.
    pub fn node_dump(&self, id: NodeId) -> Option<NodeDump> {
        let node = self.nodes.get(&id)?;
        Some(NodeDump {
            id: node.id.raw(),
            kind: node.kind,
            name: node.name.clone(),
            text: node.text.clone(),
            fill: node.fill.map(|c| c.to_hex()),
            color: node.color.map(|c| c.to_hex()),
            font_size: node.font_size,
            padding: node.layout.padding,
            gap: node.layout.gap,
            bounds: node.bounds,
            children: node.children.iter().map(|c| c.raw()).collect(),
        })
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
            padding: node.layout.padding,
            gap: node.layout.gap,
            bounds: node.bounds,
            children: node.children.iter().map(|c| c.raw()).collect(),
        });
        for child in &node.children {
            self.collect_dump(out, *child);
        }
    }
}

fn sanitize_preedit_cursor(text: &str, cursor: Option<(usize, usize)>) -> Option<(usize, usize)> {
    let (start, end) = cursor?;
    let start = clamp_char_boundary(text, start);
    let end = clamp_char_boundary(text, end);
    if start <= end {
        Some((start, end))
    } else {
        Some((end, start))
    }
}

fn clamp_scroll(node: &mut Node) -> f32 {
    let max = (node.scroll_extent - node.bounds.height).max(0.0);
    if !node.scroll_y.is_finite() {
        node.scroll_y = 0.0;
    }
    node.scroll_y = node.scroll_y.clamp(0.0, max);
    node.scroll_y
}

fn escape_quotes(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// Bound both fields before escaping, without cutting a UTF-8 character in half.
fn pretty_field_string(s: &str) -> String {
    const LIMIT: usize = 120;
    let mut chars = s.chars();
    let prefix: String = chars.by_ref().take(LIMIT).collect();
    let suffix = if chars.next().is_some() {
        " (truncated)"
    } else {
        ""
    };
    format!("{prefix:?}{suffix}")
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
    pub padding: f32,
    pub gap: f32,
    pub bounds: Rect,
    pub children: Vec<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_text_fields_show_name_and_committed_value() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        let node = tree.get_mut(field).unwrap();
        node.name = Some("Title".into());
        node.text = Some(String::new());
        assert!(tree
            .to_pretty_text()
            .contains("text_field#2 name=\"Title\" value=\"\""));

        let node = tree.get_mut(field).unwrap();
        node.text = Some("a\"\\\n\tb".into());
        node.preedit = "composing".into();
        assert!(tree
            .to_pretty_text()
            .contains(r#"text_field#2 name="Title" value="a\"\\\n\tb""#));
        assert_eq!(
            tree.node_dump(field).unwrap().text.as_deref(),
            Some("a\"\\\n\tb")
        );

        let node = tree.get_mut(field).unwrap();
        node.name = None;
        node.text = Some("é".repeat(121));
        let pretty = tree.to_pretty_text();
        assert!(pretty.contains(&format!(
            "text_field#2 name=null value=\"{}\" (truncated)",
            "é".repeat(120)
        )));
        tree.get_mut(field).unwrap().text = Some("é".repeat(120));
        assert!(!tree.to_pretty_text().contains("(truncated)"));
        let node = tree.get_mut(field).unwrap();
        node.name = Some("n".repeat(121));
        node.text = None;
        assert!(tree.to_pretty_text().contains(&format!(
            "name=\"{}\" (truncated) value=\"\"",
            "n".repeat(120)
        )));
        tree.get_mut(field).unwrap().name = Some(String::new());
        assert!(tree.to_pretty_text().contains("name=\"\" value=\"\""));
    }

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

    #[test]
    fn from_name_round_trips() {
        for kind in [
            NodeKind::Window,
            NodeKind::Group,
            NodeKind::Rect,
            NodeKind::Text,
            NodeKind::Button,
            NodeKind::TextField,
            NodeKind::Scroll,
            NodeKind::List,
            NodeKind::ListItem,
        ] {
            assert_eq!(NodeKind::from_name(kind.as_str()), Some(kind));
        }
        assert_eq!(NodeKind::from_name("nope"), None);
    }

    #[test]
    fn text_field_insert_backspace_and_patch_keep_the_label() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        tree.get_mut(field).unwrap().name = Some("Title".into());
        tree.get_mut(field).unwrap().text = Some("Hi".into());
        tree.get_mut(field).unwrap().caret = 2;
        tree.set_focus(field).unwrap();
        assert!(tree.apply_field_key(FieldKey::Insert('!')).unwrap());
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hi!"));
        assert!(tree.apply_field_key(FieldKey::Backspace).unwrap());
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hi"));
        tree.insert_text(field, " there").unwrap();
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hi there"));
        assert_eq!(tree.focus, Some(field));
        tree.patch(
            field,
            NodePatch {
                text: Some("Note".into()),
                ..NodePatch::default()
            },
        )
        .unwrap();
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Note"));
        assert_eq!(tree.get(field).unwrap().name.as_deref(), Some("Title"));
        assert_eq!(tree.get(field).unwrap().caret, 4);
        let other = tree.add(NodeKind::Text, Some(root)).unwrap();
        tree.set_focus(other).unwrap();
        assert!(!tree.apply_field_key(FieldKey::Insert('x')).unwrap());
    }

    #[test]
    fn field_caret_keys_edit_unicode_and_clamp_during_preedit() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        tree.get_mut(field).unwrap().text = Some("aé你🙂".into());
        tree.get_mut(field).unwrap().caret = 4;
        tree.set_focus(field).unwrap();
        for key in [
            FieldKey::Left,
            FieldKey::Left,
            FieldKey::Delete,
            FieldKey::Insert('界'),
        ] {
            assert!(tree.apply_field_key(key).unwrap());
        }
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("aé界🙂"));
        assert_eq!(tree.get(field).unwrap().caret, 3);
        for key in [FieldKey::Home, FieldKey::Left, FieldKey::Left] {
            tree.apply_field_key(key).unwrap();
        }
        assert_eq!(tree.get(field).unwrap().caret, 0);
        tree.apply_field_key(FieldKey::Delete).unwrap();
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("é界🙂"));
        for key in [FieldKey::End, FieldKey::Right, FieldKey::Delete] {
            tree.apply_field_key(key).unwrap();
        }
        assert_eq!(tree.get(field).unwrap().caret, 3);
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("é界🙂"));
        tree.apply_field_key(FieldKey::Home).unwrap();
        tree.apply_field_key(FieldKey::Right).unwrap();
        assert_eq!(tree.get(field).unwrap().caret, 1);
        tree.apply_ime(ImeEvent::Preedit {
            text: "文".into(),
            cursor: None,
        })
        .unwrap();
        for key in [
            FieldKey::Left,
            FieldKey::Right,
            FieldKey::Home,
            FieldKey::End,
            FieldKey::Delete,
        ] {
            assert!(!tree.apply_field_key(key).unwrap());
        }
        let node = tree.get(field).unwrap();
        assert_eq!(node.caret, 1);
        assert_eq!(node.text.as_deref(), Some("é界🙂"));
        assert_eq!(node.preedit, "文");
    }

    #[test]
    fn android_overlay_value_replaces_the_focused_field() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        tree.get_mut(field).unwrap().name = Some("Title".into());
        tree.get_mut(field).unwrap().text = Some("Hi".into());
        tree.get_mut(field).unwrap().caret = 2;
        tree.set_focus(field).unwrap();
        tree.apply_ime(ImeEvent::Preedit {
            text: "x".into(),
            cursor: None,
        })
        .unwrap();
        assert!(tree.set_focused_field_value("Hi你\n", 3).unwrap());
        let node = tree.get(field).unwrap();
        assert_eq!(node.text.as_deref(), Some("Hi你"));
        assert_eq!(node.name.as_deref(), Some("Title"));
        assert!(node.preedit.is_empty());
        assert_eq!(node.caret, 3);
        assert!(!tree.set_focused_field_value("Hi你", 3).unwrap());
        tree.set_focus(root).unwrap();
        assert!(!tree.set_focused_field_value("nope", 0).unwrap());
    }

    #[test]
    fn text_field_ime_preedit_then_commit_accent_and_cjk() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        tree.get_mut(field).unwrap().name = Some("Title".into());
        tree.get_mut(field).unwrap().text = Some("Hi".into());
        tree.get_mut(field).unwrap().caret = 2;
        tree.set_focus(field).unwrap();

        assert!(!tree.apply_ime(ImeEvent::Enabled).unwrap());
        assert!(tree
            .apply_ime(ImeEvent::Preedit {
                text: "`".into(),
                cursor: Some((0, 0)),
            })
            .unwrap());
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hi"));
        assert_eq!(tree.get(field).unwrap().composed_text(), "Hi`");
        assert_eq!(tree.get(field).unwrap().composed_caret(), Some(2));
        assert!(!tree.apply_field_key(FieldKey::Backspace).unwrap());
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hi"));

        assert!(tree
            .apply_ime(ImeEvent::Preedit {
                text: String::new(),
                cursor: None,
            })
            .unwrap());
        assert!(tree.apply_ime(ImeEvent::Commit("é".into())).unwrap());
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hié"));
        assert!(tree.get(field).unwrap().preedit.is_empty());
        assert_eq!(tree.get(field).unwrap().composed_text(), "Hié");
        assert_eq!(tree.get(field).unwrap().name.as_deref(), Some("Title"));

        // Caret sits between "H" and "ié" so a composition is not only at the end.
        tree.get_mut(field).unwrap().caret = 1;
        assert!(tree
            .apply_ime(ImeEvent::Preedit {
                text: "a b".into(),
                cursor: Some((3, 3)),
            })
            .unwrap());
        assert_eq!(tree.get(field).unwrap().composed_text(), "Ha bié");
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hié"));
        assert!(tree
            .apply_ime(ImeEvent::Preedit {
                text: "啊b".into(),
                cursor: Some((3, 3)),
            })
            .unwrap());
        // "啊" is three UTF-8 bytes; the cursor sits after it.
        assert_eq!(tree.get(field).unwrap().composed_caret(), Some(2));
        assert!(tree
            .apply_ime(ImeEvent::Preedit {
                text: String::new(),
                cursor: None,
            })
            .unwrap());
        assert!(tree.apply_ime(ImeEvent::Commit("啊不".into())).unwrap());
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("H啊不ié"));
        assert_eq!(tree.get(field).unwrap().composed_text(), "H啊不ié");

        tree.get_mut(field).unwrap().caret = 0;
        assert!(tree
            .apply_ime(ImeEvent::Preedit {
                text: "x".into(),
                cursor: None,
            })
            .unwrap());
        assert_eq!(tree.get(field).unwrap().composed_caret(), None);
        assert!(tree.apply_ime(ImeEvent::Disabled).unwrap());
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("H啊不ié"));
        assert!(tree.get(field).unwrap().preedit.is_empty());

        let other = tree.add(NodeKind::Button, Some(root)).unwrap();
        tree.set_focus(other).unwrap();
        assert!(!tree.apply_ime(ImeEvent::Commit("nope".into())).unwrap());
    }

    #[test]
    fn keyboard_text_is_ignored_while_preedit_is_showing() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        tree.get_mut(field).unwrap().text = Some("Hi".into());
        tree.get_mut(field).unwrap().caret = 2;
        tree.set_focus(field).unwrap();
        tree.apply_ime(ImeEvent::Preedit {
            text: "あ".into(),
            cursor: Some((3, 3)),
        })
        .unwrap();
        assert!(!tree.type_keyboard_text("x").unwrap());
        assert!(!tree.type_keyboard_text("ab").unwrap());
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hi"));
        assert_eq!(tree.get(field).unwrap().preedit, "あ");
        assert_eq!(tree.get(field).unwrap().composed_text(), "Hiあ");
        // Agent insert still drops the preedit. The desktop path does not.
        tree.insert_text(field, "z").unwrap();
        assert!(tree.get(field).unwrap().preedit.is_empty());
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hiz"));
    }

    #[test]
    fn focus_change_clears_the_previous_fields_preedit() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        tree.get_mut(field).unwrap().text = Some("Hi".into());
        tree.get_mut(field).unwrap().caret = 2;
        tree.set_focus(field).unwrap();
        tree.apply_ime(ImeEvent::Preedit {
            text: "ん".into(),
            cursor: Some((3, 3)),
        })
        .unwrap();
        let button = tree.add(NodeKind::Button, Some(root)).unwrap();
        tree.set_focus(button).unwrap();
        assert!(tree.get(field).unwrap().preedit.is_empty());
        assert!(tree.get(field).unwrap().preedit_cursor.is_none());
        assert_eq!(tree.get(field).unwrap().composed_text(), "Hi");
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("Hi"));
        // Disabled follows the new focus and must not leave the old field dirty.
        assert!(!tree.apply_ime(ImeEvent::Disabled).unwrap());
        assert!(tree.get(field).unwrap().preedit.is_empty());
        assert_eq!(tree.get(field).unwrap().composed_text(), "Hi");

        tree.set_focus(field).unwrap();
        tree.apply_ime(ImeEvent::Preedit {
            text: "か".into(),
            cursor: None,
        })
        .unwrap();
        let other = tree.add(NodeKind::TextField, Some(root)).unwrap();
        tree.get_mut(other).unwrap().text = Some("".into());
        tree.set_focus(other).unwrap();
        assert!(tree.get(field).unwrap().preedit.is_empty());
        assert_eq!(tree.get(field).unwrap().composed_text(), "Hi");
        assert!(tree.get(other).unwrap().preedit.is_empty());
    }

    #[test]
    fn append_attaches_an_orphan_and_remove_drops_it() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let orphan = tree.add(NodeKind::Text, None).unwrap();
        assert!(tree.get(root).unwrap().children.is_empty());
        tree.append(root, orphan).unwrap();
        assert_eq!(tree.get(root).unwrap().children, vec![orphan]);
        assert_eq!(tree.get(orphan).unwrap().parent, Some(root));
        tree.remove(orphan).unwrap();
        assert!(tree.get(orphan).is_none());
        assert!(tree.get(root).unwrap().children.is_empty());
        // ids are not reused
        let next = tree.add(NodeKind::Text, Some(root)).unwrap();
        assert_ne!(next, orphan);
    }
}
