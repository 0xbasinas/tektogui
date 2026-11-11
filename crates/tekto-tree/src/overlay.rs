//! Anchored overlays: menus, selects, and modal dialogs above the window.
use super::*;

impl Tree {
    /// Return the open overlay, if any.
    pub fn overlay(&self) -> Option<Overlay> {
        self.overlay
    }

    /// Attach an existing subtree to the window as an anchored overlay.
    /// Opening a replacement closes the previous overlay and removes its subtree.
    pub fn open_overlay(
        &mut self,
        node: NodeId,
        anchor: NodeId,
        side: OverlaySide,
    ) -> Result<(), TreeError> {
        self.layout_dirty = true;
        let root = self.root.ok_or_else(|| TreeError {
            message: "no window root".into(),
        })?;
        if self.get(root).is_none_or(|n| n.kind != NodeKind::Window)
            || node == root
            || node == anchor
            || !self.nodes.contains_key(&node)
            || !self.nodes.contains_key(&anchor)
            || !self.is_descendant(anchor, root)
            || self.is_descendant(anchor, node)
            || self.overlay.is_some_and(|old| {
                self.is_descendant(node, old.node) || self.is_descendant(anchor, old.node)
            })
        {
            return Err(TreeError {
                message: "invalid overlay or anchor".into(),
            });
        }
        let is_dialog = self.nodes[&node].kind == NodeKind::Dialog;
        let needs_name = matches!(self.nodes[&node].kind, NodeKind::Dialog | NodeKind::Menu);
        if needs_name
            && self.nodes[&node]
                .name
                .as_deref()
                .is_none_or(|name| name.trim().is_empty())
        {
            return Err(TreeError {
                message: "dialog or menu requires an accessible name".into(),
            });
        }
        self.close_overlay();
        let previous_focus = self.focus;
        self.append(root, node)?;
        self.overlay = Some(Overlay {
            node,
            anchor,
            side,
            position: None,
            previous_focus,
        });
        self.set_focus(node)?;
        if is_dialog || self.get(node).is_some_and(|n| n.kind == NodeKind::Menu) {
            self.traverse_focus(false);
        }
        Ok(())
    }

    /// Associate a detached, named menu with an attached target. The menu is
    /// owned by the target and is kept for subsequent context-menu gestures.
    pub fn bind_context_menu(&mut self, target: NodeId, menu: NodeId) -> Result<(), TreeError> {
        let valid_target = self.get(target).is_some_and(|n| !n.disabled);
        let valid_menu = self.get(menu).is_some_and(|n| {
            n.kind == NodeKind::Menu
                && n.parent.is_none()
                && n.name
                    .as_deref()
                    .is_some_and(|name| !name.trim().is_empty())
        });
        if !valid_target
            || !valid_menu
            || target == menu
            || self.context_menus.contains_key(&target)
            || self.context_menus.values().any(|id| *id == menu)
        {
            return Err(TreeError {
                message: "invalid or already bound context menu".into(),
            });
        }
        self.context_menus.insert(target, menu);
        Ok(())
    }

    /// Open the nearest bound context menu at the pointer. Returns its target.
    pub fn context_menu_at(&mut self, x: f32, y: f32) -> Option<NodeId> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        if self.overlay.is_some() {
            self.close_overlay();
        }
        let mut cursor = self.hit(x, y);
        while let Some(id) = cursor {
            let node = self.get(id)?;
            if node.disabled {
                return None;
            }
            let parent = node.parent;
            if let Some(menu) = self.context_menus.get(&id).copied() {
                self.open_overlay(menu, id, OverlaySide::Below).ok()?;
                self.overlay.as_mut()?.position = Some((x, y));
                return Some(id);
            }
            cursor = parent;
        }
        None
    }

    /// Close and remove the overlay, restoring focus if possible.
    pub fn close_overlay(&mut self) -> bool {
        self.layout_dirty = true;
        let Some(overlay) = self.overlay.take() else {
            return false;
        };
        if self
            .get(overlay.anchor)
            .is_some_and(|node| node.kind == NodeKind::Select)
            && self
                .get(overlay.node)
                .is_some_and(|node| node.kind == NodeKind::Menu)
        {
            let options = self
                .get(overlay.node)
                .map(|node| node.children.clone())
                .unwrap_or_default();
            for option in options {
                let _ = self.append(overlay.anchor, option);
            }
        }
        if self.context_menus.get(&overlay.anchor) == Some(&overlay.node) {
            if let Some(parent) = self.get(overlay.node).and_then(|node| node.parent) {
                if let Some(parent) = self.nodes.get_mut(&parent) {
                    parent.children.retain(|id| *id != overlay.node);
                }
            }
            if let Some(node) = self.nodes.get_mut(&overlay.node) {
                node.parent = None;
            }
        } else {
            let _ = self.remove(overlay.node);
        }
        if let Some(focus) = overlay.previous_focus {
            if self.set_focus(focus).is_err() {
                if let Some(root) = self.root {
                    let _ = self.set_focus(root);
                }
            }
        }
        true
    }
}
