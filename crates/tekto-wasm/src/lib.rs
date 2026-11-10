//! WASM entry for the Tekto core.
//!
//! The browser JavaScript engine sends the same [`tekto_js::HostOp`] JSON
//! that desktop QuickJS uses, then asks for a CPU-renderer PNG. This crate
//! does not link QuickJS, wgpu, or any HTML/CSS/DOM type. The host page
//! blits those PNG bytes with `@tektogui/wasm/canvas` (`paintCanvas`).
//!
//! On `wasm32-unknown-unknown` the cdylib exports a tiny C ABI
//! (`tekto_alloc`, `tekto_apply_ops`, `tekto_render_png`, `tekto_reset`,
//! `tekto_out_ptr`, `tekto_out_len`). `@tektogui/wasm` is the JS side.
//! Copy `tekto_out_*` bytes before the next call; the module reuses one
//! output buffer.

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
        let mut laid = tree.borrow().clone();
        layout_tree(&mut laid, width as f32, height as f32);
        render_tree_png(&laid, width, height, Color::PAGE).map_err(|err| err.to_string())
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
