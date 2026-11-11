//! Headless tests using the real scene, AccessKit mapping, and input actions.
use tekto_a11y::{dump, AccessKitNodeDump};
use tekto_tree::{Color, NodeId, NodeKind, Tree};

/// A fixed-size, CPU-only test environment. Time advances only when requested.
pub struct TestApp {
    pub tree: Tree,
    width: u32,
    height: u32,
    time_ms: f64,
}

impl TestApp {
    pub fn new(mut tree: Tree, width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("viewport must be non-zero".into());
        }
        tekto_layout::layout_tree(&mut tree, width as f32, height as f32);
        Ok(Self {
            tree,
            width,
            height,
            time_ms: 0.0,
        })
    }

    /// Find exactly one attached node by AccessKit role and accessible name.
    pub fn find(&self, role: &str, name: &str) -> Result<NodeId, String> {
        let snapshot = dump(&self.tree).ok_or("empty tree")?;
        let matches: Vec<_> = snapshot
            .nodes
            .iter()
            .filter(|n| n.role == role && n.label.as_deref() == Some(name))
            .collect();
        match matches.as_slice() {
            [node] => Ok(NodeId::from_raw(node.id)),
            [] => Err(format!("no {role} named {name:?}")),
            _ => Err(format!("multiple {role} nodes named {name:?}")),
        }
    }

    /// Find a unique developer key among nodes present in the real accessibility tree.
    pub fn find_key(&self, key: &str) -> Result<NodeId, String> {
        if key.is_empty() {
            return Err("test key must not be empty".into());
        }
        let snapshot = dump(&self.tree).ok_or("empty tree")?;
        let matches: Vec<_> = snapshot
            .nodes
            .iter()
            .filter_map(|node| {
                let id = NodeId::from_raw(node.id);
                self.tree
                    .get(id)
                    .filter(|scene| scene.test_key.as_deref() == Some(key))
                    .map(|_| id)
            })
            .collect();
        match matches.as_slice() {
            [id] => Ok(*id),
            [] => Err(format!("no attached node with test key {key:?}")),
            _ => Err(format!("multiple attached nodes with test key {key:?}")),
        }
    }

    /// Read an attached node through AccessKit. Stale and unrealized ids fail.
    pub fn node(&self, id: NodeId) -> Result<AccessKitNodeDump, String> {
        dump(&self.tree)
            .ok_or("empty tree")?
            .nodes
            .into_iter()
            .find(|n| n.id == id.raw())
            .ok_or_else(|| format!("node {id} is not attached"))
    }

    pub fn click(&mut self, id: NodeId) -> Result<(), String> {
        self.node(id)?;
        self.tree.click(id).map_err(|e| e.to_string())?;
        self.layout();
        Ok(())
    }

    pub fn type_text(&mut self, id: NodeId, text: &str) -> Result<(), String> {
        self.node(id)?;
        self.tree.insert_text(id, text).map_err(|e| e.to_string())?;
        self.layout();
        Ok(())
    }

    pub fn focus(&mut self, id: NodeId) -> Result<(), String> {
        self.node(id)?;
        self.tree.set_focus(id).map_err(|e| e.to_string())
    }

    pub fn is_focused(&self, id: NodeId) -> Result<bool, String> {
        self.node(id)?;
        Ok(self.tree.focus == Some(id))
    }

    pub fn checked(&self, id: NodeId) -> Result<bool, String> {
        let node = self.node(id)?;
        node.toggled
            .ok_or_else(|| format!("node {id} is not toggleable"))
    }

    pub fn value(&self, id: NodeId) -> Result<Option<String>, String> {
        Ok(self.node(id)?.value)
    }
    pub fn disabled(&self, id: NodeId) -> Result<bool, String> {
        Ok(self.node(id)?.disabled)
    }

    /// Run a condition at fixed time steps, without sleeping or an OS window.
    pub fn wait_until(
        &mut self,
        step_ms: u32,
        steps: u32,
        mut condition: impl FnMut(&Tree) -> bool,
    ) -> Result<(), String> {
        if condition(&self.tree) {
            return Ok(());
        }
        for _ in 0..steps {
            self.time_ms += f64::from(step_ms);
            self.tree.advance(self.time_ms);
            self.layout();
            if condition(&self.tree) {
                return Ok(());
            }
        }
        Err(format!(
            "condition not met after {steps} steps of {step_ms}ms"
        ))
    }

    pub fn png(&self) -> Result<Vec<u8>, String> {
        tekto_render::render_tree_png(&self.tree, self.width, self.height, Color::PAGE)
            .map_err(|e| e.to_string())
    }

    /// Compare rendered pixels to a PNG. Set `update` explicitly to write the reference.
    /// On a mismatch, writes `actual.png`, `expected.png`, and `diff.png` to `artifacts`.
    pub fn compare_png(
        &self,
        expected: &std::path::Path,
        artifacts: &std::path::Path,
        update: bool,
    ) -> Result<(), String> {
        let actual_png = self.png()?;
        if update {
            if let Some(parent) = expected.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(expected, actual_png).map_err(|e| e.to_string())?;
            return Ok(());
        }
        let expected_png = std::fs::read(expected)
            .map_err(|e| format!("cannot read {}: {e}", expected.display()))?;
        let actual = tiny_skia::Pixmap::decode_png(&actual_png).map_err(|e| e.to_string())?;
        let reference = tiny_skia::Pixmap::decode_png(&expected_png)
            .map_err(|e| format!("invalid reference PNG: {e}"))?;
        if actual.width() == reference.width()
            && actual.height() == reference.height()
            && actual.data() == reference.data()
        {
            return Ok(());
        }
        std::fs::create_dir_all(artifacts).map_err(|e| e.to_string())?;
        std::fs::write(artifacts.join("actual.png"), &actual_png).map_err(|e| e.to_string())?;
        std::fs::write(artifacts.join("expected.png"), &expected_png).map_err(|e| e.to_string())?;
        let width = actual.width().max(reference.width());
        let height = actual.height().max(reference.height());
        let mut diff = tiny_skia::Pixmap::new(width, height).ok_or("cannot allocate PNG diff")?;
        for y in 0..height {
            for x in 0..width {
                let pixel = |pix: &tiny_skia::Pixmap| -> Option<[u8; 4]> {
                    if x >= pix.width() || y >= pix.height() {
                        return None;
                    }
                    let index = ((y * pix.width() + x) * 4) as usize;
                    Some(pix.data()[index..index + 4].try_into().expect("pixel"))
                };
                let index = ((y * width + x) * 4) as usize;
                let changed = pixel(&actual) != pixel(&reference);
                diff.data_mut()[index..index + 4].copy_from_slice(if changed {
                    &[255, 0, 255, 255]
                } else {
                    &[0, 0, 0, 255]
                });
            }
        }
        std::fs::write(
            artifacts.join("diff.png"),
            diff.encode_png().map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Err(format!(
            "PNG differs from {}; artifacts in {}",
            expected.display(),
            artifacts.display()
        ))
    }

    fn layout(&mut self) {
        tekto_layout::layout_tree(&mut self.tree, self.width as f32, self.height as f32);
    }
}

