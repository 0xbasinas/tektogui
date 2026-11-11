use tekto_tree::{route_named_key, FieldKey, NamedActionKey, NodeId, Tree};
pub use winit::event::ElementState;
pub use winit::keyboard::{Key, ModifiersState, NamedKey};

pub fn route_key(
    tree: &mut Tree,
    key: &Key,
    text: Option<&str>,
    state: ElementState,
    repeat: bool,
    mods: ModifiersState,
    last_ime_commit: &mut Option<String>,
) -> (bool, Option<NodeId>) {
    if state != ElementState::Pressed {
        return (false, None);
    }
    let chord = mods.control_key() || mods.alt_key() || mods.super_key();
    if !chord && *key == Key::Named(NamedKey::Escape) {
        return route_named_key(tree, NamedActionKey::Escape, repeat);
    }
    if !chord && *key == Key::Named(NamedKey::Tab) {
        return route_named_key(
            tree,
            if mods.shift_key() {
                NamedActionKey::ShiftTab
            } else {
                NamedActionKey::Tab
            },
            repeat,
        );
    }
    if !chord
        && (key == &Key::Named(NamedKey::Enter)
            || key == &Key::Named(NamedKey::Space)
            || matches!(key, Key::Character(value) if value == " "))
    {
        let action = if *key == Key::Named(NamedKey::Enter) {
            NamedActionKey::Enter
        } else {
            NamedActionKey::Space
        };
        let result = route_named_key(tree, action, repeat);
        if tree.focus.is_some_and(|id| {
            tree.get(id).is_some_and(|n| {
                matches!(
                    n.kind,
                    tekto_tree::NodeKind::Button
                        | tekto_tree::NodeKind::Checkbox
                        | tekto_tree::NodeKind::Switch
                        | tekto_tree::NodeKind::Radio
                        | tekto_tree::NodeKind::Tab
                        | tekto_tree::NodeKind::MenuItem
                        | tekto_tree::NodeKind::Select
                )
            })
        }) {
            return result;
        }
    }
    if *key == Key::Named(NamedKey::Backspace) {
        *last_ime_commit = None;
        return route_named_key(tree, NamedActionKey::Backspace, repeat);
    }
    if (mods.control_key() || mods.super_key())
        && !mods.alt_key()
        && matches!(key, Key::Character(value) if value.eq_ignore_ascii_case("a"))
    {
        return (
            tree.apply_field_key(FieldKey::SelectAll).unwrap_or(false),
            None,
        );
    }
    if chord {
        return (false, None);
    }
    if let Some(id) = tree.focus.filter(|id| {
        tree.get(*id)
            .is_some_and(|node| node.kind == tekto_tree::NodeKind::TextField && node.multiline)
    }) {
        match key {
            Key::Named(NamedKey::Enter) => {
                return (tree.type_keyboard_text("\n").unwrap_or(false), None)
            }
            Key::Named(NamedKey::ArrowUp) => {
                return (
                    tekto_layout::field_move_lines(tree, id, -1, mods.shift_key()),
                    None,
                )
            }
            Key::Named(NamedKey::ArrowDown) => {
                return (
                    tekto_layout::field_move_lines(tree, id, 1, mods.shift_key()),
                    None,
                )
            }
            _ => {}
        }
    }
    if tree.focus.and_then(|id| tree.get(id)).is_some_and(|node| {
        matches!(
            node.kind,
            tekto_tree::NodeKind::Slider
                | tekto_tree::NodeKind::Radio
                | tekto_tree::NodeKind::Tab
                | tekto_tree::NodeKind::MenuItem
        )
    }) {
        let action = match key {
            Key::Named(NamedKey::ArrowLeft) => Some(NamedActionKey::ArrowLeft),
            Key::Named(NamedKey::ArrowRight) => Some(NamedActionKey::ArrowRight),
            Key::Named(NamedKey::ArrowUp) => Some(NamedActionKey::ArrowUp),
            Key::Named(NamedKey::ArrowDown) => Some(NamedActionKey::ArrowDown),
            Key::Named(NamedKey::Home) => Some(NamedActionKey::Home),
            Key::Named(NamedKey::End) => Some(NamedActionKey::End),
            _ => None,
        };
        if let Some(action) = action {
            return route_named_key(tree, action, repeat);
        }
    }
    if mods.shift_key() {
        let selection = match key {
            Key::Named(NamedKey::ArrowLeft) => Some(FieldKey::SelectLeft),
            Key::Named(NamedKey::ArrowRight) => Some(FieldKey::SelectRight),
            Key::Named(NamedKey::Home) => Some(FieldKey::SelectHome),
            Key::Named(NamedKey::End) => Some(FieldKey::SelectEnd),
            _ => None,
        };
        if let Some(key) = selection {
            return (tree.apply_field_key(key).unwrap_or(false), None);
        }
    }
    let field_key = match key {
        Key::Named(NamedKey::ArrowLeft) => Some(FieldKey::Left),
        Key::Named(NamedKey::ArrowRight) => Some(FieldKey::Right),
        Key::Named(NamedKey::Home) => Some(FieldKey::Home),
        Key::Named(NamedKey::End) => Some(FieldKey::End),
        Key::Named(NamedKey::Delete) => Some(FieldKey::Delete),
        _ => None,
    };
    if let Some(key) = field_key {
        *last_ime_commit = None;
        return (tree.apply_field_key(key).unwrap_or(false), None);
    }
    let Some(text) = text else {
        return (false, None);
    };
    let plain: String = text.chars().filter(|c| !c.is_control()).collect();
    if !crate::accept_keyboard_text(last_ime_commit, &plain) {
        return (false, None);
    }
    (tree.type_keyboard_text(&plain).unwrap_or(false), None)
}

