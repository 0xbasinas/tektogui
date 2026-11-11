//! Keyed lazy rows: only the keys near the viewport exist as nodes.
//!
//! [`KeyedList`] owns a scroll viewport, a column of rows, and two spacers
//! that stand in for the keys outside the viewport. Call [`KeyedList::sync`]
//! after a scroll or a key-set change; it removes rows that left the window,
//! creates rows for keys that entered it, and keeps the id of a key that
//! stayed visible. Keys are application keys, not node ids.

use super::*;

/// One scroll viewport of keyed rows. Off-screen keys have no node at all.
pub struct KeyedList {
    pub scroll: NodeId,
    /// Column that holds the top spacer, the visible rows, and the bottom spacer.
    pub rows: NodeId,
    top: NodeId,
    bottom: NodeId,
    row_height: f32,
    visible: std::collections::BTreeMap<String, NodeId>,
}

impl KeyedList {
    /// Create an empty keyed list with a fixed viewport height.
    pub fn new(
        tree: &mut Tree,
        parent: NodeId,
        viewport_height: f32,
        row_height: f32,
    ) -> Result<Self, WidgetError> {
        if !viewport_height.is_finite() || viewport_height <= 0.0 {
            return Err(WidgetError(TreeError {
                message: "keyed list needs a finite viewport height above zero".into(),
            }));
        }
        if !row_height.is_finite() || row_height <= 0.0 {
            return Err(WidgetError(TreeError {
                message: "keyed list needs a finite row height above zero".into(),
            }));
        }
        let scroll = tree.add(NodeKind::Scroll, Some(parent))?;
        if let Some(node) = tree.get_mut(scroll) {
            node.layout.height = viewport_height;
            node.layout.column = true;
            node.layout.gap = 0.0;
            node.layout.padding = 0.0;
        }
        let rows = tree.add(NodeKind::Group, Some(scroll))?;
        if let Some(node) = tree.get_mut(rows) {
            node.layout.column = true;
            node.layout.gap = 0.0;
        }
        let top = spacer(tree, rows)?;
        let bottom = spacer(tree, rows)?;
        Ok(Self {
            scroll,
            rows,
            top,
            bottom,
            row_height,
            visible: std::collections::BTreeMap::new(),
        })
    }

    /// How many rows currently have a node.
    pub fn visible_rows(&self) -> usize {
        self.visible.len()
    }

    /// The node for a key, if that key is materialized.
    pub fn row(&self, key: &str) -> Option<NodeId> {
        self.visible.get(key).copied()
    }

    /// Scroll so `key` sits at the top of the viewport. Returns false when the
    /// key is not in `keys`.
    pub fn scroll_to_key(&mut self, tree: &mut Tree, keys: &[String], key: &str) -> bool {
        let Some(index) = keys.iter().position(|candidate| candidate == key) else {
            return false;
        };
        let offset = index as f32 * self.row_height;
        tree.set_scroll_y(self.scroll, offset).is_ok()
    }

    /// Drop rows outside the window and create rows for keys inside it.
    ///
    /// `overscan` is extra pixels above and below the viewport. `build` makes
    /// one row under `self.rows`; it should return a row node. Returns the
    /// number of rows that now have a node.
    pub fn sync(
        &mut self,
        tree: &mut Tree,
        keys: &[String],
        overscan: f32,
        build: &mut dyn FnMut(&mut Tree, NodeId, &str) -> Result<NodeId, WidgetError>,
    ) -> Result<usize, WidgetError> {
        let mut seen = std::collections::BTreeSet::new();
        for key in keys {
            if !seen.insert(key.as_str()) {
                return Err(WidgetError(TreeError {
                    message: format!("duplicate keyed row `{key}`"),
                }));
            }
        }
        let overscan = if overscan.is_finite() {
            overscan.max(0.0)
        } else {
            0.0
        };
        let viewport = self.viewport(tree);
        let scroll_y = tree
            .get(self.scroll)
            .map(|node| node.scroll_y)
            .unwrap_or(0.0);
        let first = (((scroll_y - overscan) / self.row_height).floor() as isize).max(0) as usize;
        let last =
            (((scroll_y + viewport + overscan) / self.row_height).ceil() as usize).min(keys.len());
        if first >= last {
            // Nothing is visible; drop every row.
            let stale: Vec<NodeId> = self.visible.values().copied().collect();
            self.visible.clear();
            for id in stale {
                let _ = tree.remove(id);
            }
            set_height(tree, self.top, 0.0);
            set_height(tree, self.bottom, keys.len() as f32 * self.row_height);
            return Ok(0);
        }
        let wanted = &keys[first..last];
        let stale: Vec<NodeId> = self
            .visible
            .iter()
            .filter(|(key, _)| {
                !wanted
                    .iter()
                    .any(|candidate| candidate.as_str() == key.as_str())
            })
            .map(|(_, id)| *id)
            .collect();
        if let Some(node) = tree.get_mut(self.rows) {
            node.children.retain(|child| !stale.contains(child));
        }
        for (key, id) in self.visible.clone() {
            if stale.contains(&id) {
                self.visible.remove(&key);
                let _ = tree.remove(id);
            }
        }
        for (offset, key) in wanted.iter().enumerate() {
            if self.visible.contains_key(key) {
                continue;
            }
            let id = build(tree, self.rows, key)?;
            let index = 1 + offset;
            tree.insert_child_at(self.rows, index, id)?;
            self.visible.insert(key.clone(), id);
        }
        set_height(tree, self.top, first as f32 * self.row_height);
        set_height(
            tree,
            self.bottom,
            (keys.len().saturating_sub(last)) as f32 * self.row_height,
        );
        Ok(self.visible.len())
    }

    fn viewport(&self, tree: &Tree) -> f32 {
        tree.get(self.scroll)
            .map(|node| {
                if node.bounds.height.is_finite() && node.bounds.height > 0.0 {
                    node.bounds.height
                } else {
                    node.layout.height
                }
            })
            .unwrap_or(0.0)
    }
}

fn spacer(tree: &mut Tree, parent: NodeId) -> Result<NodeId, WidgetError> {
    let id = tree.add(NodeKind::Rect, Some(parent))?;
    if let Some(node) = tree.get_mut(id) {
        node.layout.height = 0.0;
    }
    Ok(id)
}

fn set_height(tree: &mut Tree, id: NodeId, height: f32) {
    if let Some(node) = tree.get_mut(id) {
        node.layout.height = height.max(0.0);
    }
}
