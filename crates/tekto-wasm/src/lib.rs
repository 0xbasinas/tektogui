//! WASM entry for the Tekto core.
//!
//! The browser JavaScript engine sends the same [`tekto_js::HostOp`] JSON
//! that desktop QuickJS uses, then asks for a CPU-renderer PNG. This crate
//! does not link QuickJS. The default build does not link wgpu. The host
//! page blits those PNG bytes with `@tektogui/wasm/canvas` (`paintCanvas`).
//! `@tektogui/wasm/gpu` (`paintPreferred`) can ask a `--features gpu` build
//! to paint the same tree on a WebGPU canvas, and otherwise uses that blit.
//!
//! On `wasm32-unknown-unknown` the cdylib exports a tiny C ABI
//! (`tekto_alloc`, `tekto_apply_ops`, `tekto_set_fallback_font`, `tekto_render_png`, `tekto_reset`,
//! `tekto_out_ptr`, `tekto_out_len`). `@tektogui/wasm` is the JS side.
//! Copy `tekto_out_*` bytes before the next call; the module reuses one
//! output buffer.
//!
//! `--features gpu` (wasm32 only, not the default) also exports
//! `tekto_gpu_frame` through wasm-bindgen. That call paints the same tree
//! with Vello on a WebGPU canvas. If it fails, the host blits the CPU PNG.

use std::cell::RefCell;

use tekto_js::{apply_ops, HostOp};
use tekto_layout::layout_tree;
use tekto_render::render_tree_png;
use tekto_tree::{Color, Tree};

thread_local! {
    static TREE: RefCell<Tree> = RefCell::new(Tree::new());
}

/// Drop the scene so the next `create` starts at id 1 again.
pub fn reset() {
    TREE.with(|tree| *tree.borrow_mut() = Tree::new());
}

/// Apply a JSON array of `HostOp` values. Returns the ids created by
/// `create` ops, in order.
pub fn apply_ops_json(json: &str) -> Result<Vec<u64>, String> {
    let ops: Vec<HostOp> = serde_json::from_str(json).map_err(|err| err.to_string())?;
    TREE.with(|tree| apply_ops(&mut tree.borrow_mut(), &ops).map_err(|err| err.to_string()))
}

/// Layout the current tree and encode a PNG with the CPU renderer.
pub fn render_png(width: u32, height: u32) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("width and height must be non-zero".into());
    }
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        layout_tree(&mut tree, width as f32, height as f32);
        render_tree_png(&tree, width, height, Color::PAGE).map_err(|err| err.to_string())
    })
}

/// Hit-test using the supplied viewport and update the stored tree.
pub fn click_at(width: u32, height: u32, x: f32, y: f32) -> Option<u64> {
    if width == 0 || height == 0 || !x.is_finite() || !y.is_finite() {
        return None;
    }
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        layout_tree(&mut tree, width as f32, height as f32);
        tree.click_at(x, y).map(|id| id.raw())
    })
}

/// Open a bound context menu at a logical pointer location.
pub fn context_menu_at(width: u32, height: u32, x: f32, y: f32) -> Option<u64> {
    if width == 0 || height == 0 || !x.is_finite() || !y.is_finite() {
        return None;
    }
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        layout_tree(&mut tree, width as f32, height as f32);
        tree.context_menu_at(x, y).map(|id| id.raw())
    })
}

/// Focus a field at the pointer and start a selection at its caret.
pub fn field_begin(width: u32, height: u32, x: f32, y: f32) -> Option<u64> {
    if width == 0 || height == 0 || !x.is_finite() || !y.is_finite() {
        return None;
    }
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        layout_tree(&mut tree, width as f32, height as f32);
        let id = tree.hit(x, y)?;
        let node = tree.get(id)?;
        if node.kind != tekto_tree::NodeKind::TextField || node.disabled || !node.preedit.is_empty()
        {
            return None;
        }
        let caret = tekto_layout::field_caret_at(&tree, id, x, y)?;
        tree.click(id).ok()?;
        tree.set_field_selection(id, caret, caret).ok()?;
        Some(id.raw())
    })
}

