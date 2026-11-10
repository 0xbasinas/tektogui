//! First-class Rust app API for Tekto.
//!
//! TypeScript / JSX is also first-class, through `tekto-js` and the
//! `@tektogui/*` packages. Both talk to the same scene tree.

pub use tekto_a11y::{
    dump as accesskit_dump, tree_update as accesskit_tree_update, tree_update_with_focus,
    AccessKitDump,
};
#[cfg(feature = "js")]
pub use tekto_js::{apply_ops, eval_module, Host, HostOp, JsError, TreeHost};
pub use tekto_layout::layout_tree;
pub use tekto_mcp::{
    endpoint_path, read_endpoint, run_stdio_mcp, start_dev_server, AppHandle, DevEndpoint,
    SharedTreeHandle, ANDROID_DEV_PORT,
};
pub use tekto_native::{
    apply_native_op, current_platform, native, Clipboard, Files, NativeError, NativeOp,
    Notifications,
};
pub use tekto_platform::lifecycle::{
    handle_lifecycle, on_resumed, on_suspended, surface_is_live, AppLifecycle,
};
#[cfg(all(feature = "desktop", target_os = "android"))]
pub use tekto_platform::{run_android, run_android_with_session, AndroidApp};
pub use tekto_platform::{
    run_headless, run_windowed, run_windowed_with_session, start_handle, AppRuntime, DevSession,
    PlatformError, Viewport, WindowOptions,
};
pub use tekto_render::{paint_tree, render_tree_png, CpuRenderer, RenderError, Renderer};
pub use tekto_tree::{
    Action, Color, FieldKey, ImeEvent, LayoutStyle, Node, NodeId, NodeKind, NodePatch, RecordedKey,
    Recording, Rect, Tree, TreeDump, SCROLL_WHEEL_LINE,
};

pub use tekto_widgets::{
    banner, button, list, list_item, notes_tree, scroll, text, text_field, title_on_banner, window,
    ListIds, ListRow, BANNER_HEIGHT, BANNER_PADDING, BODY_SIZE, LIST_GAP, LIST_ROW_PADDING,
    PAGE_PADDING, STACK_GAP, TITLE_SIZE,
};

/// Layout, paint with the CPU renderer, and write a PNG if `out` is set.
pub fn render_headless_to_file(
    tree: &Tree,
    width: u32,
    height: u32,
    out: Option<&std::path::Path>,
) -> Result<Vec<u8>, PlatformError> {
    let runtime = AppRuntime::new(tree.clone(), "Tekto");
    run_headless(&runtime, width, height, out)
}

/// Start a window on desktop (Windows / Linux / macOS).
pub fn run(tree: Tree, opts: WindowOptions) -> Result<(), PlatformError> {
    let runtime = AppRuntime::new(tree, opts.title.clone());
    run_windowed(runtime, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_notes_png_is_valid() {
        let tree = notes_tree(640.0, 400.0).unwrap();
        let png = render_headless_to_file(&tree, 640, 400, None).expect("png");
        assert!(png.starts_with(b"\x89PNG"), "not a PNG");
        assert!(png.len() > 200);
        let pretty = tree.to_pretty_text();
        assert!(pretty.contains("Hello from Tekto"));
        assert!(accesskit_tree_update(&{
            let mut t = tree.clone();
            tekto_layout::layout_tree(&mut t, 640.0, 400.0);
            t
        })
        .is_some());
    }
}

/// Install a QuickJS applier on a dev-server handle so `POST /reload`
/// can swap the live tree for a new JS bundle without a Rust rebuild.
#[cfg(feature = "quickjs")]
pub fn install_js_runtime(
    handle: &std::sync::Arc<SharedTreeHandle>,
    viewport: &std::sync::Arc<std::sync::Mutex<Viewport>>,
) {
    use std::sync::Arc;
    use std::time::Instant;
    use tekto_js::eval_to_tree;

    let tree = Arc::clone(&handle.tree);
    let logs = Arc::clone(&handle.logs);
    let vp = Arc::clone(viewport);
    let apply: tekto_mcp::ApplyJsFn = Arc::new(move |js: &str| {
        let t0 = Instant::now();
        let mut next = eval_to_tree(js).map_err(|e| e.to_string())?;
        let (w, h) = vp.lock().map(|g| (g.width, g.height)).unwrap_or((640, 400));
        layout_tree(&mut next, w as f32, h as f32);
        *tree.lock().map_err(|e| e.to_string())? = next;
        let line = format!("js reload {}ms", t0.elapsed().as_millis());
        eprintln!("{line}");
        if let Ok(mut logs) = logs.lock() {
            logs.push(line);
        }
        Ok(())
    });
    *handle.apply_js.lock().expect("apply_js") = Some(apply);
}
