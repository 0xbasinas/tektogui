//! JS / TypeScript host.
//!
//! Desktop embeds QuickJS-ng ([`rquickjs`], MIT) behind the `quickjs`
//! feature, so everyday Rust builds stay fast. On the web, this crate is
//! not used as an engine: the TypeScript app runs in the browser and talks
//! to a WASM build of the Rust core through the same [`HostOp`] protocol.

use std::cell::RefCell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use tekto_tree::{NodeId, NodeKind, NodePatch, Tree, TreeError};
use thiserror::Error;

#[cfg(feature = "quickjs")]
mod engine;

#[derive(Debug, Error)]
pub enum JsError {
    #[error("{0}")]
    Message(String),
    #[error("the QuickJS engine is not enabled; build with --features quickjs (desktop) or use the browser engine on web")]
    EngineDisabled,
}

impl From<TreeError> for JsError {
    fn from(value: TreeError) -> Self {
        JsError::Message(value.message)
    }
}

/// One mutation sent from the TS signals / JSX runtime into the Rust tree.
///
/// The desktop QuickJS host and the future web WASM glue both use this
/// surface so app code stays the same.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum HostOp {
    Create { kind: String },
    SetText { id: u64, text: String },
    SetProp { id: u64, key: String, value: String },
    Append { parent: u64, child: u64 },
    Remove { id: u64 },
}

/// Host that applies ops. The desktop QuickJS runtime and the web WASM
/// glue both call this.
pub trait Host {
    /// Apply `ops` in order. For each [`HostOp::Create`], the new stable
    /// id is pushed onto the returned list (other ops are not).
    fn apply(&mut self, ops: &[HostOp]) -> Result<Vec<u64>, JsError>;
}

/// A [`Host`] that writes into a Tekto [`Tree`].
#[derive(Clone, Default)]
pub struct TreeHost {
    pub tree: Rc<RefCell<Tree>>,
}

impl TreeHost {
    pub fn new() -> Self {
        Self {
            tree: Rc::new(RefCell::new(Tree::new())),
        }
    }

    pub fn from_tree(tree: Tree) -> Self {
        Self {
            tree: Rc::new(RefCell::new(tree)),
        }
    }
}

impl Host for TreeHost {
    fn apply(&mut self, ops: &[HostOp]) -> Result<Vec<u64>, JsError> {
        apply_ops(&mut self.tree.borrow_mut(), ops)
    }
}

/// Apply [`HostOp`]s to a tree. Used by [`TreeHost`] and by the web path later.
pub fn apply_ops(tree: &mut Tree, ops: &[HostOp]) -> Result<Vec<u64>, JsError> {
    let mut created = Vec::new();
    for op in ops {
        match op {
            HostOp::Create { kind } => {
                let kind = NodeKind::from_name(kind)
                    .ok_or_else(|| JsError::Message(format!("unknown node kind `{kind}`")))?;
                let id = tree.add(kind, None)?;
                created.push(id.raw());
            }
            HostOp::SetText { id, text } => {
                tree.patch(
                    NodeId::from_raw(*id),
                    NodePatch {
                        text: Some(text.clone()),
                        ..NodePatch::default()
                    },
                )?;
            }
            HostOp::SetProp { id, key, value } => {
                set_prop(tree, NodeId::from_raw(*id), key, value)?;
            }
            HostOp::Append { parent, child } => {
                tree.append(NodeId::from_raw(*parent), NodeId::from_raw(*child))?;
            }
            HostOp::Remove { id } => {
                tree.remove(NodeId::from_raw(*id))?;
            }
        }
    }
    Ok(created)
}

fn set_prop(tree: &mut Tree, id: NodeId, key: &str, value: &str) -> Result<(), JsError> {
    let mut patch = NodePatch::default();
    match key {
        "text" => patch.text = Some(value.into()),
        "name" => patch.name = Some(value.into()),
        "fill" => patch.fill = Some(value.into()),
        "color" => patch.color = Some(value.into()),
        "fontSize" | "font_size" => {
            let size = value.parse::<f32>().map_err(|_| {
                JsError::Message(format!("fontSize must be a number, got `{value}`"))
            })?;
            patch.font_size = Some(size);
        }
        other => {
            return Err(JsError::Message(format!("unknown prop `{other}`")));
        }
    }
    tree.patch(id, patch)?;
    Ok(())
}

/// Run a JS script against `host`.
///
/// When the `quickjs` feature is on, the script sees a global `tekto`
/// object: `create`, `setText`, `setProp`, `append`, `remove`. Those
/// call the same [`HostOp`] protocol the web target will use.
///
/// Without the feature this returns [`JsError::EngineDisabled`].
pub fn eval_module(source: &str, host: &mut dyn Host) -> Result<(), JsError> {
    #[cfg(feature = "quickjs")]
    {
        engine::eval_script(source, host)
    }
    #[cfg(not(feature = "quickjs"))]
    {
        let _ = source;
        let _ = host;
        Err(JsError::EngineDisabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_is_disabled_by_default() {
        if cfg!(feature = "quickjs") {
            return;
        }
        let err = eval_module("export {}", &mut TreeHost::new()).unwrap_err();
        assert!(matches!(err, JsError::EngineDisabled));
    }

    #[test]
    fn apply_ops_creates_a_text_node() {
        let mut host = TreeHost::new();
        let ids = host
            .apply(&[
                HostOp::Create {
                    kind: "window".into(),
                },
                HostOp::Create {
                    kind: "text".into(),
                },
                HostOp::SetText {
                    id: 2,
                    text: "Hello from Tekto".into(),
                },
                HostOp::Append {
                    parent: 1,
                    child: 2,
                },
            ])
            .expect("ops");
        assert_eq!(ids, vec![1, 2]);
        let tree = host.tree.borrow();
        let pretty = tree.to_pretty_text();
        assert!(pretty.contains("Hello from Tekto"), "{pretty}");
        assert_eq!(tree.root().map(|id| id.raw()), Some(1));
        assert_eq!(
            tree.get(NodeId::from_raw(2))
                .and_then(|n| n.text.clone())
                .as_deref(),
            Some("Hello from Tekto")
        );
    }

    #[cfg(feature = "quickjs")]
    #[test]
    fn js_script_creates_a_text_node_through_ops() {
        let mut host = TreeHost::new();
        eval_module(
            r#"
            const window = tekto.create("window");
            const text = tekto.create("text");
            tekto.setText(text, "Hello from Tekto");
            tekto.append(window, text);
            "#,
            &mut host,
        )
        .expect("eval");
        let tree = host.tree.borrow();
        let pretty = tree.to_pretty_text();
        assert!(pretty.contains("window#1"), "{pretty}");
        assert!(pretty.contains("Hello from Tekto"), "{pretty}");
    }
}
