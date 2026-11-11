//! JS / TypeScript host.
//!
//! Desktop embeds QuickJS-ng ([`rquickjs`], MIT) behind the `quickjs`
//! feature, so everyday Rust builds stay fast. On the web, this crate is
//! not used as an engine: the TypeScript app runs in the browser and talks
//! to a WASM build of the Rust core through the same [`HostOp`] protocol.

use std::cell::RefCell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use tekto_tree::{NodeId, NodeKind, NodePatch, OverlaySide, StyleTransition, Tree, TreeError};
use thiserror::Error;

#[cfg(feature = "quickjs")]
mod engine;

#[cfg(feature = "quickjs")]
pub use engine::JsSession;

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
    Create {
        kind: String,
    },
    SetText {
        id: u64,
        text: String,
    },
    SetProp {
        id: u64,
        key: String,
        value: String,
    },
    Append {
        parent: u64,
        child: u64,
    },
    Remove {
        id: u64,
    },
    /// Open a window overlay anchored to a node.
    OpenOverlay {
        id: u64,
        anchor: u64,
        side: String,
    },
    /// Associate a detached menu with a target for context-menu gestures.
    BindTabPanel {
        tab: u64,
        panel: u64,
    },
    BindContextMenu {
        target: u64,
        menu: u64,
    },
    /// Close the current overlay, if one is open.
    CloseOverlay,
    /// Start a quiet style tween. The window loop calls [`Tree::advance`].
    /// Headless callers advance time themselves. Numbers are decimal strings
    /// so this op stays `Eq`.
    Transition {
        id: u64,
        #[serde(default)]
        opacity: Option<String>,
        #[serde(default)]
        x: Option<String>,
        #[serde(default)]
        y: Option<String>,
        #[serde(default)]
        width: Option<String>,
        #[serde(default)]
        height: Option<String>,
        #[serde(default)]
        duration_ms: Option<u32>,
    },
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
                if let Some(node) = tree.get_mut(id) {
                    node.apply_widget_defaults();
                }
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
            HostOp::OpenOverlay { id, anchor, side } => {
                let side = match side.as_str() {
                    "above" => OverlaySide::Above,
                    "below" => OverlaySide::Below,
                    "left" => OverlaySide::Left,
                    "right" => OverlaySide::Right,
                    "center" => OverlaySide::Center,
                    _ => return Err(JsError::Message(format!("unknown overlay side `{side}`"))),
                };
                tree.open_overlay(NodeId::from_raw(*id), NodeId::from_raw(*anchor), side)?;
            }
            HostOp::BindTabPanel { tab, panel } => {
                tree.bind_tab_panel(NodeId::from_raw(*tab), NodeId::from_raw(*panel))?;
            }
            HostOp::BindContextMenu { target, menu } => {
                tree.bind_context_menu(NodeId::from_raw(*target), NodeId::from_raw(*menu))?;
            }
            HostOp::CloseOverlay => {
                tree.close_overlay();
            }
            HostOp::Transition {
                id,
                opacity,
                x,
                y,
                width,
                height,
                duration_ms,
            } => {
                let num = |key: &str, value: &Option<String>| -> Result<Option<f32>, JsError> {
                    match value {
                        Some(text) => Ok(Some(parse_f32(key, text)?)),
                        None => Ok(None),
                    }
                };
                tree.transition(
                    NodeId::from_raw(*id),
                    StyleTransition {
                        opacity: num("opacity", opacity)?,
                        x: num("x", x)?,
                        y: num("y", y)?,
                        width: num("width", width)?,
                        height: num("height", height)?,
                        duration_ms: duration_ms.map(|ms| ms as f32),
                    },
                )?;
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

/// Prop keys `setProp` accepts. `components.json` must stay inside this set
/// (plus the JSX alias `title`, which is rewritten to `name` before the host).
pub const HOST_PROP_KEYS: &[&str] = &[
    "disabled",
    "checked",
    "value",
    "width",
    "height",
    "column",
    "justifyCenter",
    "justify_center",
    "text",
    "name",
    "testKey",
    "imageSource",
    "imageFit",
    "decorative",
    "multiline",
    "fill",
    "color",
    "fontSize",
    "font_size",
    "radius",
    "borderColor",
    "border_color",
    "borderWidth",
    "border_width",
    "padding",
    "gap",
];

fn set_prop(tree: &mut Tree, id: NodeId, key: &str, value: &str) -> Result<(), JsError> {
    match key {
        "width" | "height" | "column" | "justifyCenter" | "justify_center" | "testKey"
        | "imageSource" | "imageFit" | "decorative" | "multiline" => {
            let node = tree
                .get_mut(id)
                .ok_or_else(|| JsError::Message(format!("unknown node {id}")))?;
            match key {
                "width" => node.layout.width = parse_dim(key, value)?,
                "height" => node.layout.height = parse_dim(key, value)?,
                "column" => node.layout.column = parse_bool(key, value)?,
                "testKey" => node.test_key = (!value.is_empty()).then(|| value.to_owned()),
                "multiline" => {
                    if node.kind != NodeKind::TextField {
                        return Err(JsError::Message("multiline requires a text field".into()));
                    }
                    node.multiline = parse_bool(key, value)?;
                }
                "imageSource" => {
                    if node.kind != NodeKind::Image {
                        return Err(JsError::Message(
                            "imageSource requires an image node".into(),
                        ));
                    }
                    #[derive(Deserialize)]
                    struct Source {
                        width: u32,
                        height: u32,
                        rgba: Vec<u8>,
                    }
                    let source: Source = serde_json::from_str(value)
                        .map_err(|e| JsError::Message(format!("invalid image source: {e}")))?;
                    node.image = Some(
                        tekto_tree::Image::new(source.width, source.height, source.rgba)
                            .map_err(|e| JsError::Message(e.to_string()))?,
                    );
                }
                "imageFit" => {
                    if node.kind != NodeKind::Image {
                        return Err(JsError::Message("imageFit requires an image node".into()));
                    }
                    node.image_fit = match value {
                        "contain" => tekto_tree::ImageFit::Contain,
                        "cover" => tekto_tree::ImageFit::Cover,
                        _ => {
                            return Err(JsError::Message(
                                "imageFit must be contain or cover".into(),
                            ))
                        }
                    };
                }
                "decorative" => {
                    if node.kind != NodeKind::Image {
                        return Err(JsError::Message("decorative requires an image node".into()));
                    }
                    node.decorative = parse_bool(key, value)?;
                }
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
        "value" => patch.value = Some(parse_f32(key, value)?),
        "checked" => patch.checked = Some(parse_bool(key, value)?),
        "disabled" => patch.disabled = Some(parse_bool(key, value)?),
        "text" => patch.text = Some(value.into()),
        "name" => patch.name = Some(value.into()),
        "fill" => patch.fill = Some(value.into()),
        "color" => patch.color = Some(value.into()),
        "fontSize" | "font_size" => patch.font_size = Some(parse_f32(key, value)?),
        "radius" => patch.radius = Some(parse_f32(key, value)?),
        "borderColor" | "border_color" => patch.border_color = Some(value.into()),
        "borderWidth" | "border_width" => patch.border_width = Some(parse_f32(key, value)?),
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
/// object: `create`, `setText`, `setProp`, `append`, `remove`,
/// `transition(id, key, value, durationMs)` for one of `opacity`, `x`,
/// `y`, `width`, `height`, or `transition(id, { opacity, x, y, width,
/// height, durationMs })` for several at once, and
/// `applyNativeOp(json)` for one [`NativeOp`] (clipboard, files,
/// notifications, tray, shortcuts). A later transition call merges.
/// Tree calls use the same [`HostOp`] protocol the web target will use.
/// `transition` starts a tween; the host loop advances time with
/// [`tekto_tree::Tree::advance`].
///
/// Without the feature this returns [`JsError::EngineDisabled`].
pub fn eval_to_tree(source: &str) -> Result<Tree, JsError> {
    let mut host = TreeHost::new();
    eval_module(source, &mut host)?;
    let tree = host.tree.borrow().clone();
    Ok(tree)
}

/// Apply a native plugin op (clipboard, files, notifications, tray, shortcuts).
///
/// This is the JS-facing hook beside [`HostOp`]. Tree ops stay on [`Host`];
/// these calls return text or nothing, so they do not go through [`Host::apply`].
/// The JSON shape is [`NativeOp`] (`clipboardGetText`, `clipboardSetText`,
/// `notificationsShow`, `trayShow`, `trayHide`, `traySetLabel`, `trayTakeClick`,
/// `shortcutsRegister`, `shortcutsUnregister`, `shortcutsTakeActivation`,
/// and the `files*` ops).
/// A missing Linux clipboard session, notification daemon, tray watcher, or
/// shortcut display is a [`JsError::Message`] whose text contains `not available`.
/// Android notification permission denial uses that same `not available` text.
/// A Windows tray whose shell rejects the icon does the same.
/// Tray ops need the `tray` feature. Shortcut ops need the `shortcuts` feature.
/// Without the feature the error says `not implemented`.
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

    #[cfg(feature = "quickjs")]
    #[test]
    fn quickjs_can_open_and_close_overlay() {
        let mut host = TreeHost::new();
        eval_module(
            r#"
            const root = tekto.create("window");
            const anchor = tekto.create("button");
            tekto.append(root, anchor);
            const popup = tekto.create("rect");
            tekto.openOverlay(popup, anchor, "below");
        "#,
            &mut host,
        )
        .unwrap();
        assert!(host.tree.borrow().overlay().is_some());
        // A retained session is used to close the same tree.
        let session = JsSession::new().unwrap();
        session.eval("tekto.closeOverlay()", &mut host).unwrap();
        assert!(host.tree.borrow().overlay().is_none());
    }

    #[cfg(feature = "quickjs")]
    #[test]
    fn explicit_overlay_close_disposes_callbacks() {
        let mut host = TreeHost::new();
        let session = JsSession::new().unwrap();
        session
            .eval(
                r#"
            const root = tekto.create("window");
            const anchor = tekto.create("button");
            tekto.append(root, anchor);
            const popup = tekto.create("rect");
            const child = tekto.create("button");
            tekto.append(popup, child);
            globalThis.disposed = 0;
            tekto.onDispose(child, () => { disposed++; });
            tekto.openOverlay(popup, anchor, "below");
            tekto.closeOverlay();
            if (disposed !== 1) throw new Error("cleanup did not run");
        "#,
                &mut host,
            )
            .unwrap();
    }

    #[test]
    fn overlay_host_ops_validate_and_close() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let anchor = tree.add(NodeKind::Button, Some(root)).unwrap();
        let popup = tree.add(NodeKind::Rect, None).unwrap();
        let json = format!(
            r#"[{{"op":"openOverlay","id":{},"anchor":{},"side":"below"}}]"#,
            popup.raw(),
            anchor.raw()
        );
        let ops: Vec<HostOp> = serde_json::from_str(&json).unwrap();
        apply_ops(&mut tree, &ops).unwrap();
        assert_eq!(tree.overlay().unwrap().node, popup);
        assert!(tree.click(anchor).is_err());
        let close: Vec<HostOp> = serde_json::from_str(r#"[{"op":"closeOverlay"}]"#).unwrap();
        apply_ops(&mut tree, &close).unwrap();
        assert!(tree.overlay().is_none());
        assert!(tree.get(popup).is_none());
        let bad = HostOp::OpenOverlay {
            id: 2,
            anchor: 1,
            side: "diagonal".into(),
        };
        assert!(apply_ops(&mut tree, &[bad]).is_err());
    }

    #[test]
    fn paint_props_reach_tree_and_agent_dump() {
        let mut tree = Tree::new();
        let id = tree.add(NodeKind::Window, None).unwrap();
        set_prop(&mut tree, id, "radius", "8").unwrap();
        set_prop(&mut tree, id, "borderColor", "#123456").unwrap();
        set_prop(&mut tree, id, "borderWidth", "2").unwrap();
        let dump = serde_json::to_value(tree.to_dump()).unwrap();
        let node = &dump["nodes"][0];
        assert_eq!(node["radius"], 8.0);
        assert_eq!(node["border_color"], "#123456");
        assert_eq!(node["border_width"], 2.0);
        assert!(set_prop(&mut tree, id, "borderWidth", "bad").is_err());
        let patch: NodePatch =
            serde_json::from_str(r##"{"borderColor":"#abcdef","borderWidth":3}"##).unwrap();
        tree.patch(id, patch).unwrap();
        assert_eq!(tree.node_dump(id).unwrap().border_width, 3.0);
    }

    #[test]
    fn developer_test_key_reaches_scene_dump() {
        let mut tree = Tree::new();
        let id = tree.add(NodeKind::Window, None).unwrap();
        set_prop(&mut tree, id, "testKey", "form").unwrap();
        assert_eq!(
            tree.node_dump(id).unwrap().test_key.as_deref(),
            Some("form")
        );
        set_prop(&mut tree, id, "testKey", "").unwrap();
        assert!(tree.node_dump(id).unwrap().test_key.is_none());
    }

    #[test]
    fn image_props_validate_without_replacing_valid_pixels() {
        let mut tree = Tree::new();
        let id = tree.add(NodeKind::Image, None).unwrap();
        set_prop(
            &mut tree,
            id,
            "imageSource",
            r#"{"width":1,"height":1,"rgba":[255,0,0,255]}"#,
        )
        .unwrap();
        set_prop(&mut tree, id, "imageFit", "cover").unwrap();
        set_prop(&mut tree, id, "decorative", "true").unwrap();
        assert!(set_prop(
            &mut tree,
            id,
            "imageSource",
            r#"{"width":2,"height":1,"rgba":[1]}"#
        )
        .is_err());
        let node = tree.get(id).unwrap();
        assert_eq!(node.image.as_ref().unwrap().pixels(), &[255, 0, 0, 255]);
        assert_eq!(node.image_fit, tekto_tree::ImageFit::Cover);
        assert!(node.decorative);
    }

    #[cfg(feature = "quickjs")]
    #[test]
    fn quickjs_image_props_reach_scene() {
        let mut host = TreeHost::new();
        let session = JsSession::new().unwrap();
        session
            .eval(
                r#"const id = tekto.create('image');
            tekto.setProp(id, 'name', 'Logo');
            tekto.setProp(id, 'imageSource', JSON.stringify({width:1,height:1,rgba:[1,2,3,255]}));
            tekto.setProp(id, 'imageFit', 'cover');"#,
                &mut host,
            )
            .unwrap();
        let tree = host.tree.borrow();
        let node = tree.get(tree.root().unwrap()).unwrap();
        assert_eq!(node.image.as_ref().unwrap().pixels(), &[1, 2, 3, 255]);
        assert_eq!(node.name.as_deref(), Some("Logo"));
        assert_eq!(node.image_fit, tekto_tree::ImageFit::Cover);
    }

    #[cfg(feature = "quickjs")]
    #[test]
    fn generated_bundle_runs_in_quickjs_when_supplied() {
        let Some(path) = std::env::var_os("TEKTO_TEST_JS_BUNDLE") else {
            return;
        };
        let source = std::fs::read_to_string(path).expect("generated bundle");
        let tree = eval_to_tree(&source).expect("bundle should mount in QuickJS");
        assert!(tree.to_pretty_text().contains("Hello from Tekto"));
    }

    #[test]
    fn disabled_host_prop_updates_control_and_focus() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let button = tree.add(NodeKind::Button, Some(root)).unwrap();
        tree.set_focus(button).unwrap();
        assert!(set_prop(&mut tree, root, "disabled", "true").is_err());
        set_prop(&mut tree, button, "disabled", "true").unwrap();
        assert!(tree.get(button).unwrap().disabled);
        assert_eq!(tree.focus, Some(root));
        assert!(tree.click(button).is_err());
        set_prop(&mut tree, button, "disabled", "false").unwrap();
        assert!(tree.click(button).is_ok());
        assert!(set_prop(&mut tree, button, "disabled", "invalid").is_err());
    }

    #[test]
    #[cfg(not(windows))]
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
    #[cfg(not(windows))]
    fn native_notification_op_errors_clearly_without_a_daemon() {
        let op: NativeOp = serde_json::from_str(
            r#"{"op":"notificationsShow","title":"Saved","body":"Note stored"}"#,
        )
        .unwrap();
        let display = std::env::var("DISPLAY").unwrap_or_default();
        let reserved = matches!(display.as_str(), ":1" | ":2" | ":1.0" | ":2.0");
        let bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS")
            .is_some_and(|value| !value.is_empty())
            || std::env::var_os("XDG_RUNTIME_DIR")
                .is_some_and(|dir| std::path::Path::new(&dir).join("bus").exists());
        if reserved || bus {
            let err = JsError::Message(
                "notifications.show is not available on linux: no session bus or notification daemon"
                    .into(),
            );
            assert!(err.to_string().contains("not available"));
            return;
        }
        let err = apply_native_host_op(&op).expect_err("headless or stub notifications");
        let msg = err.to_string();
        assert!(
            msg.contains("not available") || msg.contains("not implemented"),
            "{msg}"
        );
    }

    #[test]
    fn native_shortcut_op_errors_clearly() {
        #[cfg(not(all(target_os = "windows", feature = "shortcuts")))]
        let op: NativeOp = serde_json::from_str(
            r#"{"op":"shortcutsRegister","accelerator":"Ctrl+K","id":"search"}"#,
        )
        .unwrap();
        let display = std::env::var("DISPLAY").unwrap_or_default();
        let reserved = matches!(display.as_str(), ":1" | ":2" | ":1.0" | ":2.0");
        assert!(
            !reserved,
            "refusing shortcuts on reserved display {display}"
        );
        #[cfg(not(any(
            all(target_os = "linux", feature = "shortcuts"),
            all(target_os = "windows", feature = "shortcuts")
        )))]
        {
            let err = apply_native_host_op(&op).expect_err("shortcuts off");
            let msg = err.to_string();
            assert!(msg.contains("not implemented"), "{msg}");
        }
        #[cfg(all(target_os = "windows", feature = "shortcuts"))]
        {
            // Do not call RegisterHotKey from this host. A bad chord is a
            // message and does not start the hotkey thread.
            let bad: NativeOp = serde_json::from_str(
                r#"{"op":"shortcutsRegister","accelerator":"Nope","id":"search"}"#,
            )
            .unwrap();
            let err = apply_native_host_op(&bad).expect_err("bad chord");
            let msg = err.to_string();
            assert!(msg.contains("accelerator"), "{msg}");
            let err = apply_native_host_op(&NativeOp::ShortcutsUnregister {
                id: "search".into(),
            })
            .expect("unregister with nothing bound");
            assert!(err.is_none(), "{err:?}");
        }
        #[cfg(all(target_os = "linux", feature = "shortcuts"))]
        {
            // Do not grab a key from the default process when a display is set.
            if !display.is_empty() {
                let err = JsError::Message(
                    "shortcuts.register is not available on linux: no X11 display".into(),
                );
                assert!(err.to_string().contains("not available"));
                return;
            }
            let err = apply_native_host_op(&op).expect_err("headless shortcuts");
            let msg = err.to_string();
            assert!(msg.contains("not available"), "{msg}");
        }
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

    #[test]
    fn transition_op_reaches_the_end_when_time_advances() {
        let mut tree = Tree::new();
        let id = tree.add(NodeKind::Rect, None).unwrap();
        tree.get_mut(id).unwrap().bounds = tekto_tree::Rect::new(0.0, 0.0, 20.0, 10.0);
        apply_ops(
            &mut tree,
            &[HostOp::Transition {
                id: id.raw(),
                opacity: Some("0".into()),
                x: Some("40".into()),
                y: None,
                width: None,
                height: None,
                duration_ms: Some(100),
            }],
        )
        .unwrap();
        assert!(tree.advance(0.0));
        assert!(tree.advance(50.0));
        let mid = tree.get(id).unwrap();
        assert!(mid.opacity > 0.2 && mid.opacity < 0.8);
        assert!(mid.bounds.x > 10.0 && mid.bounds.x < 30.0);
        assert!(!tree.advance(100.0));
        assert!(tree.get(id).unwrap().opacity < 0.01);
        assert!((tree.get(id).unwrap().bounds.x - 40.0).abs() < 0.01);
    }

    #[test]
    fn consecutive_transition_ops_keep_both_properties() {
        let mut tree = Tree::new();
        let id = tree.add(NodeKind::Rect, None).unwrap();
        tree.get_mut(id).unwrap().bounds = tekto_tree::Rect::new(0.0, 0.0, 20.0, 10.0);
        tree.get_mut(id).unwrap().opacity = 1.0;
        apply_ops(
            &mut tree,
            &[
                HostOp::Transition {
                    id: id.raw(),
                    opacity: Some("0".into()),
                    x: None,
                    y: None,
                    width: None,
                    height: None,
                    duration_ms: Some(100),
                },
                HostOp::Transition {
                    id: id.raw(),
                    opacity: None,
                    x: Some("40".into()),
                    y: None,
                    width: None,
                    height: None,
                    duration_ms: Some(100),
                },
            ],
        )
        .unwrap();
        assert!(tree.advance(0.0));
        assert!(tree.advance(50.0));
        let mid = tree.get(id).unwrap();
        assert!(
            mid.opacity > 0.2 && mid.opacity < 0.8,
            "opacity {}",
            mid.opacity
        );
        assert!(
            mid.bounds.x > 10.0 && mid.bounds.x < 30.0,
            "x {}",
            mid.bounds.x
        );
    }

    #[cfg(feature = "quickjs")]
    #[test]
    fn retained_session_dispatches_and_forgets_callbacks() {
        let mut host = TreeHost::new();
        let session = JsSession::new().unwrap();
        session
            .eval(
                r#"
            const root = tekto.create("window");
            const label = tekto.create("text");
            const button = tekto.create("button");
            tekto.append(root, label); tekto.append(root, button);
            let count = 0;
            tekto.setText(label, "0");
            tekto.onClick(button, () => tekto.setText(label, String(++count)));
        "#,
                &mut host,
            )
            .unwrap();
        assert!(!session.dispatch_click(1, &mut host).unwrap());
        for expected in ["1", "2"] {
            assert!(session.dispatch_click(3, &mut host).unwrap());
            assert_eq!(
                host.tree
                    .borrow()
                    .get(NodeId::from_raw(2))
                    .unwrap()
                    .text
                    .as_deref(),
                Some(expected)
            );
        }
        session.eval("tekto.remove(3)", &mut host).unwrap();
        assert!(!session.dispatch_click(3, &mut host).unwrap());
        assert!(host.tree.borrow().get(NodeId::from_raw(2)).is_some());
        let replacement = JsSession::new().unwrap();
        assert!(!replacement.dispatch_click(3, &mut host).unwrap());
    }

    #[cfg(feature = "quickjs")]
    #[test]
    fn session_cleanup_errors_do_not_skip_other_cleanups_or_registrations() {
        let mut host = TreeHost::new();
        let session = JsSession::new().unwrap();
        session
            .eval(
                r#"
            const label = tekto.create("text");
            tekto.onClick(2, () => {});
            tekto.onDispose(1, () => { throw new Error("cleanup failed"); });
            globalThis.done = 0;
            tekto.onDispose(1, () => { done++; });
            tekto.onDispose(2, () => { done++; });
        "#,
                &mut host,
            )
            .unwrap();
        assert!(session
            .dispose(&mut host)
            .unwrap_err()
            .to_string()
            .contains("cleanup failed"));
        session
            .eval(
                "if (done !== 2) throw new Error('cleanup skipped')",
                &mut host,
            )
            .unwrap();
        assert!(!session.dispatch_click(2, &mut host).unwrap());
        session.dispose(&mut host).unwrap();
    }

    #[cfg(feature = "quickjs")]
    #[test]
    fn retained_session_reports_callback_failure_and_remains_usable() {
        let mut host = TreeHost::new();
        let session = JsSession::new().unwrap();
        session
            .eval(
                r#"tekto.onClick(1, () => { throw new Error("callback failed"); });"#,
                &mut host,
            )
            .unwrap();
        let error = session
            .dispatch_click(1, &mut host)
            .unwrap_err()
            .to_string();
        assert!(error.contains("callback failed"), "{error}");
        session.forget_node(1, &mut host).unwrap();
        assert!(!session.dispatch_click(1, &mut host).unwrap());
    }

    #[cfg(feature = "quickjs")]
    #[test]
    fn js_transition_object_and_follow_up_call_both_run() {
        let mut host = TreeHost::new();
        eval_module(
            r#"
            const box = tekto.create("rect");
            tekto.transition(box, { opacity: 0, x: 40, durationMs: 100 });
            tekto.transition(box, "y", 16, 100);
            "#,
            &mut host,
        )
        .expect("eval");
        let mut tree = host.tree.borrow().clone();
        let id = NodeId::from_raw(1);
        assert!(tree.advance(0.0));
        assert!(tree.advance(50.0));
        let mid = tree.get(id).unwrap();
        assert!(mid.opacity > 0.2 && mid.opacity < 0.8, "{}", mid.opacity);
        assert!(
            mid.bounds.x > 10.0 && mid.bounds.x < 30.0,
            "{}",
            mid.bounds.x
        );
        assert!(
            mid.bounds.y > 4.0 && mid.bounds.y < 12.0,
            "{}",
            mid.bounds.y
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

    /// JS can register, unregister, and poll shortcut activations.
    ///
    /// With `shortcuts` on Linux this runs in a child with no display, so
    /// register is `Unavailable` and nothing is grabbed. With `shortcuts` on
    /// Windows only a bad chord is checked, so `RegisterHotKey` is not called.
    /// Other builds get `Unsupported` (`not implemented`). Neither path panics.
    #[cfg(feature = "quickjs")]
    #[test]
    fn js_apply_native_op_reaches_shortcut_ops() {
        #[cfg(all(target_os = "linux", feature = "shortcuts"))]
        if std::env::var_os("TEKTO_JS_SHORTCUTS_CHILD").is_none() {
            let exe = std::env::current_exe().expect("test exe");
            let child = std::process::Command::new(&exe)
                .args(["tests::js_apply_native_op_reaches_shortcut_ops", "--exact"])
                .env_remove("DISPLAY")
                .env_remove("WAYLAND_DISPLAY")
                .env("TEKTO_JS_SHORTCUTS_CHILD", "1")
                .output()
                .expect("spawn headless js shortcut test");
            let stdout = String::from_utf8_lossy(&child.stdout);
            let stderr = String::from_utf8_lossy(&child.stderr);
            assert!(
                child.status.success() && stdout.contains("1 passed"),
                "js shortcut child failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
            );
            return;
        }

        let mut host = TreeHost::new();
        eval_module(
            r#"
            function detail(err) {
              if (err && typeof err.message === "string" && err.message.length > 0) {
                return err.message;
              }
              return String(err);
            }
            function call(json) {
              try {
                const value = tekto.applyNativeOp(json);
                return "ok:" + value;
              } catch (err) {
                return "err:" + detail(err);
              }
            }
            const text = tekto.create("text");
            tekto.setText(text, [
              call('{"op":"shortcutsRegister","accelerator":"Ctrl+K","id":"search"}'),
              call('{"op":"shortcutsUnregister","id":"search"}'),
              call('{"op":"shortcutsTakeActivation"}'),
              call("{")
            ].join("|"));
            "#,
            &mut host,
        )
        .expect("shortcut ops from JS");
        let pretty = host.tree.borrow().to_pretty_text();
        let report = pretty
            .lines()
            .find(|line| line.contains("err:") || line.contains("ok:"))
            .unwrap_or(&pretty)
            .to_string();
        assert!(
            report.contains("not available") || report.contains("not implemented"),
            "{pretty}"
        );
        if report.contains("not implemented") {
            assert!(report.matches("not implemented").count() >= 3, "{pretty}");
        } else {
            assert!(
                report.contains("ok:null") && report.matches("ok:null").count() >= 2,
                "{pretty}"
            );
        }
        assert!(report.contains("native op JSON"), "{pretty}");
    }

    #[test]
    fn component_schema_lists_every_host_prop() {
        let raw = include_str!("../../../packages/jsx-runtime/src/components.json");
        let schema: serde_json::Value = serde_json::from_str(raw).unwrap();
        let aliases = schema["propAliases"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        // Snake_case spellings are aliases. JSX rewrites `title` before setProp.
        // Disabled is limited to buttons and text fields.
        let mut required: Vec<&str> = HOST_PROP_KEYS
            .iter()
            .copied()
            .filter(|key| {
                ![
                    "disabled",
                    "imageSource",
                    "imageFit",
                    "decorative",
                    "multiline",
                ]
                .contains(key)
                    && !aliases.contains_key(*key)
            })
            .collect();
        required.push("title");
        let components = schema["components"].as_array().unwrap();
        assert!(!components.is_empty(), "components.json lists no widgets");
        for comp in components {
            let kind = comp["name"].as_str().unwrap();
            let listed: Vec<&str> = comp["props"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["name"].as_str().unwrap())
                .collect();
            for key in ["imageSource", "imageFit", "decorative"] {
                assert_eq!(listed.contains(&key), kind == "image");
            }
            assert_eq!(listed.contains(&"multiline"), kind == "text_field");
            assert_eq!(
                listed.contains(&"disabled"),
                matches!(
                    kind,
                    "button"
                        | "text_field"
                        | "checkbox"
                        | "switch"
                        | "slider"
                        | "radio"
                        | "tab"
                        | "menu_item"
                        | "select"
                )
            );
            for name in &listed {
                let host = aliases.get(*name).and_then(|v| v.as_str()).unwrap_or(name);
                assert!(
                    HOST_PROP_KEYS.contains(&host),
                    "{kind} prop {name} is not a host prop"
                );
            }
            for key in &required {
                assert!(
                    listed.contains(key),
                    "{kind} schema omits host prop {key} (setProp accepts it on every node)"
                );
            }
        }
    }
}