/// Extend the captured field selection to a pointer position.
pub fn field_move(id: u64, x: f32, y: f32) -> bool {
    if !x.is_finite() || !y.is_finite() {
        return false;
    }
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        let id = tekto_tree::NodeId::from_raw(id);
        if tree.focus != Some(id) {
            return false;
        }
        let Some(node) = tree.get(id) else {
            return false;
        };
        if node.disabled || !node.preedit.is_empty() {
            return false;
        }
        let Some(anchor) = node.selection_anchor else {
            return false;
        };
        let Some(caret) = tekto_layout::field_caret_at(&tree, id, x, y) else {
            return false;
        };
        tree.set_field_selection(id, anchor, caret).is_ok()
    })
}

/// Start a slider drag at a point, returning the captured id.
pub fn slider_begin(width: u32, height: u32, x: f32, y: f32) -> Option<u64> {
    if width == 0 || height == 0 || !x.is_finite() || !y.is_finite() {
        return None;
    }
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        layout_tree(&mut tree, width as f32, height as f32);
        let id = tree.hit(x, y)?;
        let node = tree.get(id)?;
        if node.kind != tekto_tree::NodeKind::Slider || node.disabled {
            return None;
        }
        tree.set_focus(id).ok()?;
        let bounds = tree.get(id)?.bounds;
        let value = (x - bounds.x - 8.0) / (bounds.width - 16.0).max(1.0);
        tree.set_slider_value(id, value).ok()?;
        Some(id.raw())
    })
}

/// Move an existing slider capture; removed, blocked or disabled sliders fail.
pub fn slider_move(id: u64, x: f32) -> bool {
    if !x.is_finite() {
        return false;
    }
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        let id = tekto_tree::NodeId::from_raw(id);
        let Some(node) = tree.get(id) else {
            return false;
        };
        let value = (x - node.bounds.x - 8.0) / (node.bounds.width - 16.0).max(1.0);
        tree.set_slider_value(id, value).is_ok()
    })
}

/// Route slider keyboard navigation, returning whether a slider handled the key.
pub fn slider_key(code: u32) -> bool {
    slider_key_with_activation(code).0
}

fn slider_key_with_activation(code: u32) -> (bool, Option<u64>) {
    let key = match code {
        1 => tekto_tree::NamedActionKey::ArrowLeft,
        2 => tekto_tree::NamedActionKey::ArrowRight,
        3 => tekto_tree::NamedActionKey::Home,
        4 => tekto_tree::NamedActionKey::End,
        5 => tekto_tree::NamedActionKey::ArrowUp,
        6 => tekto_tree::NamedActionKey::ArrowDown,
        _ => return (false, None),
    };
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        let eligible = tree.focus.and_then(|id| tree.get(id)).is_some_and(|n| {
            matches!(
                n.kind,
                tekto_tree::NodeKind::Slider
                    | tekto_tree::NodeKind::Radio
                    | tekto_tree::NodeKind::Tab
                    | tekto_tree::NodeKind::MenuItem
            ) && !n.disabled
        });
        let activation = if eligible {
            let (_, id) = tekto_tree::route_named_key(&mut tree, key, false);
            id.filter(|id| tree.click(*id).is_ok()).map(|id| id.raw())
        } else {
            None
        };
        (eligible, activation)
    })
}

/// Apply printable keyboard text or a field editing key to the focused field.
/// Return its id and committed value only when the value changed.
pub fn field_input(text: &str, backspace: bool) -> Option<(u64, String)> {
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        let id = tree.focus?;
        let before = tree.get(id)?.text.clone().unwrap_or_default();
        if backspace {
            tree.apply_field_key(tekto_tree::FieldKey::Backspace).ok()?;
        } else {
            tree.type_keyboard_text(text).ok()?;
        }
        let value = tree.get(id)?.text.clone().unwrap_or_default();
        (before != value).then_some((id.raw(), value))
    })
}

