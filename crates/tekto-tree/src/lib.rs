//! Scene tree with stable node ids.
//!
//! Ids are monotonic and are never reused. That keeps AccessKit, the
//! agent MCP, and live patches pointed at the same node after a rebuild
//! of only part of the tree.

mod animate;
mod fields;
mod overlay;
mod recording;
mod scroll_feel;
mod store;
mod tabs;
mod tokens;
pub use animate::{
    ease_linear, ease_smoothstep, reduced_motion_requested, Easing, StyleTransition,
    DEFAULT_TRANSITION_MS,
};
pub use recording::{Action, RecordedKey, Recording};
pub use store::{StateInfo, Store, StoreError};
pub use tekto_image::{Fit as ImageFit, Image, ImageError};
pub use tokens::{TokenPatch, Tokens};

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;

/// Named keys supported by agent input and desktop routing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedActionKey {
    Tab,
    ShiftTab,
    Enter,
    Space,
    Backspace,
    Escape,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    ShiftLeft,
    ShiftRight,
    ShiftHome,
    ShiftEnd,
    SelectAll,
    Home,
    End,
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
            "Escape" => Ok(Self::Escape),
            "ArrowLeft" => Ok(Self::ArrowLeft),
            "ArrowRight" => Ok(Self::ArrowRight),
            "ArrowUp" => Ok(Self::ArrowUp),
            "ArrowDown" => Ok(Self::ArrowDown),
            "Shift+ArrowLeft" => Ok(Self::ShiftLeft),
            "Shift+ArrowRight" => Ok(Self::ShiftRight),
            "Shift+Home" => Ok(Self::ShiftHome),
            "Shift+End" => Ok(Self::ShiftEnd),
            "SelectAll" => Ok(Self::SelectAll),
            "Home" => Ok(Self::Home),
            "End" => Ok(Self::End),
            _ => Err(format!(
                "unknown key {name:?}; expected Tab, Shift+Tab, Enter, Space, Backspace, Escape, ArrowLeft, ArrowRight, ArrowUp, ArrowDown, Home, End, Shift+ArrowLeft, Shift+ArrowRight, Shift+Home, Shift+End, or SelectAll"
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
            let button = tree.focus.filter(|id| {
                tree.get(*id).is_some_and(|n| {
                    (matches!(
                        n.kind,
                        NodeKind::Button | NodeKind::MenuItem | NodeKind::Select | NodeKind::Tab
                    ) || (key == NamedActionKey::Space
                        && matches!(
                            n.kind,
                            NodeKind::Checkbox | NodeKind::Switch | NodeKind::Radio | NodeKind::Tab
                        )))
                        && !n.disabled
                })
            });
            (false, button.filter(|_| !repeat))
        }
        NamedActionKey::ArrowLeft
        | NamedActionKey::ArrowRight
        | NamedActionKey::ArrowUp
        | NamedActionKey::ArrowDown
        | NamedActionKey::Home
        | NamedActionKey::End => {
            let before = tree
                .focus
                .and_then(|id| tree.get(id))
                .filter(|node| node.kind == NodeKind::Tab)
                .map(|node| node.id);
            let changed = tree
                .focus
                .and_then(|id| {
                    let node = tree.get(id)?;
                    if node.kind == NodeKind::MenuItem
                        && matches!(
                            key,
                            NamedActionKey::ArrowUp
                                | NamedActionKey::ArrowDown
                                | NamedActionKey::Home
                                | NamedActionKey::End
                        )
                    {
                        return Some(tree.move_menu(id, key).unwrap_or(false));
                    }
                    if matches!(node.kind, NodeKind::Radio | NodeKind::Tab)
                        && matches!(
                            key,
                            NamedActionKey::ArrowLeft
                                | NamedActionKey::ArrowRight
                                | NamedActionKey::ArrowUp
                                | NamedActionKey::ArrowDown
                        )
                    {
                        return Some(
                            tree.move_radio(
                                id,
                                matches!(key, NamedActionKey::ArrowLeft | NamedActionKey::ArrowUp),
                            )
                            .unwrap_or(false),
                        );
                    }
                    if node.kind != NodeKind::Slider {
                        return None;
                    }
                    let value = match key {
                        NamedActionKey::ArrowLeft | NamedActionKey::ArrowDown => {
                            node.progress - 0.05
                        }
                        NamedActionKey::ArrowRight | NamedActionKey::ArrowUp => {
                            node.progress + 0.05
                        }
                        NamedActionKey::Home => 0.0,
                        NamedActionKey::End => 1.0,
                        _ => unreachable!(),
                    };
                    Some(tree.set_slider_value(id, value).unwrap_or(false))
                })
                .unwrap_or(false);
            let activation = if changed {
                tree.focus
                    .filter(|id| Some(*id) != before && before.is_some())
            } else {
                None
            };
            (changed, activation)
        }
        NamedActionKey::ShiftLeft
        | NamedActionKey::ShiftRight
        | NamedActionKey::ShiftHome
        | NamedActionKey::ShiftEnd
        | NamedActionKey::SelectAll => {
            let key = match key {
                NamedActionKey::ShiftLeft => FieldKey::SelectLeft,
                NamedActionKey::ShiftRight => FieldKey::SelectRight,
                NamedActionKey::ShiftHome => FieldKey::SelectHome,
                NamedActionKey::ShiftEnd => FieldKey::SelectEnd,
                NamedActionKey::SelectAll => FieldKey::SelectAll,
                _ => unreachable!(),
            };
            (tree.apply_field_key(key).unwrap_or(false), None)
        }
        NamedActionKey::Escape => (tree.close_overlay(), None),
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
    Image,
    Text,
    Button,
    Checkbox,
    Switch,
    Progress,
    Slider,
    RadioGroup,
    Radio,
    TabList,
    Tab,
    TabPanel,
    Dialog,
    Menu,
    MenuItem,
    Select,
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
            NodeKind::Image => "image",
            NodeKind::Text => "text",
            NodeKind::Button => "button",
            NodeKind::Checkbox => "checkbox",
            NodeKind::Switch => "switch",
            NodeKind::Progress => "progress",
            NodeKind::Slider => "slider",
            NodeKind::RadioGroup => "radio_group",
            NodeKind::Radio => "radio",
            NodeKind::TabList => "tab_list",
            NodeKind::Tab => "tab",
            NodeKind::TabPanel => "tab_panel",
            NodeKind::Dialog => "dialog",
            NodeKind::Menu => "menu",
            NodeKind::MenuItem => "menu_item",
            NodeKind::Select => "select",
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
            "image" => Some(Self::Image),
            "text" => Some(Self::Text),
            "button" => Some(Self::Button),
            "checkbox" => Some(Self::Checkbox),
            "switch" => Some(Self::Switch),
            "progress" => Some(Self::Progress),
            "slider" => Some(Self::Slider),
            "radio_group" => Some(Self::RadioGroup),
            "radio" => Some(Self::Radio),
            "tab_list" => Some(Self::TabList),
            "tab" => Some(Self::Tab),
            "tab_panel" => Some(Self::TabPanel),
            "dialog" => Some(Self::Dialog),
            "menu" => Some(Self::Menu),
            "menu_item" => Some(Self::MenuItem),
            "select" => Some(Self::Select),
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
            NodeKind::Image,
            NodeKind::Progress,
            NodeKind::Slider,
            NodeKind::RadioGroup,
            NodeKind::Radio,
            NodeKind::TabList,
            NodeKind::Tab,
            NodeKind::TabPanel,
            NodeKind::Dialog,
            NodeKind::Menu,
            NodeKind::MenuItem,
            NodeKind::Select,
            NodeKind::Text,
            NodeKind::Button,
            NodeKind::Checkbox,
            NodeKind::Switch,
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
            NodeKind::Slider => &["click", "set_value"],
            NodeKind::Window
            | NodeKind::Group
            | NodeKind::Rect
            | NodeKind::Image
            | NodeKind::Text
            | NodeKind::Button
            | NodeKind::Checkbox
            | NodeKind::Switch
            | NodeKind::RadioGroup
            | NodeKind::Radio
            | NodeKind::TabList
            | NodeKind::Tab
            | NodeKind::TabPanel
            | NodeKind::Dialog
            | NodeKind::Menu
            | NodeKind::MenuItem
            | NodeKind::Select
            | NodeKind::Progress
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
/// A native window action bound to a node, for undecorated windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowControl {
    #[default]
    None,
    Minimize,
    Maximize,
    Close,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    #[serde(default)]
    pub disabled: bool,
    /// Makes the node a drag region for an undecorated window.
    #[serde(default)]
    pub drag_window: bool,
    /// Native window action performed when this node is activated.
    #[serde(default)]
    pub window_control: WindowControl,
    #[serde(default)]
    pub checked: bool,
    /// Normalized progress from zero to one.
    #[serde(default)]
    pub progress: f32,
    /// Whether a text field accepts line breaks.
    #[serde(default)]
    pub multiline: bool,
    pub name: Option<String>,
    /// Optional developer key for locating an attached node in tests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_key: Option<String>,
    /// Pixel buffers are runtime resources and are not included in scene JSON.
    #[serde(skip)]
    pub image: Option<tekto_image::Image>,
    #[serde(skip)]
    pub image_fit: tekto_image::Fit,
    #[serde(default)]
    pub decorative: bool,
    pub text: Option<String>,
    pub fill: Option<Color>,
    pub color: Option<Color>,
    pub font_size: f32,
    pub layout: LayoutStyle,
    /// Share of the parent's row or column. Zero keeps the natural size.
    #[serde(default)]
    pub grow: f32,
    /// Collapse this subtree when the viewport is narrower than this width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapse_below: Option<f32>,
    /// Set by layout when [`Self::collapse_below`] is triggered.
    #[serde(default)]
    pub collapsed: bool,
    pub bounds: Rect,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// Character index in the committed `text` (Unicode scalar values).
    /// Used by text fields. A preedit string, if any, is inserted here for display.
    #[serde(default)]
    pub caret: usize,
    /// Selection anchor in Unicode scalar indices. None means no selection.
    #[serde(default)]
    pub selection_anchor: Option<usize>,
    /// Selection edge positions in logical pixels, populated by layout.
    #[serde(default)]
    pub selection_x: Option<(f32, f32)>,
    /// Composition underline extents in logical pixels, populated by layout.
    #[serde(default)]
    pub preedit_x: Option<(f32, f32)>,
    /// Caret x inside the content box, in logical pixels. Layout fills this in.
    #[serde(default)]
    pub caret_x: f32,
    /// Multiline caret and selection geometry relative to the text origin.
    #[serde(default)]
    pub caret_y: f32,
    #[serde(default)]
    pub caret_height: f32,
    /// One box per grapheme of the composed text, relative to the text origin.
    /// Filled by layout; empty for very long text. Not saved with the scene.
    #[serde(skip)]
    pub text_chars: Vec<Rect>,
    #[serde(default)]
    pub selection_rects: Vec<Rect>,
    #[serde(default)]
    pub preedit_rects: Vec<Rect>,
    #[serde(default)]
    pub preedit_selection_rects: Vec<Rect>,
    #[serde(default)]
    pub text_scroll_y: f32,
    /// Preferred visual column for consecutive vertical caret moves.
    #[serde(skip)]
    pub vertical_goal_x: Option<f32>,
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
    /// Corner radius in logical pixels for the paint pass.
    #[serde(default)]
    pub radius: f32,
    /// Border stroke color. Drawn inside the node bounds when set;
    /// `border_width` zero hides it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_color: Option<Color>,
    /// Border stroke width in logical pixels. Ignored when `border_color` is
    /// unset.
    #[serde(default = "default_border_width")]
    pub border_width: f32,
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

/// Default border stroke width in logical pixels for a node that sets a
/// border color.
fn default_border_width() -> f32 {
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
            disabled: false,
            drag_window: false,
            window_control: WindowControl::None,
            checked: false,
            progress: 0.0,
            multiline: false,
            name: None,
            test_key: None,
            image: None,
            image_fit: tekto_image::Fit::Contain,
            decorative: false,
            text: None,
            fill: None,
            color: None,
            font_size: 16.0,
            layout: LayoutStyle::default(),
            grow: 0.0,
            collapse_below: None,
            collapsed: false,
            bounds: Rect::default(),
            parent: None,
            children: Vec::new(),
            caret: 0,
            selection_anchor: None,
            selection_x: None,
            preedit_x: None,
            caret_x: 0.0,
            caret_y: 0.0,
            caret_height: 0.0,
            text_chars: Vec::new(),
            selection_rects: Vec::new(),
            preedit_rects: Vec::new(),
            preedit_selection_rects: Vec::new(),
            text_scroll_y: 0.0,
            vertical_goal_x: None,
            preedit: String::new(),
            preedit_cursor: None,
            scroll_y: 0.0,
            scroll_extent: 0.0,
            list_viewport: f32::NAN,
            list_slots: Vec::new(),
            list_pad_top: 0,
            list_pad_bottom: 0,
            opacity: 1.0,
            radius: 0.0,
            border_color: None,
            border_width: 1.0,
        }
    }

    /// Apply the built-in defaults for a window or control before setting props.
    pub fn apply_widget_defaults(&mut self) {
        self.apply_widget_defaults_with(&Tokens::default());
    }

    /// Like [`Self::apply_widget_defaults`], but with an app token set.
    pub fn apply_widget_defaults_with(&mut self, tokens: &Tokens) {
        match self.kind {
            NodeKind::Window => {
                self.fill = Some(tokens.page);
                self.layout.padding = tokens.padding;
                self.layout.gap = tokens.gap;
                self.layout.column = true;
            }
            NodeKind::Button => {
                self.fill = Some(tokens.banner);
                self.color = Some(tokens.on_banner);
                self.font_size = tokens.body_size;
                self.layout.padding = 12.0;
            }
            NodeKind::Checkbox | NodeKind::Switch | NodeKind::Radio => {
                self.color = Some(tokens.ink);
                self.font_size = tokens.body_size;
                self.layout.padding = 4.0;
            }
            NodeKind::TabList => {
                self.layout.column = false;
            }
            NodeKind::Tab => {
                self.color = Some(tokens.ink);
                self.font_size = tokens.body_size;
                self.layout.padding = 8.0;
            }
            NodeKind::Menu => {
                self.fill = Some(tokens.page);
                self.color = Some(tokens.ink);
                self.layout.width = 180.0;
                self.layout.padding = 4.0;
                self.radius = 6.0;
                self.border_color = Some(tokens.border);
            }
            NodeKind::MenuItem => {
                self.color = Some(tokens.ink);
                self.font_size = tokens.body_size;
                self.layout.padding = 8.0;
            }
            NodeKind::Select => {
                self.fill = Some(tokens.field);
                self.color = Some(tokens.ink);
                self.font_size = tokens.body_size;
                self.layout.padding = 10.0;
                self.layout.width = 160.0;
            }
            NodeKind::Dialog => {
                self.fill = Some(tokens.page);
                self.color = Some(tokens.ink);
                self.layout.width = 320.0;
                self.layout.padding = 20.0;
                self.layout.gap = 12.0;
                self.radius = 8.0;
                self.border_color = Some(tokens.border);
            }
            NodeKind::Progress | NodeKind::Slider => {
                self.fill = Some(tokens.field);
                self.color = Some(tokens.ink);
                self.layout.width = 160.0;
                self.layout.height = if self.kind == NodeKind::Slider {
                    24.0
                } else {
                    8.0
                };
                self.radius = 4.0;
            }
            NodeKind::TextField => {
                self.fill = Some(tokens.field);
                self.color = Some(tokens.ink);
                self.font_size = tokens.body_size;
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
        let (start, end) = self.selection().unwrap_or_else(|| {
            let caret = self.caret.min(chars.len());
            (caret, caret)
        });
        let mut out: String = chars[..start].iter().collect();
        out.push_str(&self.preedit);
        out.extend(chars.drain(end..));
        out
    }

    /// Selected composition subrange in composed-text scalar indices.
    pub fn preedit_selection(&self) -> Option<(usize, usize)> {
        if self.preedit.is_empty() {
            return None;
        }
        let (start, end) = self.preedit_cursor?;
        let start = clamp_char_boundary(&self.preedit, start);
        let end = clamp_char_boundary(&self.preedit, end);
        if start == end {
            return None;
        }
        let offset = self.selection().map_or(self.caret, |(start, _)| start);
        let a = offset + self.preedit[..start].chars().count();
        let b = offset + self.preedit[..end].chars().count();
        Some((a.min(b), a.max(b)))
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
        let caret = self.selection().map_or(caret, |(start, _)| start);
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

fn grapheme_boundaries(text: &str) -> Vec<usize> {
    let mut result = vec![0];
    let mut chars = 0;
    for grapheme in text.graphemes(true) {
        chars += grapheme.chars().count();
        result.push(chars);
    }
    result
}

impl Node {
    /// Ordered selection in committed Unicode scalar indices, if non-empty.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let len = self.text.as_deref().unwrap_or("").chars().count();
        let anchor = self.selection_anchor?.min(len);
        let caret = self.caret.min(len);
        (anchor != caret).then_some((anchor.min(caret), anchor.max(caret)))
    }
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
    SelectLeft,
    SelectRight,
    SelectHome,
    SelectEnd,
    SelectAll,
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
    pub disabled: Option<bool>,
    pub checked: Option<bool>,
    /// Makes this node a drag region on an undecorated window.
    #[serde(default, alias = "dragWindow")]
    pub drag_window: Option<bool>,
    /// Native window action for activation: `minimize`, `maximize`, `close`, `none`.
    #[serde(default, alias = "windowControl")]
    pub window_control: Option<String>,
    /// Normalized progress, clamped to 0..=1.
    pub value: Option<f32>,
    pub text: Option<String>,
    pub name: Option<String>,
    pub fill: Option<String>,
    pub color: Option<String>,
    /// Also accepted as `fontSize` in JSON / MCP.
    #[serde(default, alias = "fontSize")]
    pub font_size: Option<f32>,
    /// Corner radius in logical pixels.
    #[serde(default)]
    pub radius: Option<f32>,
    /// Border stroke color.
    #[serde(default, alias = "borderColor")]
    pub border_color: Option<String>,
    /// Border stroke width in logical pixels.
    #[serde(default, alias = "borderWidth")]
    pub border_width: Option<f32>,
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

/// Placement of a window overlay relative to its anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlaySide {
    Above,
    Below,
    Left,
    Right,
    Center,
}

/// The currently open window overlay.
#[derive(Clone, Copy, Debug)]
pub struct Overlay {
    pub node: NodeId,
    pub anchor: NodeId,
    pub side: OverlaySide,
    /// Pointer position for a context menu, in logical pixels.
    pub position: Option<(f32, f32)>,
    previous_focus: Option<NodeId>,
}

/// The scene tree.
#[derive(Clone, Debug)]
pub struct Tree {
    event_handler: Option<EventHandler>,
    next_id: u64,
    root: Option<NodeId>,
    nodes: HashMap<NodeId, Node>,
    overlay: Option<Overlay>,
    context_menus: HashMap<NodeId, NodeId>,
    tab_panels: HashMap<NodeId, NodeId>,
    /// Keyboard / AccessKit focus. Defaults to the root once it exists.
    pub focus: Option<NodeId>,
    /// Last node that was clicked, if any.
    pub last_click: Option<NodeId>,
    /// Node receiving pointer motion until the current press ends.
    pub(crate) pointer_capture: Option<NodeId>,
    pub(crate) transitions: Vec<animate::ActiveTransition>,
    /// Milliseconds last passed to [`Tree::advance`].
    pub(crate) time_ms: f64,
    pub(crate) reduced_motion: bool,
    pub(crate) gesture: Option<scroll_feel::Gesture>,
    pub(crate) flings: Vec<scroll_feel::Fling>,
    pub(crate) tokens: Tokens,
    /// Set when a change may affect layout. The window skips layout while
    /// only paint properties (colour, opacity) animate.
    pub(crate) layout_dirty: bool,
    /// Clock value when the caret was last moved or edited, for blink reset.
    pub(crate) caret_origin_ms: f64,
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
            overlay: None,
            context_menus: HashMap::new(),
            tab_panels: HashMap::new(),
            focus: None,
            last_click: None,
            pointer_capture: None,
            transitions: Vec::new(),
            time_ms: 0.0,
            reduced_motion: false,
            gesture: None,
            flings: Vec::new(),
            tokens: Tokens::default(),
            layout_dirty: true,
            caret_origin_ms: 0.0,
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

    /// Mutable access marks the tree for a layout pass. Widgets and apps use
    /// this to set style before the next frame; colour-only animation does not
    /// go through here, so a colour tween does not trigger layout.
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.layout_dirty = true;
        self.nodes.get_mut(&id)
    }

    /// Whether the next frame needs a layout pass.
    pub fn layout_dirty(&self) -> bool {
        self.layout_dirty
    }

    /// Force the next frame to lay out, for example after a host resize.
    pub fn mark_layout_dirty(&mut self) {
        self.layout_dirty = true;
    }

    /// Called by the layout pass once bounds are written.
    pub fn clear_layout_dirty(&mut self) {
        self.layout_dirty = false;
    }

    /// Half-period of the caret blink, in milliseconds.
    pub const CARET_BLINK_MS: f64 = 530.0;

    /// Whether the caret should be painted on this frame.
    pub fn caret_visible(&self) -> bool {
        if !self.caret_blinking() {
            return false;
        }
        let elapsed = (self.time_ms - self.caret_origin_ms).max(0.0);
        ((elapsed / Self::CARET_BLINK_MS).floor() as u64).is_multiple_of(2)
    }

    /// A focused, enabled text field with no composition is blinking.
    pub fn caret_blinking(&self) -> bool {
        self.focus.is_some_and(|id| {
            self.get(id).is_some_and(|node| {
                node.kind == NodeKind::TextField && !node.disabled && node.preedit.is_empty()
            })
        })
    }

    /// Milliseconds until the caret should switch on or off, if it blinks.
    pub fn next_caret_toggle_ms(&self) -> Option<f64> {
        if !self.caret_blinking() {
            return None;
        }
        let elapsed = (self.time_ms - self.caret_origin_ms).max(0.0);
        let period = 2.0 * Self::CARET_BLINK_MS;
        let phase = elapsed % period;
        Some(if phase < Self::CARET_BLINK_MS {
            Self::CARET_BLINK_MS - phase
        } else {
            period - phase
        })
    }

    /// Show the caret solid and restart the blink from now.
    pub fn reset_caret_blink(&mut self) {
        self.caret_origin_ms = self.time_ms;
    }

    pub fn ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.keys().copied()
    }

    fn is_descendant(&self, mut id: NodeId, ancestor: NodeId) -> bool {
        loop {
            if id == ancestor {
                return true;
            }
            let Some(parent) = self.get(id).and_then(|n| n.parent) else {
                return false;
            };
            id = parent;
        }
    }

    /// Allocate a node and, if `parent` is set, append it as a child.
    pub fn add(&mut self, kind: NodeKind, parent: Option<NodeId>) -> Result<NodeId, TreeError> {
        self.layout_dirty = true;
        if matches!(kind, NodeKind::Radio | NodeKind::Tab)
            && parent.is_some_and(|id| {
                let expected = if kind == NodeKind::Radio {
                    NodeKind::RadioGroup
                } else {
                    NodeKind::TabList
                };
                self.nodes.get(&id).is_none_or(|n| n.kind != expected)
            })
        {
            return Err(TreeError {
                message: "radio must belong to a radio group".into(),
            });
        }
        let id = NodeId(self.next_id);
        self.next_id += 1;
        let mut node = Node::new(id, kind);
        node.apply_widget_defaults_with(&self.tokens);
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
        self.attach_child(parent, child, None)
    }

    /// Attach `child` under `parent` at `index`, clamped to the child count.
    /// The child keeps its id and is moved from any previous parent.
    pub fn insert_child_at(
        &mut self,
        parent: NodeId,
        index: usize,
        child: NodeId,
    ) -> Result<(), TreeError> {
        self.attach_child(parent, child, Some(index))
    }

    fn attach_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        index: Option<usize>,
    ) -> Result<(), TreeError> {
        self.layout_dirty = true;
        if parent == child {
            return Err(TreeError {
                message: "cannot append a node to itself".into(),
            });
        }
        if self.is_descendant(parent, child) {
            return Err(TreeError {
                message: "cannot append a node inside its own subtree".into(),
            });
        }
        if self.overlay.is_some_and(|o| o.node == child) {
            return Err(TreeError {
                message: "close the overlay before moving it".into(),
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
        if (self.nodes[&child].kind == NodeKind::Radio
            && self.nodes[&parent].kind != NodeKind::RadioGroup)
            || (self.nodes[&child].kind == NodeKind::Tab
                && self.nodes[&parent].kind != NodeKind::TabList)
        {
            return Err(TreeError {
                message: "radio must belong to a radio group".into(),
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
        let parent_node = self.nodes.get_mut(&parent).unwrap();
        let at = index
            .map(|index| index.min(parent_node.children.len()))
            .unwrap_or(parent_node.children.len());
        parent_node.children.insert(at, child);
        self.nodes.get_mut(&child).unwrap().parent = Some(parent);
        if matches!(self.nodes[&child].kind, NodeKind::Radio | NodeKind::Tab)
            && self.nodes[&child].checked
        {
            self.select_radio(child)?;
        }
        if self.nodes[&parent].kind == NodeKind::Select
            && self.nodes[&child].kind == NodeKind::MenuItem
            && self.nodes[&child].checked
        {
            self.select_option(parent, child)?;
        }
        Ok(())
    }

    /// Drop a node and its descendants. Ids are never reused.
    pub fn remove(&mut self, id: NodeId) -> Result<(), TreeError> {
        self.layout_dirty = true;
        if !self.nodes.contains_key(&id) {
            return Err(TreeError {
                message: format!("unknown node {id}"),
            });
        }
        if self.overlay.is_some_and(|o| {
            o.node == id
                && self
                    .get(o.anchor)
                    .is_some_and(|n| n.kind == NodeKind::Select)
        }) {
            self.close_overlay();
            return Ok(());
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
        if self.overlay.is_some_and(|overlay| {
            queued.contains(&overlay.node) || queued.contains(&overlay.anchor)
        }) {
            if let Some(overlay) = self.overlay.take() {
                if !queued.contains(&overlay.node) {
                    let _ = self.remove(overlay.node);
                }
            }
        }
        let owned_menus: Vec<_> = self
            .context_menus
            .iter()
            .filter(|(target, menu)| queued.contains(target) && !queued.contains(menu))
            .map(|(_, menu)| *menu)
            .collect();
        self.context_menus
            .retain(|target, menu| !queued.contains(target) && !queued.contains(menu));
        for menu in owned_menus {
            let _ = self.remove(menu);
        }
        let owned_panels: Vec<_> = self
            .tab_panels
            .iter()
            .filter(|(tab, panel)| queued.contains(tab) && !queued.contains(panel))
            .map(|(_, panel)| *panel)
            .collect();
        self.tab_panels
            .retain(|tab, panel| !queued.contains(tab) && !queued.contains(panel));
        for panel in owned_panels {
            let _ = self.remove(panel);
        }
        if self
            .pointer_capture
            .is_some_and(|captured| queued.contains(&captured))
        {
            self.pointer_capture = None;
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
        let mut stack: Vec<_> = self
            .overlay
            .map(|o| o.node)
            .or(self.root)
            .into_iter()
            .collect();
        let mut eligible = Vec::new();
        while let Some(id) = stack.pop() {
            if !self.is_visible(id) {
                continue;
            }
            if let Some(node) = self.get(id) {
                if !node.disabled
                    && matches!(
                        node.kind,
                        NodeKind::Button
                            | NodeKind::TextField
                            | NodeKind::Checkbox
                            | NodeKind::Switch
                            | NodeKind::Slider
                            | NodeKind::Radio
                            | NodeKind::Tab
                            | NodeKind::MenuItem
                            | NodeKind::Select
                    )
                {
                    let representative = if matches!(node.kind, NodeKind::Radio | NodeKind::Tab) {
                        let group_kind = if node.kind == NodeKind::Radio {
                            NodeKind::RadioGroup
                        } else {
                            NodeKind::TabList
                        };
                        node.parent
                            .and_then(|parent| self.get(parent))
                            .filter(|group| group.kind == group_kind)
                            .and_then(|group| {
                                let mut radios = group.children.iter().copied().filter(|child| {
                                    self.get(*child)
                                        .is_some_and(|n| n.kind == node.kind && !n.disabled)
                                });
                                let first = radios.next()?;
                                if self.get(first).is_some_and(|n| n.checked) {
                                    return Some(first);
                                }
                                Some(
                                    radios
                                        .find(|child| self.get(*child).is_some_and(|n| n.checked))
                                        .unwrap_or(first),
                                )
                            })
                            == Some(id)
                    } else {
                        true
                    };
                    if representative {
                        eligible.push(id);
                    }
                }
                if node.kind != NodeKind::Select {
                    stack.extend(node.children.iter().rev().copied());
                }
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

    /// Capture pointer motion for one attached node until the press ends.
    ///
    /// The node must be visible and enabled. Capture is cleared when the node
    /// is removed, when focus moves to a different node, or by
    /// [`Self::release_pointer`]. A slider drag is the current user.
    pub fn capture_pointer(&mut self, id: NodeId) -> Result<(), TreeError> {
        let attached =
            self.root == Some(id) || self.get(id).is_some_and(|node| node.parent.is_some());
        if !attached || !self.get(id).is_some_and(|node| !node.disabled) {
            return Err(TreeError {
                message: format!("cannot capture pointer for {id}"),
            });
        }
        if !self.is_visible(id) {
            return Err(TreeError {
                message: format!("cannot capture pointer for hidden node {id}"),
            });
        }
        self.pointer_capture = Some(id);
        Ok(())
    }

    /// Drop the capture, if any, and return the node that had it.
    pub fn release_pointer(&mut self) -> Option<NodeId> {
        self.pointer_capture.take()
    }

    /// The node receiving pointer motion for the current press.
    pub fn pointer_capture(&self) -> Option<NodeId> {
        self.pointer_capture
    }

    /// Set AccessKit / keyboard focus. The id must already be in the tree.
    ///
    /// Leaving a text field drops its preedit. The composition is not committed.
    pub fn set_focus(&mut self, id: NodeId) -> Result<(), TreeError> {
        self.layout_dirty = true;
        if !self.nodes.contains_key(&id) {
            return Err(TreeError {
                message: format!("unknown node {id}"),
            });
        }
        if !self.is_visible(id) {
            return Err(TreeError {
                message: format!("node {id} is in an inactive tab panel"),
            });
        }
        if self
            .overlay
            .is_some_and(|o| !self.is_descendant(id, o.node))
        {
            return Err(TreeError {
                message: format!("node {id} is blocked by an overlay"),
            });
        }
        if self.nodes[&id].kind == NodeKind::MenuItem
            && self.nodes[&id]
                .parent
                .and_then(|p| self.nodes.get(&p))
                .is_some_and(|n| n.kind == NodeKind::Select)
        {
            return Err(TreeError {
                message: format!("option {id} is not visible"),
            });
        }
        if self.nodes[&id].disabled {
            return Err(TreeError {
                message: format!("node {id} is disabled"),
            });
        }
        if self.pointer_capture.is_some_and(|captured| captured != id) {
            self.pointer_capture = None;
        }
        if self.focus != Some(id) {
            if let Some(prev) = self.focus {
                self.clear_field_preedit(prev);
            }
        }
        self.focus = Some(id);
        self.reset_caret_blink();
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
                node.selection_anchor = None;
                node.selection_x = None;
                node.vertical_goal_x = None;
            }
        }
        let is_select = self
            .nodes
            .get(&id)
            .is_some_and(|node| node.kind == NodeKind::Select);
        if is_select {
            self.emit(TreeEvent::Click(id));
            return self.open_select(id);
        }
        let select_anchor = self
            .overlay
            .filter(|overlay| {
                self.get(overlay.anchor)
                    .is_some_and(|node| node.kind == NodeKind::Select)
                    && self
                        .get(id)
                        .is_some_and(|node| node.kind == NodeKind::MenuItem)
                    && self.is_descendant(id, overlay.node)
            })
            .map(|overlay| overlay.anchor);
        if let Some(select) = select_anchor {
            self.select_option(select, id)?;
        }
        let is_radio = self
            .nodes
            .get(&id)
            .is_some_and(|node| matches!(node.kind, NodeKind::Radio | NodeKind::Tab));
        let changed = if is_radio {
            self.select_radio(id)?
        } else {
            self.nodes.get_mut(&id).is_some_and(|node| {
                if matches!(node.kind, NodeKind::Checkbox | NodeKind::Switch) {
                    node.checked = !node.checked;
                    true
                } else {
                    false
                }
            })
        };
        if changed {
            self.emit(TreeEvent::Changed(id));
        }
        self.emit(TreeEvent::Click(id));
        if self.overlay.is_some_and(|overlay| {
            self.get(overlay.node)
                .is_some_and(|n| n.kind == NodeKind::Menu)
                && self.is_descendant(id, overlay.node)
        }) && self.get(id).is_some_and(|n| n.kind == NodeKind::MenuItem)
        {
            self.close_overlay();
        }
        Ok(())
    }

    /// Open a select using a temporary popup while preserving its option ids.
    pub fn open_select(&mut self, id: NodeId) -> Result<(), TreeError> {
        let node = self.get(id).ok_or_else(|| TreeError {
            message: format!("unknown node {id}"),
        })?;
        if node.kind != NodeKind::Select || node.disabled {
            return Err(TreeError {
                message: "select is invalid or disabled".into(),
            });
        }
        let options = node.children.clone();
        if options
            .iter()
            .any(|id| self.get(*id).is_none_or(|n| n.kind != NodeKind::MenuItem))
        {
            return Err(TreeError {
                message: "select children must be options".into(),
            });
        }
        let mut labels = HashSet::new();
        for option in &options {
            let label = self.get(*option).expect("validated option").display_name();
            if label.trim().is_empty() || !labels.insert(label) {
                return Err(TreeError {
                    message: "select option labels must be non-empty and unique".into(),
                });
            }
        }
        let popup = self.add(NodeKind::Menu, None)?;
        {
            let node = self.get_mut(popup).expect("popup");
            node.name = Some("Options".into());
        }
        let selected = options.iter().copied().find(|option| {
            self.get(*option)
                .is_some_and(|node| node.checked && !node.disabled)
        });
        for option in options {
            self.append(popup, option)?;
        }
        self.open_overlay(popup, id, OverlaySide::Below)?;
        if let Some(selected) = selected {
            self.set_focus(selected)?;
        }
        Ok(())
    }

    /// Select an existing option by stable id, updating the labeled control.
    pub fn select_option(&mut self, select: NodeId, option: NodeId) -> Result<(), TreeError> {
        let node = self
            .get(select)
            .filter(|n| n.kind == NodeKind::Select && !n.disabled)
            .ok_or_else(|| TreeError {
                message: "select is invalid or disabled".into(),
            })?;
        let children = if let Some(overlay) = self.overlay.filter(|o| o.anchor == select) {
            self.get(overlay.node)
                .map(|n| n.children.clone())
                .unwrap_or_default()
        } else {
            node.children.clone()
        };
        let item = self
            .get(option)
            .filter(|n| n.kind == NodeKind::MenuItem && !n.disabled && children.contains(&option))
            .ok_or_else(|| TreeError {
                message: "option is invalid or disabled".into(),
            })?;
        let value = item.display_name();
        let changed = self.get(select).and_then(|n| n.text.as_deref()) != Some(value.as_str());
        for child in children {
            if let Some(node) = self.get_mut(child) {
                node.checked = child == option;
            }
        }
        self.get_mut(select).expect("select").text = Some(value);
        if changed {
            self.emit(TreeEvent::Changed(select));
        }
        Ok(())
    }

    /// Select a radio and clear its direct radio siblings in the same group.
    fn select_radio(&mut self, id: NodeId) -> Result<bool, TreeError> {
        let node = self.nodes.get(&id).ok_or_else(|| TreeError {
            message: format!("unknown node {id}"),
        })?;
        let kind = node.kind;
        let group_kind = if kind == NodeKind::Tab {
            NodeKind::TabList
        } else {
            NodeKind::RadioGroup
        };
        let parent = node.parent.ok_or_else(|| TreeError {
            message: "radio must belong to a radio group".into(),
        })?;
        let group = self
            .nodes
            .get(&parent)
            .filter(|n| n.kind == group_kind)
            .ok_or_else(|| TreeError {
                message: "radio must belong to a radio group".into(),
            })?;
        let siblings = group.children.clone();
        let changed = !self.nodes[&id].checked;
        for sibling in siblings {
            if let Some(node) = self.nodes.get_mut(&sibling).filter(|n| n.kind == kind) {
                node.checked = sibling == id;
            }
        }
        if kind == NodeKind::Tab && self.focus.is_some_and(|focus| !self.is_visible(focus)) {
            let _ = self.set_focus(id);
        }
        Ok(changed)
    }

    fn move_menu(&mut self, id: NodeId, key: NamedActionKey) -> Result<bool, TreeError> {
        let parent = self
            .get(id)
            .and_then(|n| n.parent)
            .ok_or_else(|| TreeError {
                message: "menu item has no menu".into(),
            })?;
        let menu = self
            .get(parent)
            .filter(|n| n.kind == NodeKind::Menu)
            .ok_or_else(|| TreeError {
                message: "menu item has no menu".into(),
            })?;
        let items: Vec<_> = menu
            .children
            .iter()
            .copied()
            .filter(|id| {
                self.get(*id)
                    .is_some_and(|n| n.kind == NodeKind::MenuItem && !n.disabled)
            })
            .collect();
        let index = items
            .iter()
            .position(|item| *item == id)
            .ok_or_else(|| TreeError {
                message: "menu item is disabled".into(),
            })?;
        let next = match key {
            NamedActionKey::Home => 0,
            NamedActionKey::End => items.len() - 1,
            NamedActionKey::ArrowUp => (index + items.len() - 1) % items.len(),
            _ => (index + 1) % items.len(),
        };
        self.set_focus(items[next])?;
        Ok(items[next] != id)
    }

    fn move_radio(&mut self, id: NodeId, reverse: bool) -> Result<bool, TreeError> {
        let kind = self
            .get(id)
            .ok_or_else(|| TreeError {
                message: "missing option".into(),
            })?
            .kind;
        let group_kind = if kind == NodeKind::Tab {
            NodeKind::TabList
        } else {
            NodeKind::RadioGroup
        };
        let parent = self
            .nodes
            .get(&id)
            .and_then(|n| n.parent)
            .ok_or_else(|| TreeError {
                message: "radio has no group".into(),
            })?;
        let group = self
            .nodes
            .get(&parent)
            .filter(|n| n.kind == group_kind)
            .ok_or_else(|| TreeError {
                message: "radio has no group".into(),
            })?;
        let items: Vec<_> = group
            .children
            .iter()
            .copied()
            .filter(|child| {
                self.nodes
                    .get(child)
                    .is_some_and(|n| n.kind == kind && !n.disabled)
            })
            .collect();
        let index = items
            .iter()
            .position(|child| *child == id)
            .ok_or_else(|| TreeError {
                message: "radio is disabled".into(),
            })?;
        let next = if reverse {
            (index + items.len() - 1) % items.len()
        } else {
            (index + 1) % items.len()
        };
        let target = items[next];
        self.set_focus(target)?;
        let changed = self.select_radio(target)?;
        if changed {
            self.emit(TreeEvent::Changed(target));
        }
        Ok(changed || target != id)
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
        self.layout_dirty = true;
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

    /// Set an enabled slider's normalized value and emit a change when it moves.
    pub fn set_slider_value(&mut self, id: NodeId, value: f32) -> Result<bool, TreeError> {
        if !value.is_finite() {
            return Err(TreeError {
                message: "slider value must be finite".into(),
            });
        }
        let node = self.nodes.get(&id).ok_or_else(|| TreeError {
            message: format!("unknown node {id}"),
        })?;
        if self.root.is_none_or(|root| !self.is_descendant(id, root)) {
            return Err(TreeError {
                message: format!("node {id} is not attached"),
            });
        }
        if node.kind != NodeKind::Slider {
            return Err(TreeError {
                message: format!("node {id} is not a slider"),
            });
        }
        if node.disabled
            || self
                .overlay
                .is_some_and(|o| !self.is_descendant(id, o.node))
        {
            return Err(TreeError {
                message: format!("node {id} is disabled or blocked"),
            });
        }
        let value = value.clamp(0.0, 1.0);
        if self.nodes[&id].progress == value {
            return Ok(false);
        }
        self.nodes.get_mut(&id).expect("slider").progress = value;
        self.emit(TreeEvent::Changed(id));
        Ok(true)
    }

    /// Hit-test `(x, y)` in logical pixels, then [`click`] the node.
    pub fn click_at(&mut self, x: f32, y: f32) -> Option<NodeId> {
        if let Some(overlay) = self.overlay {
            if self.hit_in(overlay.node, x, y).is_none() {
                self.close_overlay();
                return None;
            }
        }
        let id = self.hit(x, y)?;
        self.click(id).ok()?;
        if let Some(node) = self.get(id).filter(|node| node.kind == NodeKind::Slider) {
            let value = (x - node.bounds.x - 8.0) / (node.bounds.width - 16.0).max(1.0);
            self.set_slider_value(id, value).ok()?;
        }
        Some(id)
    }

    /// Disabling the focused control moves focus to the root and drops preedit.
    pub fn patch(&mut self, id: NodeId, patch: NodePatch) -> Result<(), TreeError> {
        self.layout_dirty = true;
        let changed_text = patch.text.is_some();
        let disabling = patch.disabled == Some(true);
        let node = self.nodes.get_mut(&id).ok_or_else(|| TreeError {
            message: format!("unknown node {id}"),
        })?;
        if patch.disabled.is_some()
            && !matches!(
                node.kind,
                NodeKind::Button
                    | NodeKind::TextField
                    | NodeKind::Checkbox
                    | NodeKind::Switch
                    | NodeKind::Slider
                    | NodeKind::Radio
                    | NodeKind::Tab
                    | NodeKind::MenuItem
                    | NodeKind::Select
            )
        {
            return Err(TreeError {
                message: format!(
                    "disabled is only supported on buttons, text fields, checkboxes, switches, sliders, radios, and menu items (node {id})"
                ),
            });
        }
        if let Some(value) = patch.value {
            if !matches!(node.kind, NodeKind::Progress | NodeKind::Slider) || !value.is_finite() {
                return Err(TreeError {
                    message: format!(
                        "value must be finite and is only supported on progress and slider nodes (node {id})"
                    ),
                });
            }
            node.progress = value.clamp(0.0, 1.0);
        }
        if let Some(checked) = patch.checked {
            if !matches!(
                node.kind,
                NodeKind::Checkbox
                    | NodeKind::Switch
                    | NodeKind::Radio
                    | NodeKind::Tab
                    | NodeKind::MenuItem
            ) {
                return Err(TreeError {
                    message: format!(
                        "checked is only supported on checkboxes, switches, and radios (node {id})"
                    ),
                });
            }
            node.checked = checked;
        }
        if let Some(disabled) = patch.disabled {
            node.disabled = disabled;
        }
        if let Some(drag) = patch.drag_window {
            node.drag_window = drag;
        }
        if let Some(control) = patch.window_control {
            node.window_control = match control.trim().to_ascii_lowercase().as_str() {
                "" | "none" => WindowControl::None,
                "minimize" => WindowControl::Minimize,
                "maximize" => WindowControl::Maximize,
                "close" => WindowControl::Close,
                other => {
                    return Err(TreeError {
                        message: format!("unknown window control `{other}`"),
                    });
                }
            };
        }
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
                node.selection_anchor = None;
                node.selection_x = None;
                node.vertical_goal_x = None;
            }
            node.text = Some(text);
        }
        if let Some(name) = patch.name {
            node.name = Some(name);
        }
        if let Some(fill) = patch.fill {
            // `none` or an empty value clears the fill, which lets a TSX
            // button sit on a coloured app bar without a second box.
            node.fill = if fill.trim().eq_ignore_ascii_case("none") || fill.trim().is_empty() {
                None
            } else {
                Some(Color::from_hex(&fill).ok_or_else(|| TreeError {
                    message: format!("bad fill color {fill}"),
                })?)
            };
        }
        if let Some(radius) = patch.radius {
            node.radius = radius.max(0.0);
        }
        if let Some(border_color) = patch.border_color {
            node.border_color = Some(Color::from_hex(&border_color).ok_or_else(|| TreeError {
                message: format!("bad border color {border_color}"),
            })?);
        }
        if let Some(border_width) = patch.border_width {
            node.border_width = border_width.max(0.0);
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
        let select_radio = patch.checked == Some(true)
            && matches!(node.kind, NodeKind::Radio | NodeKind::Tab)
            && node.parent.is_some();
        if select_radio {
            self.select_radio(id)?;
        }
        let select_parent = self
            .get(id)
            .filter(|n| n.kind == NodeKind::MenuItem && n.checked)
            .and_then(|n| n.parent)
            .filter(|parent| {
                self.get(*parent)
                    .is_some_and(|n| n.kind == NodeKind::Select)
            });
        if let Some(parent) = select_parent {
            self.select_option(parent, id)?;
        }
        if disabling
            && self.overlay.is_some_and(|o| {
                o.anchor == id && self.get(id).is_some_and(|n| n.kind == NodeKind::Select)
            })
        {
            self.close_overlay();
        }
        if disabling && self.focus == Some(id) {
            self.clear_field_preedit(id);
            self.focus = self.root;
        }
        if changed_text {
            self.emit(TreeEvent::Changed(id));
        }
        Ok(())
    }

    /// Hit-test from the front (last child) backwards.
    pub fn hit(&self, x: f32, y: f32) -> Option<NodeId> {
        if let Some(overlay) = self.overlay {
            return self.hit_in(overlay.node, x, y);
        }
        let root = self.root?;
        self.hit_in(root, x, y)
    }

    /// Whether a press at this point should start a native window drag.
    ///
    /// The walk stops at an interactive node, so a button inside a drag
    /// region stays clickable.
    pub fn drag_region_at(&self, x: f32, y: f32) -> bool {
        let mut current = self.hit(x, y);
        while let Some(id) = current {
            let Some(node) = self.get(id) else {
                return false;
            };
            if matches!(
                node.kind,
                NodeKind::Button
                    | NodeKind::TextField
                    | NodeKind::Slider
                    | NodeKind::Tab
                    | NodeKind::Checkbox
                    | NodeKind::Switch
                    | NodeKind::Radio
                    | NodeKind::Select
                    | NodeKind::MenuItem
            ) {
                return false;
            }
            if node.drag_window {
                return true;
            }
            current = node.parent;
        }
        false
    }

    fn hit_in(&self, id: NodeId, x: f32, y: f32) -> Option<NodeId> {
        let node = self.nodes.get(&id)?;
        if !self.is_visible(id) {
            return None;
        }
        if !node.bounds.contains(x, y) && !matches!(node.kind, NodeKind::Window) {
            return None;
        }
        for child in node
            .children
            .iter()
            .rev()
            .filter(|_| node.kind != NodeKind::Select)
        {
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
        let label = if matches!(
            node.kind,
            NodeKind::Checkbox | NodeKind::Switch | NodeKind::Radio | NodeKind::Tab
        ) {
            format!("{label} checked={}", node.checked)
        } else {
            label
        };
        let label = if node.disabled {
            format!("{label} disabled=true")
        } else {
            label
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
            disabled: node.disabled,
            checked: node.checked,
            progress: node.progress,
            multiline: node.multiline,
            name: node.name.clone(),
            test_key: node.test_key.clone(),
            image: image_dump(node),
            text: node.text.clone(),
            fill: node.fill.map(|c| c.to_hex()),
            color: node.color.map(|c| c.to_hex()),
            font_size: node.font_size,
            selection: node.selection(),
            radius: node.radius,
            border_color: node.border_color.map(|c| c.to_hex()),
            border_width: node.border_width,
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
            disabled: node.disabled,
            checked: node.checked,
            progress: node.progress,
            multiline: node.multiline,
            name: node.name.clone(),
            test_key: node.test_key.clone(),
            image: image_dump(node),
            text: node.text.clone(),
            fill: node.fill.map(|c| c.to_hex()),
            color: node.color.map(|c| c.to_hex()),
            font_size: node.font_size,
            selection: node.selection(),
            radius: node.radius,
            border_color: node.border_color.map(|c| c.to_hex()),
            border_width: node.border_width,
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
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub checked: bool,
    #[serde(default)]
    pub progress: f32,
    #[serde(default)]
    pub multiline: bool,
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageDump>,
    pub text: Option<String>,
    pub fill: Option<String>,
    pub color: Option<String>,
    pub font_size: f32,
    #[serde(default)]
    pub selection: Option<(usize, usize)>,
    #[serde(default)]
    pub radius: f32,
    #[serde(default)]
    pub border_color: Option<String>,
    #[serde(default = "default_border_width")]
    pub border_width: f32,
    pub padding: f32,
    pub gap: f32,
    pub bounds: Rect,
    pub children: Vec<u64>,
}

/// Image metadata for agents; pixel buffers remain private to the scene.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageDump {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fit: String,
    pub decorative: bool,
}

fn image_dump(node: &Node) -> Option<ImageDump> {
    (node.kind == NodeKind::Image).then(|| ImageDump {
        width: node.image.as_ref().map(Image::width),
        height: node.image.as_ref().map(Image::height),
        fit: match node.image_fit {
            ImageFit::Contain => "contain",
            ImageFit::Cover => "cover",
        }
        .into(),
        decorative: node.decorative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_child_at_positions_and_moves_a_node() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let a = tree.add(NodeKind::Rect, Some(root)).unwrap();
        let b = tree.add(NodeKind::Rect, Some(root)).unwrap();
        let c = tree.add(NodeKind::Rect, Some(root)).unwrap();
        tree.insert_child_at(root, 1, c).unwrap();
        assert_eq!(tree.get(root).unwrap().children, vec![a, c, b]);
        // A large index is clamped to the end.
        tree.insert_child_at(root, 99, a).unwrap();
        assert_eq!(tree.get(root).unwrap().children, vec![c, b, a]);
        assert!(tree.insert_child_at(root, 0, root).is_err());
    }

    #[test]
    fn pointer_capture_releases_on_remove_and_focus_change() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let slider = tree.add(NodeKind::Slider, Some(root)).unwrap();
        let button = tree.add(NodeKind::Button, Some(root)).unwrap();

        tree.capture_pointer(slider).unwrap();
        assert_eq!(tree.pointer_capture(), Some(slider));
        // Keeping focus on the captured node keeps the capture.
        tree.set_focus(slider).unwrap();
        assert_eq!(tree.pointer_capture(), Some(slider));
        // Focus moving elsewhere drops it.
        tree.set_focus(button).unwrap();
        assert_eq!(tree.pointer_capture(), None);

        tree.capture_pointer(slider).unwrap();
        tree.remove(slider).unwrap();
        assert_eq!(tree.pointer_capture(), None);

        let disabled = tree.add(NodeKind::Slider, Some(root)).unwrap();
        tree.get_mut(disabled).unwrap().disabled = true;
        assert!(tree.capture_pointer(disabled).is_err());
        let detached = tree.add(NodeKind::Slider, None).unwrap();
        assert!(tree.capture_pointer(detached).is_err());
    }

    #[test]
    fn multiline_fields_keep_newlines_and_move_to_line_edges() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        tree.get_mut(field).unwrap().multiline = true;
        tree.insert_text(field, "one\ntwo🙂").unwrap();
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("one\ntwo🙂"));
        tree.apply_field_key(FieldKey::Home).unwrap();
        assert_eq!(tree.get(field).unwrap().caret, 4);
        tree.apply_field_key(FieldKey::End).unwrap();
        assert_eq!(tree.get(field).unwrap().caret, 8);
        tree.apply_field_key(FieldKey::Backspace).unwrap();
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("one\ntwo"));
        let single = tree.add(NodeKind::TextField, Some(root)).unwrap();
        tree.insert_text(single, "a\nb").unwrap();
        assert_eq!(tree.get(single).unwrap().text.as_deref(), Some("ab"));
    }

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