/// Apply clipboard shortcuts through the same field editing methods.
/// A failed clipboard write never cuts the selection.
pub fn route_clipboard(
    tree: &mut Tree,
    clipboard: &dyn tekto_native::Clipboard,
    key: &Key,
    state: ElementState,
    mods: ModifiersState,
) -> Result<Option<bool>, String> {
    if state != ElementState::Pressed || !(mods.control_key() || mods.super_key()) || mods.alt_key()
    {
        return Ok(None);
    }
    let Key::Character(key) = key else {
        return Ok(None);
    };
    let operation = key.to_lowercase();
    if !matches!(operation.as_str(), "c" | "x" | "v") {
        return Ok(None);
    }
    let Some(id) = tree.focus else {
        return Ok(None);
    };
    if tree.get(id).is_none_or(|n| {
        n.kind != tekto_tree::NodeKind::TextField || n.disabled || !n.preedit.is_empty()
    }) {
        return Ok(None);
    }
    if operation == "v" {
        let value = clipboard.get_text().map_err(|e| e.to_string())?;
        let before = tree
            .get(id)
            .and_then(|n| n.text.clone())
            .unwrap_or_default();
        tree.insert_text(id, &value).map_err(|e| e.to_string())?;
        return Ok(Some(
            tree.get(id).and_then(|n| n.text.as_deref()) != Some(before.as_str()),
        ));
    }
    let selected = tree.selected_text(id).map_err(|e| e.to_string())?;
    if selected.is_empty() {
        return Ok(Some(false));
    }
    clipboard.set_text(&selected).map_err(|e| e.to_string())?;
    if operation == "x" {
        tree.apply_field_key(FieldKey::Delete)
            .map_err(|e| e.to_string())?;
        return Ok(Some(true));
    }
    Ok(Some(false))
}

