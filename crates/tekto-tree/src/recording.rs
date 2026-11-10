//! Portable recordings of tree input and agent actions.
use crate::{FieldKey, NodeId, NodePatch, Tree, TreeError};
use serde::{Deserialize, Serialize};

/// Version 1 records actions in the order they were applied.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    pub version: u32,
    pub events: Vec<Action>,
}

impl Default for Recording {
    fn default() -> Self {
        Self {
            version: 1,
            events: Vec::new(),
        }
    }
}

/// Tree-level equivalents of pointer, keyboard, and MCP actions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    PointerClick { x: f32, y: f32 },
    Key { key: RecordedKey },
    Click { id: NodeId },
    Type { id: NodeId, text: String },
    Patch { id: NodeId, patch: NodePatch },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "char", rename_all = "snake_case")]
pub enum RecordedKey {
    Insert(char),
    Backspace,
}

impl Action {
    /// Apply the same tree operations used by desktop input and the dev server.
    pub fn apply(&self, tree: &mut Tree) -> Result<(), TreeError> {
        match self {
            Self::PointerClick { x, y } => {
                tree.click_at(*x, *y);
                Ok(())
            }
            Self::Key { key } => {
                let key = match key {
                    RecordedKey::Insert(c) => FieldKey::Insert(*c),
                    RecordedKey::Backspace => FieldKey::Backspace,
                };
                tree.apply_field_key(key).map(|_| ())
            }
            Self::Click { id } => tree.click(*id),
            Self::Type { id, text } => tree.insert_text(*id, text),
            Self::Patch { id, patch } => tree.patch(*id, patch.clone()),
        }
    }
}

impl Recording {
    /// Apply and append a successful action to the log.
    pub fn record(&mut self, tree: &mut Tree, action: Action) -> Result<(), TreeError> {
        if self.version != 1 {
            return Err(TreeError {
                message: format!("unsupported recording version {}", self.version),
            });
        }
        action.apply(tree)?;
        self.events.push(action);
        Ok(())
    }

    /// Replay against a tree initialized and laid out like the original.
    /// Call `layout` after each action when an action can change geometry.
    pub fn replay(
        &self,
        tree: &mut Tree,
        mut layout: impl FnMut(&mut Tree),
    ) -> Result<(), TreeError> {
        if self.version != 1 {
            return Err(TreeError {
                message: format!("unsupported recording version {}", self.version),
            });
        }
        for (index, action) in self.events.iter().enumerate() {
            action.apply(tree).map_err(|e| TreeError {
                message: format!("event {index}: {e}"),
            })?;
            layout(tree);
        }
        Ok(())
    }
}
