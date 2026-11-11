//! Tab and panel binding, and the visibility test that follows it.
use super::*;

impl Tree {
    /// Associate one tab with one panel. Panels and their state stay in the tree.
    pub fn bind_tab_panel(&mut self, tab: NodeId, panel: NodeId) -> Result<(), TreeError> {
        if self.get(tab).is_none_or(|n| n.kind != NodeKind::Tab)
            || self.get(panel).is_none_or(|n| n.kind != NodeKind::TabPanel)
            || self.tab_panels.contains_key(&tab)
            || self.tab_panels.values().any(|id| *id == panel)
            || self.is_descendant(tab, panel)
            || self.is_descendant(panel, tab)
        {
            return Err(TreeError {
                message: "invalid or already bound tab panel".into(),
            });
        }
        self.tab_panels.insert(tab, panel);
        if self.focus.is_some_and(|focus| !self.is_visible(focus)) {
            let _ = self.set_focus(tab);
        }
        Ok(())
    }

    pub fn panel_for_tab(&self, tab: NodeId) -> Option<NodeId> {
        self.tab_panels.get(&tab).copied()
    }
    pub fn tab_for_panel(&self, panel: NodeId) -> Option<NodeId> {
        self.tab_panels
            .iter()
            .find_map(|(tab, id)| (*id == panel).then_some(*tab))
    }

    /// Whether a node and its ancestors are in active tab panels and not
    /// inside a collapsed responsive pane.
    pub fn is_visible(&self, mut id: NodeId) -> bool {
        loop {
            let Some(node) = self.get(id) else {
                return false;
            };
            if node.collapsed {
                return false;
            }
            if let Some(tab) = self.tab_for_panel(id) {
                if self.get(tab).is_none_or(|n| !n.checked) {
                    return false;
                }
            }
            match node.parent {
                Some(parent) => id = parent,
                None => return true,
            }
        }
    }
}