/// Retained QuickJS test host. It is available only with `--features quickjs`.
#[cfg(feature = "quickjs")]
pub struct JsTestApp {
    session: tekto_js::JsSession,
    host: tekto_js::TreeHost,
    width: u32,
    height: u32,
}

#[cfg(feature = "quickjs")]
impl JsTestApp {
    /// Evaluate a bundled JavaScript app and retain its callbacks.
    pub fn new(source: &str, width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("viewport must be non-zero".into());
        }
        let mut host = tekto_js::TreeHost::new();
        let session = tekto_js::JsSession::new().map_err(|e| e.to_string())?;
        session.eval(source, &mut host).map_err(|e| e.to_string())?;
        let mut app = Self {
            session,
            host,
            width,
            height,
        };
        app.layout();
        Ok(app)
    }

    pub fn find(&self, role: &str, name: &str) -> Result<NodeId, String> {
        TestApp::new(self.host.tree.borrow().clone(), self.width, self.height)?.find(role, name)
    }

    pub fn find_key(&self, key: &str) -> Result<NodeId, String> {
        TestApp::new(self.host.tree.borrow().clone(), self.width, self.height)?.find_key(key)
    }

    pub fn node(&self, id: NodeId) -> Result<AccessKitNodeDump, String> {
        TestApp::new(self.host.tree.borrow().clone(), self.width, self.height)?.node(id)
    }

    pub fn click(&mut self, id: NodeId) -> Result<(), String> {
        self.node(id)?;
        self.host
            .tree
            .borrow_mut()
            .click(id)
            .map_err(|e| e.to_string())?;
        self.session
            .dispatch_click(id.raw(), &mut self.host)
            .map_err(|e| e.to_string())?;
        self.layout();
        Ok(())
    }

    pub fn type_text(&mut self, id: NodeId, text: &str) -> Result<(), String> {
        self.node(id)?;
        let value = {
            let mut tree = self.host.tree.borrow_mut();
            let before = tree
                .get(id)
                .and_then(|n| n.text.clone())
                .unwrap_or_default();
            tree.insert_text(id, text).map_err(|e| e.to_string())?;
            let after = tree
                .get(id)
                .and_then(|n| n.text.clone())
                .unwrap_or_default();
            (before != after).then_some(after)
        };
        if let Some(value) = value {
            self.session
                .dispatch_input(id.raw(), &value, &mut self.host)
                .map_err(|e| e.to_string())?;
        }
        self.layout();
        Ok(())
    }

    pub fn dispose(&mut self) -> Result<(), String> {
        self.session
            .dispose(&mut self.host)
            .map_err(|e| e.to_string())
    }

    fn layout(&mut self) {
        tekto_layout::layout_tree(
            &mut self.host.tree.borrow_mut(),
            self.width as f32,
            self.height as f32,
        );
    }
}