/// Apply IME preedit, commit, or cancellation to the focused field.
/// Returns the field id and a value only when committed text changed.
pub fn field_ime(kind: u32, text: &str, expected: Option<u64>) -> Option<(u64, Option<String>)> {
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        let id = tree.focus?;
        if expected.is_some_and(|target| target != id.raw()) {
            return None;
        }
        let node = tree.get(id)?;
        if node.kind != tekto_tree::NodeKind::TextField || node.disabled {
            return None;
        }
        let before = node.text.clone().unwrap_or_default();
        let event = match kind {
            1 => tekto_tree::ImeEvent::Preedit {
                text: text.to_owned(),
                cursor: Some((text.len(), text.len())),
            },
            2 => tekto_tree::ImeEvent::Commit(text.to_owned()),
            3 => tekto_tree::ImeEvent::Disabled,
            _ => return None,
        };
        tree.apply_ime(event).ok()?;
        let after = tree.get(id)?.text.clone().unwrap_or_default();
        Some((id.raw(), (before != after).then_some(after)))
    })
}

/// Apply a field navigation or editing key and return its resulting value.
pub fn field_key(code: u32) -> Option<(u64, Option<String>)> {
    use tekto_tree::FieldKey;
    if (11..=15).contains(&code) {
        return TREE.with(|tree| {
            let mut tree = tree.borrow_mut();
            let id = tree.focus?;
            let node = tree.get(id)?;
            if node.kind != tekto_tree::NodeKind::TextField
                || !node.multiline
                || node.disabled
                || !node.preedit.is_empty()
            {
                return None;
            }
            let before = node.text.clone().unwrap_or_default();
            if code == 15 {
                if !tree.type_keyboard_text("\n").ok()? {
                    return None;
                }
            } else if !tekto_layout::field_move_lines(
                &mut tree,
                id,
                if code == 11 || code == 13 { -1 } else { 1 },
                code >= 13,
            ) {
                return None;
            }
            let after = tree.get(id)?.text.clone().unwrap_or_default();
            Some((id.raw(), (before != after).then_some(after)))
        });
    }
    let key = match code {
        1 => FieldKey::Left,
        2 => FieldKey::Right,
        3 => FieldKey::Home,
        4 => FieldKey::End,
        5 => FieldKey::Delete,
        6 => FieldKey::SelectLeft,
        7 => FieldKey::SelectRight,
        8 => FieldKey::SelectHome,
        9 => FieldKey::SelectEnd,
        10 => FieldKey::SelectAll,
        _ => return None,
    };
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        let id = tree.focus?;
        let before = tree.get(id)?.text.clone().unwrap_or_default();
        if !tree.apply_field_key(key).ok()? {
            return None;
        }
        let after = tree.get(id)?.text.clone().unwrap_or_default();
        Some((id.raw(), (before != after).then_some(after)))
    })
}

/// Return selected text in the focused, enabled field, if any.
pub fn field_selection() -> Option<String> {
    TREE.with(|tree| {
        let tree = tree.borrow();
        let id = tree.focus?;
        if tree.get(id)?.disabled {
            return None;
        }
        let text = tree.selected_text(id).ok()?;
        (!text.is_empty()).then_some(text)
    })
}

/// The focused editable field, if enabled.
pub fn focused_field() -> Option<u64> {
    TREE.with(|tree| {
        let tree = tree.borrow();
        let id = tree.focus?;
        let node = tree.get(id)?;
        (node.kind == tekto_tree::NodeKind::TextField && !node.disabled).then_some(id.raw())
    })
}

/// Activate the focused button, menu item, or toggle with Enter or Space.
pub fn activate_focused(space: bool) -> Option<u64> {
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        let key = if space {
            tekto_tree::NamedActionKey::Space
        } else {
            tekto_tree::NamedActionKey::Enter
        };
        let (_, id) = tekto_tree::route_named_key(&mut tree, key, false);
        let id = id?;
        tree.click(id).ok()?;
        Some(id.raw())
    })
}

/// Read the select that owns the current option popup.
pub fn open_select_state() -> Option<(u64, String)> {
    TREE.with(|tree| {
        let tree = tree.borrow();
        let overlay = tree.overlay()?;
        let node = tree.get(overlay.anchor)?;
        (node.kind == tekto_tree::NodeKind::Select)
            .then(|| (overlay.anchor.raw(), node.text.clone().unwrap_or_default()))
    })
}

