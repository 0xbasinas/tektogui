use tekto_tree::{FieldKey, NodeId, NodeKind, Tree};
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
    if !chord && *key == Key::Named(NamedKey::Tab) {
        return (tree.traverse_focus(mods.shift_key()), None);
    }
    let button = tree
        .focus
        .filter(|id| tree.get(*id).is_some_and(|n| n.kind == NodeKind::Button));
    if !chord
        && (key == &Key::Named(NamedKey::Enter)
            || key == &Key::Named(NamedKey::Space)
            || matches!(key, Key::Character(value) if value == " "))
    {
        if let Some(id) = button {
            return (false, (!repeat).then_some(id));
        }
    }
    if *key == Key::Named(NamedKey::Backspace) {
        *last_ime_commit = None;
        return (
            tree.apply_field_key(FieldKey::Backspace).unwrap_or(false),
            None,
        );
    }
    if chord {
        return (false, None);
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
    handle.route_action(|tree| route_key(tree, key, text, state, repeat, mods, echo))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tekto_tree::ImeEvent;

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
