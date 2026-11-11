//! Map the Tekto scene tree to an AccessKit tree.
//!
//! This is the same tree agents get from MCP `ui.tree` and `GET /accesskit`.
//! Screen readers and coding agents share one source of truth.

use accesskit::{Node, NodeId, Rect, Role, TextPosition, TextSelection, Toggled, Tree, TreeUpdate};
use serde::Serialize;
use tekto_tree::{NodeKind, Tree as SceneTree};
use unicode_segmentation::UnicodeSegmentation;

/// Build a full AccessKit tree update from the current scene.
///
/// Focus is `scene.focus` if that node still exists, otherwise the root.
pub fn tree_update(scene: &SceneTree) -> Option<TreeUpdate> {
    tree_update_with_focus(scene, scene.focus)
}

/// Like [`tree_update`], but the caller can override focus.
pub fn tree_update_with_focus(
    scene: &SceneTree,
    focus: Option<tekto_tree::NodeId>,
) -> Option<TreeUpdate> {
    let root = scene.root()?;
    let focus_id = focus.filter(|id| scene.get(*id).is_some()).unwrap_or(root);
    let mut nodes = Vec::new();
    collect(scene, root, &mut nodes);
    Some(TreeUpdate {
        nodes,
        tree: Some(Tree::new(ak_id(root))),
        focus: ak_id(focus_id),
    })
}