/// Read a select's current value after an action.
pub fn select_value(id: u64) -> Option<String> {
    TREE.with(|tree| {
        let tree = tree.borrow();
        let node = tree.get(tekto_tree::NodeId::from_raw(id))?;
        (node.kind == tekto_tree::NodeKind::Select).then(|| node.text.clone().unwrap_or_default())
    })
}

/// Stable id of the open overlay, if present.
pub fn overlay_id() -> Option<u64> {
    TREE.with(|tree| tree.borrow().overlay().map(|overlay| overlay.node.raw()))
}

/// Dismiss an open overlay as if Escape was pressed.
pub fn escape() -> bool {
    TREE.with(|tree| tree.borrow_mut().close_overlay())
}

/// Scroll the container under the point, in layout pixels.
pub fn scroll_at(width: u32, height: u32, x: f32, y: f32, delta: f32) -> Option<f32> {
    if width == 0 || height == 0 || !x.is_finite() || !y.is_finite() || !delta.is_finite() {
        return None;
    }
    TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        layout_tree(&mut tree, width as f32, height as f32);
        tree.scroll_at(x, y, delta)
    })
}

/// Pointer ABI used by the browser glue. Host tests call the functions above.
#[cfg(target_arch = "wasm32")]
mod ffi {
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicU32, Ordering};

    thread_local! {
        static INPUT: RefCell<Vec<u8>> = RefCell::new(Vec::new());
        static OUTPUT: RefCell<Vec<u8>> = RefCell::new(Vec::new());
    }

    static OUT_PTR: AtomicU32 = AtomicU32::new(0);
    static OUT_LEN: AtomicU32 = AtomicU32::new(0);

    fn publish(bytes: &[u8]) {
        OUTPUT.with(|slot| {
            let mut buf = slot.borrow_mut();
            buf.clear();
            buf.extend_from_slice(bytes);
            // The allocation stays in this thread-local until the next publish.
            OUT_PTR.store(buf.as_ptr() as u32, Ordering::Relaxed);
            OUT_LEN.store(buf.len() as u32, Ordering::Relaxed);
        });
    }

    /// Resize the single input buffer and return its address.
    ///
    /// The pointer is valid until the next `tekto_alloc`. `len` is in bytes.
    #[no_mangle]
    pub extern "C" fn tekto_alloc(len: u32) -> *mut u8 {
        INPUT.with(|slot| {
            let mut buf = slot.borrow_mut();
            buf.clear();
            buf.resize(len as usize, 0);
            buf.as_mut_ptr()
        })
    }

    #[no_mangle]
    pub extern "C" fn tekto_out_ptr() -> u32 {
        OUT_PTR.load(Ordering::Relaxed)
    }

    #[no_mangle]
    pub extern "C" fn tekto_out_len() -> u32 {
        OUT_LEN.load(Ordering::Relaxed)
    }

    #[no_mangle]
    pub extern "C" fn tekto_reset() {
        super::reset();
    }

    /// `ptr`/`len` are UTF-8 JSON (`HostOp` array). The output buffer becomes
    /// `{"ok":true,"created":[...]}` or `{"ok":false,"error":"..."}`.
    ///
    /// # Safety
    ///
    /// `ptr` must be `len` bytes inside this module's memory (usually from
    /// [`tekto_alloc`]) and must stay live for this call.
    #[no_mangle]
    pub unsafe extern "C" fn tekto_apply_ops(ptr: *const u8, len: u32) {
        let parsed = read_utf8(ptr, len).and_then(|json| super::apply_ops_json(&json));
        let body = match parsed {
            Ok(created) => serde_json::json!({ "ok": true, "created": created }),
            Err(error) => serde_json::json!({ "ok": false, "error": error }),
        };
        publish(body.to_string().as_bytes());
    }

    /// Configure a fallback font from copied bytes, before text initialization.
    /// Publishes a JSON object with `ok` and, on failure, `error`.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null and valid for `len` bytes for this call.
    #[no_mangle]
    pub unsafe extern "C" fn tekto_set_fallback_font(ptr: *const u8, len: u32) {
        let result = if ptr.is_null() {
            Err("null font pointer")
        } else {
            let bytes = std::slice::from_raw_parts(ptr, len as usize).to_vec();
            tekto_text::set_fallback_font(bytes)
        };
        let body = match result {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(error) => serde_json::json!({ "ok": false, "error": error }),
        };
        publish(body.to_string().as_bytes());
    }

    /// Layout and paint. On success the output buffer is PNG bytes. On
    /// failure it is a UTF-8 error string (it does not start with the PNG
    /// magic).
    #[no_mangle]
    pub extern "C" fn tekto_render_png(width: u32, height: u32) {
        match super::render_png(width, height) {
            Ok(png) => publish(&png),
            Err(error) => publish(error.as_bytes()),
        }
    }

    /// Returns the hit node id, or zero when there is no hit.
    #[no_mangle]
    pub extern "C" fn tekto_click_at(width: u32, height: u32, x: f32, y: f32) -> f64 {
        super::click_at(width, height, x, y).unwrap_or(0) as f64
    }

    #[no_mangle]
    pub extern "C" fn tekto_context_menu_at(width: u32, height: u32, x: f32, y: f32) -> f64 {
        super::context_menu_at(width, height, x, y).unwrap_or(0) as f64
    }

    #[no_mangle]
    pub extern "C" fn tekto_field_begin(width: u32, height: u32, x: f32, y: f32) -> f64 {
        super::field_begin(width, height, x, y).unwrap_or(0) as f64
    }

    #[no_mangle]
    pub extern "C" fn tekto_field_move(id: f64, x: f32, y: f32) -> u32 {
        if id <= 0.0 || !id.is_finite() || id.fract() != 0.0 {
            return 0;
        }
        u32::from(super::field_move(id as u64, x, y))
    }

    /// Apply keyboard text, publishing the changed field id and value as JSON.
    /// # Safety
    /// `ptr` and `len` must point to valid UTF-8 bytes in this module.
    #[no_mangle]
    pub unsafe extern "C" fn tekto_field_input(ptr: *const u8, len: u32, backspace: u32) {
        let value = read_utf8(ptr, len)
            .ok()
            .and_then(|s| super::field_input(&s, backspace != 0));
        publish(serde_json::to_string(&value).unwrap().as_bytes());
    }

    /// # Safety
    /// `ptr` and `len` must identify valid UTF-8 bytes in module memory.
    #[no_mangle]
    pub unsafe extern "C" fn tekto_field_ime(ptr: *const u8, len: u32, kind: u32, expected: f64) {
        let target = if expected == 0.0 {
            None
        } else if expected > 0.0 && expected.is_finite() && expected.fract() == 0.0 {
            Some(expected as u64)
        } else {
            publish(b"null");
            return;
        };
        let value = read_utf8(ptr, len)
            .ok()
            .and_then(|text| super::field_ime(kind, &text, target));
        publish(serde_json::to_string(&value).unwrap().as_bytes());
    }

    #[no_mangle]
    pub extern "C" fn tekto_field_key(code: u32) {
        publish(
            serde_json::to_string(&super::field_key(code))
                .unwrap()
                .as_bytes(),
        );
    }

    #[no_mangle]
    pub extern "C" fn tekto_field_selection() {
        publish(
            serde_json::to_string(&super::field_selection())
                .unwrap()
                .as_bytes(),
        );
    }

    #[no_mangle]
    pub extern "C" fn tekto_focused_field() -> f64 {
        super::focused_field().unwrap_or(0) as f64
    }

    #[no_mangle]
    pub extern "C" fn tekto_activate_focused(space: u32) -> f64 {
        super::activate_focused(space != 0).unwrap_or(0) as f64
    }

    #[no_mangle]
    pub extern "C" fn tekto_open_select_state() {
        publish(
            serde_json::to_string(&super::open_select_state())
                .unwrap()
                .as_bytes(),
        );
    }

    #[no_mangle]
    pub extern "C" fn tekto_select_value(id: f64) {
        let value = if id > 0.0 && id.is_finite() && id.fract() == 0.0 {
            super::select_value(id as u64)
        } else {
            None
        };
        publish(serde_json::to_string(&value).unwrap().as_bytes());
    }

    /// Return the current overlay id, or zero when none is open.
    #[no_mangle]
    pub extern "C" fn tekto_overlay_id() -> f64 {
        super::overlay_id().unwrap_or(0) as f64
    }

    #[no_mangle]
    pub extern "C" fn tekto_slider_begin(width: u32, height: u32, x: f32, y: f32) -> f64 {
        super::slider_begin(width, height, x, y).unwrap_or(0) as f64
    }

    #[no_mangle]
    pub extern "C" fn tekto_slider_move(id: f64, x: f32) -> u32 {
        if id <= 0.0 || !id.is_finite() || id.fract() != 0.0 {
            return 0;
        }
        u32::from(super::slider_move(id as u64, x))
    }

    #[no_mangle]
    pub extern "C" fn tekto_slider_key(code: u32) -> u32 {
        let (handled, activation) = super::slider_key_with_activation(code);
        publish(serde_json::to_string(&activation).unwrap().as_bytes());
        u32::from(handled)
    }

    /// Return one if Escape dismissed an overlay.
    #[no_mangle]
    pub extern "C" fn tekto_escape() -> u32 {
        u32::from(super::escape())
    }

    /// Returns the scroll offset, or NaN when there is no scroll target.
    #[no_mangle]
    pub extern "C" fn tekto_scroll_at(width: u32, height: u32, x: f32, y: f32, delta: f32) -> f32 {
        super::scroll_at(width, height, x, y, delta).unwrap_or(f32::NAN)
    }

    /// # Safety
    ///
    /// Same contract as [`tekto_apply_ops`].
    unsafe fn read_utf8(ptr: *const u8, len: u32) -> Result<String, String> {
        if ptr.is_null() {
            return Err("null ops pointer".into());
        }
        let bytes = std::slice::from_raw_parts(ptr, len as usize);
        String::from_utf8(bytes.to_vec()).map_err(|err| err.to_string())
    }
}