/// Route a keyboard event through the live action gate and callback delivery.
pub fn deliver_key(
    handle: &tekto_mcp::SharedTreeHandle,
    key: &Key,
    text: Option<&str>,
    state: ElementState,
    repeat: bool,
    mods: ModifiersState,
    echo: &mut Option<String>,
) -> Result<bool, tekto_mcp::McpError> {
    handle.route_activated_action(|tree| {
        if let Some(changed) = route_clipboard(tree, &tekto_native::native(), key, state, mods)
            .map_err(tekto_mcp::McpError::msg)?
        {
            return Ok((changed, None));
        }
        let (changed, id) = route_key(tree, key, text, state, repeat, mods, echo);
        if let Some(id) = id {
            tree.click(id)
                .map_err(|e| tekto_mcp::McpError::msg(e.to_string()))?;
        }
        Ok((changed, id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tekto_tree::{ImeEvent, NodeKind};

    #[test]
    fn multiline_enter_and_vertical_keys_edit_the_focused_field() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        let node = tree.get_mut(field).unwrap();
        node.multiline = true;
        node.layout.width = 120.0;
        node.layout.height = 80.0;
        tree.insert_text(field, "first\nsecond").unwrap();
        tekto_layout::layout_tree(&mut tree, 200.0, 120.0);
        let mut echo = None;
        let route = |tree: &mut Tree, key, mods, echo: &mut Option<String>| {
            route_key(
                tree,
                &Key::Named(key),
                None,
                ElementState::Pressed,
                false,
                mods,
                echo,
            )
            .0
        };
        assert!(route(
            &mut tree,
            NamedKey::ArrowUp,
            ModifiersState::empty(),
            &mut echo
        ));
        assert!(tree.get(field).unwrap().caret < 6);
        assert!(route(
            &mut tree,
            NamedKey::ArrowDown,
            ModifiersState::SHIFT,
            &mut echo
        ));
        assert!(tree.get(field).unwrap().selection().is_some());
        assert!(route(
            &mut tree,
            NamedKey::Enter,
            ModifiersState::empty(),
            &mut echo
        ));
        assert!(tree
            .get(field)
            .unwrap()
            .text
            .as_deref()
            .unwrap()
            .contains('\n'));
    }

    #[test]
    fn clipboard_copy_cut_paste_and_failed_cut() {
        use std::sync::Mutex;
        struct Clipboard {
            text: Mutex<String>,
            fail: bool,
        }
        impl tekto_native::Clipboard for Clipboard {
            fn get_text(&self) -> Result<String, tekto_native::NativeError> {
                Ok(self.text.lock().unwrap().clone())
            }
            fn set_text(&self, text: &str) -> Result<(), tekto_native::NativeError> {
                if self.fail {
                    return Err(tekto_native::NativeError::Message("unavailable".into()));
                }
                *self.text.lock().unwrap() = text.into();
                Ok(())
            }
        }
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        tree.get_mut(field).unwrap().text = Some("A👩‍💻B".into());
        tree.set_field_selection(field, 1, 4).unwrap();
        let clipboard = Clipboard {
            text: Mutex::new(String::new()),
            fail: false,
        };
        let call = |tree: &mut Tree, clip: &Clipboard, key: &str| {
            route_clipboard(
                tree,
                clip,
                &Key::Character(key.into()),
                ElementState::Pressed,
                ModifiersState::CONTROL,
            )
        };
        assert_eq!(call(&mut tree, &clipboard, "c").unwrap(), Some(false));
        assert_eq!(*clipboard.text.lock().unwrap(), "👩‍💻");
        let failed = Clipboard {
            text: Mutex::new(String::new()),
            fail: true,
        };
        assert!(call(&mut tree, &failed, "x").is_err());
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("A👩‍💻B"));
        assert_eq!(call(&mut tree, &clipboard, "x").unwrap(), Some(true));
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("AB"));
        assert_eq!(call(&mut tree, &clipboard, "v").unwrap(), Some(true));
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("A👩‍💻B"));
    }

    #[test]
    fn shift_navigation_selects_text() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        tree.get_mut(field).unwrap().text = Some("abc".into());
        tree.get_mut(field).unwrap().caret = 3;
        tree.set_focus(field).unwrap();
        let mut echo = None;
        assert!(
            route_key(
                &mut tree,
                &Key::Named(NamedKey::ArrowLeft),
                None,
                ElementState::Pressed,
                false,
                ModifiersState::SHIFT,
                &mut echo
            )
            .0
        );
        assert_eq!(tree.selected_text(field).unwrap(), "c");
        assert!(
            route_key(
                &mut tree,
                &Key::Character("a".into()),
                None,
                ElementState::Pressed,
                false,
                ModifiersState::CONTROL,
                &mut echo
            )
            .0
        );
        assert_eq!(tree.selected_text(field).unwrap(), "abc");
    }

    #[test]
    fn routes_field_navigation_and_delete() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let field = tree.add(NodeKind::TextField, Some(root)).unwrap();
        tree.get_mut(field).unwrap().text = Some("aé你".into());
        tree.get_mut(field).unwrap().caret = 3;
        tree.set_focus(field).unwrap();
        let mut echo = None;
        for key in [
            NamedKey::Home,
            NamedKey::ArrowRight,
            NamedKey::Delete,
            NamedKey::End,
            NamedKey::ArrowLeft,
        ] {
            assert!(
                route_key(
                    &mut tree,
                    &Key::Named(key),
                    None,
                    ElementState::Pressed,
                    false,
                    ModifiersState::empty(),
                    &mut echo
                )
                .0
            );
        }
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("a你"));
        assert_eq!(tree.get(field).unwrap().caret, 1);
        assert!(
            !route_key(
                &mut tree,
                &Key::Named(NamedKey::Delete),
                None,
                ElementState::Released,
                false,
                ModifiersState::empty(),
                &mut echo
            )
            .0
        );
        assert!(
            !route_key(
                &mut tree,
                &Key::Named(NamedKey::Home),
                None,
                ElementState::Pressed,
                false,
                ModifiersState::CONTROL,
                &mut echo
            )
            .0
        );
        tree.apply_ime(ImeEvent::Preedit {
            text: "文".into(),
            cursor: None,
        })
        .unwrap();
        assert!(
            !route_key(
                &mut tree,
                &Key::Named(NamedKey::Delete),
                None,
                ElementState::Pressed,
                false,
                ModifiersState::empty(),
                &mut echo
            )
            .0
        );
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("a你"));
    }

    #[test]
    fn routing_focus_activation_and_ime() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let group = tree.add(NodeKind::Group, Some(root)).unwrap();
        let field = tree.add(NodeKind::TextField, Some(group)).unwrap();
        let button = tree.add(NodeKind::Button, Some(group)).unwrap();
        let last = tree.add(NodeKind::Button, Some(root)).unwrap();
        let mut echo = None;
        let press = ElementState::Pressed;
        let tab = Key::Named(NamedKey::Tab);
        let mut route = |tree: &mut Tree, key: &Key, text, state, repeat, mods| {
            let (changed, click) = route_key(tree, key, text, state, repeat, mods, &mut echo);
            changed || click.is_some_and(|id| tree.click(id).is_ok())
        };
        assert!(route(
            &mut tree,
            &tab,
            None,
            press,
            false,
            ModifiersState::empty()
        ));
        assert_eq!(tree.focus, Some(field));
        assert_eq!(tekto_a11y::tree_update(&tree).unwrap().focus.0, field.raw());
        assert!(route(
            &mut tree,
            &Key::Character("x".into()),
            Some("x"),
            press,
            false,
            ModifiersState::empty()
        ));
        assert!(route(
            &mut tree,
            &Key::Character(" ".into()),
            Some(" "),
            press,
            false,
            ModifiersState::empty()
        ));
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("x "));
        tree.apply_ime(ImeEvent::Preedit {
            text: "あ".into(),
            cursor: Some((0, 3)),
        })
        .unwrap();
        assert!(!route(
            &mut tree,
            &Key::Character("y".into()),
            Some("y"),
            press,
            false,
            ModifiersState::empty()
        ));
        assert_eq!(tree.get(field).unwrap().text.as_deref(), Some("x "));
        assert!(route(
            &mut tree,
            &tab,
            None,
            press,
            false,
            ModifiersState::empty()
        ));
        assert_eq!(tree.focus, Some(button));
        assert!(tree.get(field).unwrap().preedit.is_empty());
        assert_eq!(
            tekto_a11y::tree_update(&tree).unwrap().focus.0,
            button.raw()
        );
        for key in [
            Key::Named(NamedKey::Enter),
            Key::Character(" ".into()),
            Key::Named(NamedKey::Space),
        ] {
            tree.last_click = None;
            assert!(route(
                &mut tree,
                &key,
                Some(" "),
                press,
                false,
                ModifiersState::empty()
            ));
            assert_eq!(tree.last_click, Some(button));
            tree.last_click = None;
            assert!(!route(
                &mut tree,
                &key,
                Some(" "),
                press,
                true,
                ModifiersState::empty()
            ));
            assert!(!route(
                &mut tree,
                &key,
                Some(" "),
                ElementState::Released,
                false,
                ModifiersState::empty()
            ));
            assert_eq!(tree.last_click, None);
        }
        assert!(!route(
            &mut tree,
            &tab,
            None,
            press,
            false,
            ModifiersState::CONTROL
        ));
        assert!(route(
            &mut tree,
            &tab,
            None,
            press,
            false,
            ModifiersState::SHIFT
        ));
        assert_eq!(tree.focus, Some(field));
        assert!(route(
            &mut tree,
            &tab,
            None,
            press,
            false,
            ModifiersState::SHIFT
        ));
        assert_eq!(tree.focus, Some(last));
        assert!(route(
            &mut tree,
            &tab,
            None,
            press,
            false,
            ModifiersState::empty()
        ));
        assert_eq!(tree.focus, Some(field));
        tree.remove(field).unwrap();
        assert_eq!(tree.focus, Some(root));
        assert!(route(
            &mut tree,
            &tab,
            None,
            press,
            false,
            ModifiersState::empty()
        ));
        assert_eq!(tree.focus, Some(button));
        tree.remove(group).unwrap();
        tree.remove(last).unwrap();
        assert!(!route(
            &mut tree,
            &tab,
            None,
            press,
            false,
            ModifiersState::empty()
        ));
        assert_eq!(tree.focus, Some(root));
        tree.remove(root).unwrap();
        assert!(!route(
            &mut tree,
            &tab,
            None,
            press,
            false,
            ModifiersState::empty()
        ));
        assert_eq!(tree.focus, None);
    }
}