/// A JSON-friendly dump of the AccessKit tree (same ids as the scene tree).
#[derive(Clone, Debug, Serialize)]
pub struct AccessKitDump {
    pub focus: u64,
    pub nodes: Vec<AccessKitNodeDump>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AccessKitNodeDump {
    pub id: u64,
    pub role: String,
    pub disabled: bool,
    pub modal: bool,
    pub expanded: Option<bool>,
    pub selected: Option<bool>,
    pub toggled: Option<bool>,
    pub numeric_value: Option<f64>,
    pub min_numeric_value: Option<f64>,
    pub max_numeric_value: Option<f64>,
    pub label: Option<String>,
    pub value: Option<String>,
    pub bounds: [f64; 4],
    pub children: Vec<u64>,
}

/// Dump the AccessKit tree for the dev server and for tests.
pub fn dump(scene: &SceneTree) -> Option<AccessKitDump> {
    let update = tree_update(scene)?;
    let nodes = update
        .nodes
        .iter()
        .map(|(id, node)| AccessKitNodeDump {
            id: id.0,
            role: format!("{:?}", node.role()),
            disabled: node.is_disabled(),
            modal: node.is_modal(),
            expanded: node.is_expanded(),
            selected: node.is_selected(),
            toggled: node.toggled().map(|value| value == Toggled::True),
            numeric_value: node.numeric_value(),
            min_numeric_value: node.min_numeric_value(),
            max_numeric_value: node.max_numeric_value(),
            label: node.label().map(|s| s.to_string()),
            value: node.value().map(|s| s.to_string()),
            bounds: node.bounds().map_or([0.0; 4], |r| [r.x0, r.y0, r.x1, r.y1]),
            children: node.children().iter().map(|c| c.0).collect(),
        })
        .collect();
    Some(AccessKitDump {
        focus: update.focus.0,
        nodes,
    })
}

fn collect(scene: &SceneTree, id: tekto_tree::NodeId, out: &mut Vec<(NodeId, Node)>) {
    let Some(src) = scene.get(id).filter(|_| scene.is_visible(id)) else {
        return;
    };
    let select_popup = scene.overlay().is_some_and(|o| {
        o.node == id
            && scene
                .get(o.anchor)
                .is_some_and(|n| n.kind == NodeKind::Select)
    });
    let option = src.kind == NodeKind::MenuItem
        && src.parent.and_then(|p| scene.get(p)).is_some_and(|parent| {
            parent.kind == NodeKind::Select
                || (parent.kind == NodeKind::Menu
                    && scene.overlay().is_some_and(|o| {
                        o.node == parent.id
                            && scene
                                .get(o.anchor)
                                .is_some_and(|n| n.kind == NodeKind::Select)
                    }))
        });
    let mut node = Node::new(if select_popup {
        Role::ListBox
    } else if option {
        Role::ListBoxOption
    } else if src.kind == NodeKind::TextField && src.multiline {
        Role::MultilineTextInput
    } else {
        role_for(src.kind)
    });
    if let Some(panel) = scene
        .panel_for_tab(id)
        .filter(|panel| scene.is_visible(*panel))
    {
        node.set_controls(vec![ak_id(panel)]);
    }
    if let Some(tab) = scene.tab_for_panel(id) {
        node.set_labelled_by(vec![ak_id(tab)]);
    }
    if src.kind == NodeKind::Image && src.decorative {
        node.set_hidden();
    }
    if option || src.kind == NodeKind::Tab {
        node.set_selected(src.checked);
    }
    if src.kind == NodeKind::Select {
        node.set_expanded(scene.overlay().is_some_and(|o| o.anchor == id));
        node.set_has_popup(accesskit::HasPopup::Listbox);
    }
    if matches!(
        src.kind,
        NodeKind::Checkbox | NodeKind::Switch | NodeKind::Radio
    ) {
        node.set_toggled(if src.checked {
            Toggled::True
        } else {
            Toggled::False
        });
    }
    if matches!(src.kind, NodeKind::Progress | NodeKind::Slider) {
        node.set_numeric_value(f64::from(src.progress));
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(1.0);
    }
    if scene.overlay().is_some_and(|overlay| {
        overlay.anchor == id
            && scene
                .get(overlay.node)
                .is_some_and(|n| n.kind == NodeKind::Menu)
    }) {
        node.set_expanded(true);
        node.set_has_popup(accesskit::HasPopup::Menu);
    }
    if src.kind == NodeKind::Dialog && scene.overlay().is_some_and(|overlay| overlay.node == id) {
        node.set_modal();
    }
    if src.disabled {
        node.set_disabled();
    }
    if src.kind == NodeKind::TextField {
        if let Some(name) = src.name.as_ref().filter(|s| !s.is_empty()) {
            node.set_label(name.clone());
        }
        // Committed text plus the current preedit, so a screen reader sees
        // the composition before it is committed.
        node.set_value(src.composed_text());
    } else {
        let label = src.display_name();
        if !label.is_empty() {
            node.set_label(label);
        }
        if let Some(text) = &src.text {
            node.set_value(text.clone());
        }
    }
    let b = src.bounds;
    node.set_bounds(Rect::new(
        b.x as f64,
        b.y as f64,
        (b.x + b.width) as f64,
        (b.y + b.height) as f64,
    ));
    let text_run = if src.kind == NodeKind::TextField {
        text_runs(scene, src)
    } else {
        None
    };
    let children: Vec<NodeId> = if src.kind == NodeKind::Select {
        Vec::new()
    } else {
        src.children
            .iter()
            .copied()
            .filter(|child| scene.is_visible(*child))
            .map(ak_id)
            .collect()
    };
    let mut children = children;
    if let Some((runs, selection)) = &text_run {
        children.extend(runs.iter().map(|(id, _)| *id));
        if let Some(selection) = selection {
            node.set_text_selection(*selection);
        }
    }
    if !children.is_empty() {
        node.set_children(children);
    }
    out.push((ak_id(id), node));
    if let Some((runs, _)) = text_run {
        out.extend(runs);
    }
    for child in src.children.iter().filter(|_| src.kind != NodeKind::Select) {
        collect(scene, *child, out);
    }
}

type TextRuns = (Vec<(NodeId, Node)>, Option<TextSelection>);

// Reserve a range that stays exact in JavaScript numbers. Line 0 keeps
// `field | RUN_BIT`; later lines add the line number above bit 32. Check for a
// scene collision and omit metadata rather than expose ambiguous IDs.
fn text_runs(scene: &SceneTree, field: &tekto_tree::Node) -> Option<TextRuns> {
    const TEXT_RUN_BIT: u64 = 1 << 52;
    let raw = field.id.raw();
    if raw >= TEXT_RUN_BIT || (field.multiline && raw >= 1 << 32) {
        return None;
    }
    let value = field.composed_text();
    if !field.multiline
        && value
            .chars()
            .any(|c| c == char::from(10) || c == char::from(13))
    {
        return None;
    }
    let graphemes: Vec<&str> = value.graphemes(true).collect();
    // Each run is one line and keeps its trailing line break.
    let mut lines: Vec<(usize, usize)> = Vec::new(); // grapheme start, end
    let mut start = 0;
    for (i, g) in graphemes.iter().enumerate() {
        if g.contains(char::from(10)) {
            lines.push((start, i + 1));
            start = i + 1;
        }
    }
    // A multiline field that ends with a break has an empty last line, where
    // the caret sits after the break.
    if start < graphemes.len() || lines.is_empty() || field.multiline {
        lines.push((start, graphemes.len()));
    }
    if lines.len() >= 1 << 19 {
        return None;
    }
    let run_id = |line: usize| NodeId(TEXT_RUN_BIT | ((line as u64) << 32) | raw);
    if (0..lines.len()).any(|l| {
        scene
            .get(tekto_tree::NodeId::from_raw(run_id(l).0))
            .is_some()
    }) {
        return None;
    }
    let index = |scalar: usize| -> usize {
        let mut scalars = 0;
        for (i, grapheme) in graphemes.iter().enumerate() {
            let next = scalars + grapheme.chars().count();
            if scalar < next {
                return i;
            }
            scalars = next;
        }
        graphemes.len()
    };
    let position = |global: usize| -> TextPosition {
        let line = lines
            .iter()
            .position(|(s, e)| global >= *s && global < *e)
            .unwrap_or(lines.len() - 1);
        TextPosition {
            node: run_id(line),
            character_index: global - lines[line].0,
        }
    };
    let selection = if field.preedit.is_empty() {
        let focus = index(field.caret);
        let anchor = index(field.selection_anchor.unwrap_or(field.caret));
        Some((anchor, focus))
    } else if let Some((start, end)) = field.preedit_selection() {
        Some((index(start), index(end)))
    } else {
        field
            .composed_caret()
            .map(|caret| (index(caret), index(caret)))
    }
    .map(|(anchor, focus)| TextSelection {
        anchor: position(anchor),
        focus: position(focus),
    });
    let bounds = field.bounds;
    // Per-grapheme boxes come from layout, relative to the text origin.
    let have_geometry = field.text_chars.len() == graphemes.len();
    let origin_x = f64::from(bounds.x + field.layout.padding);
    let origin_y = f64::from(
        bounds.y + field.layout.padding
            - if field.multiline {
                field.text_scroll_y
            } else {
                0.0
            },
    );
    let mut runs = Vec::with_capacity(lines.len());
    let mut last_bottom = 0.0_f64;
    let mut last_height = f64::from(field.caret_height);
    for (l, (s, e)) in lines.iter().enumerate() {
        let parts = &graphemes[*s..*e];
        let lengths: Vec<u8> = parts
            .iter()
            .map(|part| u8::try_from(part.len()))
            .collect::<Result<_, _>>()
            .ok()?;
        let mut run = Node::new(Role::TextRun);
        run.set_value(parts.concat());
        run.set_character_lengths(lengths);
        let boxes =
            &field.text_chars[(*s).min(field.text_chars.len())..(*e).min(field.text_chars.len())];
        if have_geometry && !boxes.is_empty() {
            let left = boxes
                .iter()
                .map(|b| f64::from(b.x))
                .fold(f64::MAX, f64::min);
            let top = boxes
                .iter()
                .map(|b| f64::from(b.y))
                .fold(f64::MAX, f64::min);
            let right = boxes
                .iter()
                .map(|b| f64::from(b.x + b.width))
                .fold(f64::MIN, f64::max);
            let bottom = boxes
                .iter()
                .map(|b| f64::from(b.y + b.height))
                .fold(f64::MIN, f64::max);
            run.set_bounds(Rect::new(
                origin_x + left,
                origin_y + top,
                origin_x + right,
                origin_y + bottom,
            ));
            // Positions only make sense when the logical line is one visual
            // line; a wrapped line keeps its bounds only.
            if boxes.iter().all(|b| (f64::from(b.y) - top).abs() < 0.5) {
                // Right-to-left runs lay out from the right edge, so offsets
                // are measured from there. Mixed-direction lines are not
                // reported separately; the first and last boxes decide.
                let rtl = boxes.len() > 1 && boxes[0].x > boxes[boxes.len() - 1].x;
                if rtl {
                    run.set_text_direction(accesskit::TextDirection::RightToLeft);
                }
                run.set_character_positions(
                    boxes
                        .iter()
                        .map(|b| {
                            if rtl {
                                (right - f64::from(b.x + b.width)) as f32
                            } else {
                                (f64::from(b.x) - left) as f32
                            }
                        })
                        .collect::<Vec<_>>(),
                );
                run.set_character_widths(boxes.iter().map(|b| b.width).collect::<Vec<_>>());
            }
            last_bottom = origin_y + bottom;
            last_height = bottom - top;
        } else if have_geometry && l > 0 {
            // Empty last line after a final break: one line below the last.
            run.set_bounds(Rect::new(
                origin_x,
                last_bottom,
                origin_x,
                last_bottom + last_height,
            ));
            last_bottom += last_height;
        } else {
            run.set_bounds(Rect::new(
                bounds.x as f64,
                bounds.y as f64,
                (bounds.x + bounds.width) as f64,
                (bounds.y + bounds.height) as f64,
            ));
        }
        runs.push((run_id(l), run));
    }
    Some((runs, selection))
}

fn role_for(kind: NodeKind) -> Role {
    match kind {
        NodeKind::Window => Role::Window,
        NodeKind::Group => Role::GenericContainer,
        NodeKind::Rect => Role::GenericContainer,
        NodeKind::Image => Role::Image,
        NodeKind::Text => Role::Label,
        NodeKind::Button => Role::Button,
        NodeKind::Checkbox => Role::CheckBox,
        NodeKind::Switch => Role::Switch,
        NodeKind::Progress => Role::ProgressIndicator,
        NodeKind::Slider => Role::Slider,
        NodeKind::RadioGroup => Role::RadioGroup,
        NodeKind::Radio => Role::RadioButton,
        NodeKind::TabList => Role::TabList,
        NodeKind::Tab => Role::Tab,
        NodeKind::TabPanel => Role::TabPanel,
        NodeKind::Dialog => Role::Dialog,
        NodeKind::Menu => Role::Menu,
        NodeKind::MenuItem => Role::MenuItem,
        NodeKind::Select => Role::ComboBox,
        NodeKind::TextField => Role::TextInput,
        NodeKind::Scroll => Role::ScrollView,
        NodeKind::List => Role::List,
        NodeKind::ListItem => Role::ListItem,
    }
}

fn ak_id(id: tekto_tree::NodeId) -> NodeId {
    NodeId(id.raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tekto_tree::NodeKind as K;

    #[test]
    fn builds_a_window_and_text() {
        let mut scene = SceneTree::new();
        let w = scene.add(K::Window, None).unwrap();
        scene.get_mut(w).unwrap().name = Some("Notes".into());
        let t = scene.add(K::Text, Some(w)).unwrap();
        scene.get_mut(t).unwrap().text = Some("Hello from Tekto".into());
        let update = tree_update(&scene).expect("update");
        assert_eq!(update.nodes.len(), 2);
        assert_eq!(update.focus, ak_id(w));
        assert!(update
            .nodes
            .iter()
            .any(|(_, n)| { n.label().map(|l| l.contains("Notes")).unwrap_or(false) }));
    }

    #[test]
    fn focus_follows_set_focus_and_dump_uses_same_ids() {
        let mut scene = SceneTree::new();
        let w = scene.add(K::Window, None).unwrap();
        let t = scene.add(K::Text, Some(w)).unwrap();
        scene.get_mut(t).unwrap().text = Some("Hello".into());
        scene.get_mut(t).unwrap().bounds = tekto_tree::Rect::new(10.0, 20.0, 80.0, 24.0);
        scene.set_focus(t).unwrap();
        let update = tree_update(&scene).expect("update");
        assert_eq!(update.focus, ak_id(t));
        let dump = dump(&scene).expect("dump");
        assert_eq!(dump.focus, t.raw());
        let text = dump.nodes.iter().find(|n| n.id == t.raw()).expect("text");
        assert_eq!(text.role, "Label");
        assert_eq!(text.value.as_deref(), Some("Hello"));
        assert_eq!(text.bounds, [10.0, 20.0, 90.0, 44.0]);
        assert!(dump
            .nodes
            .iter()
            .any(|n| n.id == w.raw() && n.role == "Window"));
    }

    #[test]
    fn text_field_accesskit_value_includes_preedit_then_commit() {
        let mut scene = SceneTree::new();
        let w = scene.add(K::Window, None).unwrap();
        let field = scene.add(K::TextField, Some(w)).unwrap();
        scene.get_mut(field).unwrap().name = Some("Title".into());
        scene.get_mut(field).unwrap().text = Some("Hi".into());
        scene.get_mut(field).unwrap().caret = 2;
        scene.set_focus(field).unwrap();
        scene
            .apply_ime(tekto_tree::ImeEvent::Preedit {
                text: "你".into(),
                cursor: Some((3, 3)),
            })
            .unwrap();
        let dump = dump(&scene).expect("dump");
        let node = dump.nodes.iter().find(|n| n.id == field.raw()).unwrap();
        assert_eq!(node.role, "TextInput");
        assert_eq!(node.label.as_deref(), Some("Title"));
        assert_eq!(node.value.as_deref(), Some("Hi你"));
        scene
            .apply_ime(tekto_tree::ImeEvent::Commit("你".into()))
            .unwrap();
        let dump = super::dump(&scene).expect("dump");
        let node = dump.nodes.iter().find(|n| n.id == field.raw()).unwrap();
        assert_eq!(node.value.as_deref(), Some("Hi你"));
        assert_eq!(scene.get(field).unwrap().text.as_deref(), Some("Hi你"));
    }

    #[test]
    fn multiline_text_runs_split_lines_and_span_selection() {
        let mut scene = SceneTree::new();
        let root = scene.add(K::Window, None).unwrap();
        let field = scene.add(K::TextField, Some(root)).unwrap();
        {
            let node = scene.get_mut(field).unwrap();
            node.multiline = true;
            node.text = Some(String::from("ab\ncd\n\nef"));
        }
        scene.set_field_selection(field, 1, 8).unwrap();
        let update = tree_update(&scene).unwrap();
        let parent = &update
            .nodes
            .iter()
            .find(|(id, _)| id.0 == field.raw())
            .unwrap()
            .1;
        let runs = parent.children().to_vec();
        assert_eq!(runs.len(), 4);
        let values: Vec<_> = runs
            .iter()
            .map(|r| {
                update
                    .nodes
                    .iter()
                    .find(|(id, _)| id == r)
                    .unwrap()
                    .1
                    .value()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(values, ["ab\n", "cd\n", "\n", "ef"]);
        let selection = parent.text_selection().unwrap();
        assert_eq!(selection.anchor.node, runs[0]);
        assert_eq!(selection.anchor.character_index, 1);
        assert_eq!(selection.focus.node, runs[3]);
        assert_eq!(selection.focus.character_index, 1);
    }

    #[test]
    fn multiline_caret_after_final_break_sits_on_an_empty_last_line() {
        let mut scene = SceneTree::new();
        let root = scene.add(K::Window, None).unwrap();
        let field = scene.add(K::TextField, Some(root)).unwrap();
        {
            let node = scene.get_mut(field).unwrap();
            node.multiline = true;
            node.text = Some(String::from("ab\n"));
        }
        scene.set_field_selection(field, 3, 3).unwrap();
        let update = tree_update(&scene).unwrap();
        let parent = &update
            .nodes
            .iter()
            .find(|(id, _)| id.0 == field.raw())
            .unwrap()
            .1;
        let runs = parent.children().to_vec();
        assert_eq!(runs.len(), 2);
        let selection = parent.text_selection().unwrap();
        assert_eq!(selection.focus.node, runs[1]);
        assert_eq!(selection.focus.character_index, 0);
    }

    #[test]
    fn single_line_text_run_tracks_graphemes_selection_and_composition() {
        let mut scene = SceneTree::new();
        let root = scene.add(K::Window, None).unwrap();
        let field = scene.add(K::TextField, Some(root)).unwrap();
        scene.get_mut(field).unwrap().text = Some("Ae\u{301}👩‍💻Z".into());
        scene.set_field_selection(field, 1, 3).unwrap();
        let update = tree_update(&scene).unwrap();
        let parent = &update
            .nodes
            .iter()
            .find(|(id, _)| id.0 == field.raw())
            .unwrap()
            .1;
        let run_id = parent.children()[0];
        let run = &update.nodes.iter().find(|(id, _)| *id == run_id).unwrap().1;
        assert_eq!(run.role(), Role::TextRun);
        assert_eq!(run.value(), Some("Ae\u{301}👩‍💻Z"));
        assert_eq!(run.character_lengths(), &[1, 3, 11, 1]);
        let selection = parent.text_selection().unwrap();
        assert_eq!(selection.anchor.character_index, 1);
        assert_eq!(selection.focus.character_index, 2);
        scene
            .apply_ime(tekto_tree::ImeEvent::Preedit {
                text: "aé界z".into(),
                cursor: Some((1, 6)),
            })
            .unwrap();
        let update = tree_update(&scene).unwrap();
        let parent = &update
            .nodes
            .iter()
            .find(|(id, _)| id.0 == field.raw())
            .unwrap()
            .1;
        assert_eq!(parent.children()[0], run_id);
        let selected = parent.text_selection().unwrap();
        assert_eq!(
            (
                selected.anchor.character_index,
                selected.focus.character_index
            ),
            (2, 4)
        );
        scene.apply_ime(tekto_tree::ImeEvent::Disabled).unwrap();
        let update = tree_update(&scene).unwrap();
        let parent = &update
            .nodes
            .iter()
            .find(|(id, _)| id.0 == field.raw())
            .unwrap()
            .1;
        assert_eq!(parent.children()[0], run_id);
    }

    #[test]
    fn android_overlay_value_is_the_accesskit_value() {
        let mut scene = SceneTree::new();
        let w = scene.add(K::Window, None).unwrap();
        let field = scene.add(K::TextField, Some(w)).unwrap();
        scene.get_mut(field).unwrap().name = Some("Title".into());
        scene.get_mut(field).unwrap().text = Some("".into());
        scene.set_focus(field).unwrap();
        assert!(scene.set_focused_field_value("Milk", 4).unwrap());
        let dump = dump(&scene).expect("dump");
        let node = dump.nodes.iter().find(|n| n.id == field.raw()).unwrap();
        assert_eq!(node.role, "TextInput");
        assert_eq!(node.label.as_deref(), Some("Title"));
        assert_eq!(node.value.as_deref(), Some("Milk"));
    }
}