/// WebGPU present. Compiled only with `--features gpu` on wasm32.
/// The default module has no GPU imports, so `connectWasm` stays the same.
#[cfg(all(feature = "gpu", target_arch = "wasm32"))]
mod web_gpu {
    use wasm_bindgen::prelude::*;

    use super::*;

    /// Paint the current scene onto `canvas` with wgpu's WebGPU backend.
    /// The error string is the cue for the host to use the CPU blit.
    #[wasm_bindgen]
    pub async fn tekto_gpu_frame(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<(), JsValue> {
        let laid = TREE.with(|tree| {
            let mut laid = tree.borrow().clone();
            layout_tree(&mut laid, width as f32, height as f32);
            laid
        });
        tekto_render::paint_tree_webgpu(&laid, canvas, width, height, Color::PAGE)
            .await
            .map_err(|err| JsValue::from_str(&err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_ops_then_cpu_png() {
        reset();
        let created = apply_ops_json(
            r#"[
                {"op":"create","kind":"text"},
                {"op":"setText","id":1,"text":"Hello from WASM"},
                {"op":"setProp","id":1,"key":"fontSize","value":"18"}
            ]"#,
        )
        .expect("ops");
        assert_eq!(created, vec![1]);
        let png = render_png(120, 40).expect("png");
        assert!(png.starts_with(b"\x89PNG"), "not a PNG");
        assert!(png.len() > 32);
    }

    #[test]
    fn click_persists_focus_after_render() {
        reset();
        apply_ops_json(r#"[{"op":"create","kind":"button"}]"#).unwrap();
        render_png(100, 80).unwrap();
        assert_eq!(click_at(100, 80, 10.0, 10.0), Some(1));
        TREE.with(|tree| {
            let tree = tree.borrow();
            assert_eq!(tree.focus.map(|id| id.raw()), Some(1));
            assert_eq!(tree.last_click.map(|id| id.raw()), Some(1));
        });
    }

    #[test]
    fn unknown_kind_is_an_error() {
        reset();
        let err = apply_ops_json(r#"[{"op":"create","kind":"dom-node"}]"#).unwrap_err();
        assert!(err.contains("unknown node kind"));
    }

    #[test]
    fn reset_restarts_ids() {
        reset();
        apply_ops_json(r#"[{"op":"create","kind":"rect"}]"#).unwrap();
        reset();
        let created = apply_ops_json(r#"[{"op":"create","kind":"rect"}]"#).unwrap();
        assert_eq!(created, vec![1]);
    }
}