/// Check the real scene kind when a test needs to distinguish generic roles.
pub fn kind(tree: &Tree, id: NodeId) -> Option<NodeKind> {
    tree.get(id).map(|node| node.kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tekto_tree::{NodePatch, TreeEvent};
    use tekto_widgets::{button, checkbox, text, text_field, window};

    #[test]
    fn form_uses_semantics_and_real_actions() {
        let mut tree = Tree::new();
        let root = window(&mut tree, "Form", 320.0, 240.0).unwrap();
        let field = text_field(&mut tree, root, "Name", "").unwrap();
        let check = checkbox(&mut tree, root, "Agree", false).unwrap();
        let save = button(&mut tree, root, "Save").unwrap();
        let label = text(&mut tree, root, "Waiting", 20.0, Color::INK).unwrap();
        tree.set_event_handler(move |tree, event| {
            if event == TreeEvent::Click(save) {
                tree.patch(
                    label,
                    NodePatch {
                        text: Some("Saved".into()),
                        ..NodePatch::default()
                    },
                )
                .unwrap();
            }
        });
        let mut app = TestApp::new(tree, 320, 240).unwrap();
        assert_eq!(app.find("TextInput", "Name").unwrap(), field);
        assert_eq!(app.find("Button", "Save").unwrap(), save);
        app.type_text(field, "Ada").unwrap();
        assert_eq!(app.value(field).unwrap().as_deref(), Some("Ada"));
        assert!(app.is_focused(field).unwrap());
        app.click(check).unwrap();
        assert!(app.checked(check).unwrap());
        app.click(save).unwrap();
        assert!(app.find("Label", "Saved").is_ok());
        assert!(app.png().unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
        app.tree
            .patch(
                save,
                NodePatch {
                    disabled: Some(true),
                    ..NodePatch::default()
                },
            )
            .unwrap();
        assert!(app.disabled(save).unwrap());
        assert!(app.click(save).unwrap_err().contains("disabled"));
        app.tree.remove(save).unwrap();
        assert!(app.click(save).unwrap_err().contains("not attached"));
    }

    #[cfg(feature = "quickjs")]
    #[test]
    fn retained_js_click_and_input_update_a_label() {
        let source = r#"
            const root = tekto.create('window');
            const button = tekto.create('button'); tekto.setProp(button, 'name', 'Save');
            const field = tekto.create('text_field'); tekto.setProp(field, 'name', 'Name');
            const label = tekto.create('text'); tekto.setText(label, 'Waiting');
            tekto.append(root, button); tekto.append(root, field); tekto.append(root, label);
            tekto.onClick(button, () => tekto.setText(label, 'Clicked'));
            tekto.onInput(field, value => tekto.setText(label, value));
        "#;
        let mut app = JsTestApp::new(source, 320, 240).unwrap();
        let button = app.find("Button", "Save").unwrap();
        let field = app.find("TextInput", "Name").unwrap();
        app.click(button).unwrap();
        assert!(app.find("Label", "Clicked").is_ok());
        app.type_text(field, "Ada").unwrap();
        assert!(app.find("Label", "Ada").is_ok());
        app.host
            .tree
            .borrow_mut()
            .patch(
                button,
                NodePatch {
                    disabled: Some(true),
                    ..NodePatch::default()
                },
            )
            .unwrap();
        assert!(app.click(button).unwrap_err().contains("disabled"));
        assert!(app.find("Label", "Ada").is_ok());
        app.dispose().unwrap();
    }

    #[test]
    fn png_compare_updates_and_writes_diff_artifacts() {
        let mut tree = Tree::new();
        let root = window(&mut tree, "Form", 100.0, 100.0).unwrap();
        let label = text(&mut tree, root, "Before", 20.0, Color::INK).unwrap();
        let mut app = TestApp::new(tree, 100, 100).unwrap();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tekto-test-{}-{stamp}", std::process::id()));
        let reference = dir.join("reference.png");
        let artifacts = dir.join("artifacts");
        app.compare_png(&reference, &artifacts, true).unwrap();
        app.compare_png(&reference, &artifacts, false).unwrap();
        app.tree
            .patch(
                label,
                NodePatch {
                    text: Some("After".into()),
                    ..NodePatch::default()
                },
            )
            .unwrap();
        app.layout();
        assert!(app.compare_png(&reference, &artifacts, false).is_err());
        for name in ["actual.png", "expected.png", "diff.png"] {
            assert!(std::fs::read(artifacts.join(name))
                .unwrap()
                .starts_with(b"\x89PNG\r\n\x1a\n"));
        }
    }

    #[test]
    fn ambiguous_names_and_unattached_nodes_fail() {
        let mut tree = Tree::new();
        let root = window(&mut tree, "Form", 100.0, 100.0).unwrap();
        button(&mut tree, root, "Same").unwrap();
        button(&mut tree, root, "Same").unwrap();
        let orphan = tree.add(NodeKind::Button, None).unwrap();
        let app = TestApp::new(tree, 100, 100).unwrap();
        assert!(app.find("Button", "Same").unwrap_err().contains("multiple"));
        assert!(app.node(orphan).unwrap_err().contains("not attached"));
    }

    #[test]
    fn test_keys_resolve_only_unique_attached_nodes() {
        let mut tree = Tree::new();
        let root = window(&mut tree, "Form", 100.0, 100.0).unwrap();
        let first = button(&mut tree, root, "One").unwrap();
        let second = button(&mut tree, root, "Two").unwrap();
        let orphan = tree.add(NodeKind::Button, None).unwrap();
        tree.get_mut(first).unwrap().test_key = Some("save".into());
        tree.get_mut(orphan).unwrap().test_key = Some("save".into());
        let mut app = TestApp::new(tree, 100, 100).unwrap();
        assert_eq!(app.find_key("save").unwrap(), first);
        app.tree.get_mut(second).unwrap().test_key = Some("save".into());
        assert!(app.find_key("save").unwrap_err().contains("multiple"));
        app.tree.remove(first).unwrap();
        assert_eq!(app.find_key("save").unwrap(), second);
        app.tree.remove(second).unwrap();
        assert!(app.find_key("save").unwrap_err().contains("no attached"));
    }
}
