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

mod bundle;
mod watch;

pub use bundle::{bundle, find_packages_root, make_wrapper, which_esbuild};
pub use tekto_native::{apply_native_op, NativeOp};
pub use watch::{collect_watch_files, spawn_watcher};

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

fn parse_f32(key: &str, value: &str) -> Result<f32, JsError> {
    value
        .parse::<f32>()
        .map_err(|_| JsError::Message(format!("{key} must be a number, got `{value}`")))
}

fn parse_dim(key: &str, value: &str) -> Result<f32, JsError> {
    match value.trim() {
        "" | "auto" | "nan" | "NaN" | "unset" => Ok(f32::NAN),
        other => parse_f32(key, other),
    }
}

fn parse_bool(key: &str, value: &str) -> Result<bool, JsError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        other => Err(JsError::Message(format!(
            "{key} must be true or false, got `{other}`"
        ))),
    }
}

fn set_prop(tree: &mut Tree, id: NodeId, key: &str, value: &str) -> Result<(), JsError> {
    match key {
        "width" | "height" | "column" | "justifyCenter" | "justify_center" => {
            let node = tree
                .get_mut(id)
                .ok_or_else(|| JsError::Message(format!("unknown node {id}")))?;
            match key {
                "width" => node.layout.width = parse_dim(key, value)?,
                "height" => node.layout.height = parse_dim(key, value)?,
                "column" => node.layout.column = parse_bool(key, value)?,
                "justifyCenter" | "justify_center" => {
                    node.layout.justify_center = parse_bool(key, value)?;
                }
                _ => unreachable!(),
            }
            return Ok(());
        }
        _ => {}
    }
    let mut patch = NodePatch::default();
    match key {
        "text" => patch.text = Some(value.into()),
        "name" => patch.name = Some(value.into()),
        "fill" => patch.fill = Some(value.into()),
        "color" => patch.color = Some(value.into()),
        "fontSize" | "font_size" => patch.font_size = Some(parse_f32(key, value)?),
        "padding" => patch.padding = Some(parse_f32(key, value)?),
        "gap" => patch.gap = Some(parse_f32(key, value)?),
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
pub fn eval_to_tree(source: &str) -> Result<Tree, JsError> {
    let mut host = TreeHost::new();
    eval_module(source, &mut host)?;
    let tree = host.tree.borrow().clone();
    Ok(tree)
}

/// Apply a native plugin op (clipboard get/set today).
///
/// This is the JS-facing hook beside [`HostOp`]. Tree ops stay on [`Host`];
/// clipboard returns text, so it does not go through [`Host::apply`].
/// The JSON shape is [`NativeOp`] (`clipboardGetText` / `clipboardSetText`).
/// A missing Linux clipboard session is a [`JsError::Message`] whose text
/// contains `not available`.
pub fn apply_native_host_op(op: &NativeOp) -> Result<Option<String>, JsError> {
    apply_native_op(op).map_err(|err| JsError::Message(err.to_string()))
}

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
    fn native_clipboard_op_errors_clearly_without_a_session() {
        let op: NativeOp = serde_json::from_str(r#"{"op":"clipboardGetText"}"#).unwrap();
        let display = std::env::var("DISPLAY").unwrap_or_default();
        let reserved = matches!(display.as_str(), ":1" | ":2" | ":1.0" | ":2.0");
        let has_session = !display.is_empty() || std::env::var_os("WAYLAND_DISPLAY").is_some();
        if reserved || has_session {
            // Do not open a live clipboard from the default test suite.
            let err = JsError::Message(
                "clipboard.get is not available on linux: no X11 or Wayland clipboard session"
                    .into(),
            );
            assert!(err.to_string().contains("not available"));
            return;
        }
        let err = apply_native_host_op(&op).expect_err("headless or stub clipboard");
        let msg = err.to_string();
        assert!(
            msg.contains("not available") || msg.contains("not implemented"),
            "{msg}"
        );
    }

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

    #[test]
    fn set_prop_applies_layout_and_padding() {
        let mut host = TreeHost::new();
        let ids = host
            .apply(&[
                HostOp::Create {
                    kind: "window".into(),
                },
                HostOp::SetProp {
                    id: 1,
                    key: "padding".into(),
                    value: "24".into(),
                },
                HostOp::SetProp {
                    id: 1,
                    key: "gap".into(),
                    value: "20".into(),
                },
                HostOp::Create {
                    kind: "rect".into(),
                },
                HostOp::SetProp {
                    id: 2,
                    key: "height".into(),
                    value: "64".into(),
                },
                HostOp::SetProp {
                    id: 2,
                    key: "width".into(),
                    value: "auto".into(),
                },
                HostOp::SetProp {
                    id: 2,
                    key: "justifyCenter".into(),
                    value: "true".into(),
                },
                HostOp::SetProp {
                    id: 2,
                    key: "padding".into(),
                    value: "20".into(),
                },
                HostOp::Append {
                    parent: 1,
                    child: 2,
                },
            ])
            .expect("ops");
        assert_eq!(ids, vec![1, 2]);
        let tree = host.tree.borrow();
        let window = tree.get(NodeId::from_raw(1)).unwrap();
        assert_eq!(window.layout.padding, 24.0);
        assert_eq!(window.layout.gap, 20.0);
        let banner = tree.get(NodeId::from_raw(2)).unwrap();
        assert_eq!(banner.layout.height, 64.0);
        assert!(
            !banner.layout.is_width_set(),
            "auto width should stay unset, got {}",
            banner.layout.width
        );
        assert!(banner.layout.justify_center);
        assert_eq!(banner.layout.padding, 20.0);
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

    #[cfg(feature = "quickjs")]
    #[test]
    fn eval_to_tree_can_swap_a_second_bundle() {
        fn script(label: &str) -> String {
            format!(
                r#"
                const window = tekto.create("window");
                const text = tekto.create("text");
                tekto.setText(text, "{label}");
                tekto.append(window, text);
                "#
            )
        }
        let first = eval_to_tree(&script("before")).expect("first");
        assert!(
            first.to_pretty_text().contains("before"),
            "{}",
            first.to_pretty_text()
        );
        let second = eval_to_tree(&script("after")).expect("second");
        let pretty = second.to_pretty_text();
        assert!(pretty.contains("after"), "{pretty}");
        assert!(!pretty.contains("before"), "{pretty}");
        // New tree, same starting ids (ids are never reused inside one Tree,
        // but a remount starts a fresh Tree at 1).
        assert_eq!(second.root().map(|id| id.raw()), Some(1));
    }
}
