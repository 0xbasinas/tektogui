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
    apply_native_op, current_platform, native, Clipboard, Files, GlobalShortcuts, NativeError,
    NativeOp, Notifications, SystemTray, TrayMenuItem,
};
pub use tekto_platform::lifecycle::{
    handle_lifecycle, on_resumed, on_suspended, surface_is_live, AppLifecycle,
};
#[cfg(all(feature = "desktop", not(target_os = "android")))]
pub use tekto_platform::run_windowed_with_handle;
pub use tekto_platform::{
    create_handle, run_headless, run_windowed, run_windowed_with_session, start_handle, AppRuntime,
    DevSession, PlatformError, Viewport, WindowOptions,
};
#[cfg(all(feature = "desktop", target_os = "android"))]
pub use tekto_platform::{run_android, run_android_with_session, AndroidApp};
pub use tekto_render::{
    paint_tree, render_tree_png, CpuRenderer, GpuRenderer, RenderError, Renderer,
};
pub use tekto_text::set_fallback_font;
pub use tekto_tree::{
    ease_smoothstep, reduced_motion_requested, Action, Color, FieldKey, ImeEvent, LayoutStyle,
    Node, NodeId, NodeKind, NodePatch, RecordedKey, Recording, Rect, StateInfo, Store, StoreError,
    StyleTransition, Tree, TreeDump, TreeEvent, DEFAULT_TRANSITION_MS, LIST_OVERSCAN_PX,
    SCROLL_WHEEL_LINE,
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

#[cfg(all(test, feature = "gpu"))]
mod gpu_notes {
    use super::*;

    #[test]
    fn gpu_notes_tree_or_no_adapter() {
        let mut renderer = match GpuRenderer::new(640, 400) {
            Ok(renderer) => renderer,
            Err(err) => {
                eprintln!("gpu notes: no adapter: {err}");
                return;
            }
        };
        eprintln!("gpu notes adapter: {}", renderer.adapter_name());
        let mut tree = notes_tree(640.0, 400.0).unwrap();
        layout_tree(&mut tree, 640.0, 400.0);
        paint_tree(&mut renderer, &tree, Color::PAGE);
        let png = renderer.to_png().expect("gpu notes png");
        assert!(png.starts_with(b"\x89PNG"));
        assert!(png.len() > 500, "gpu notes png too small");
        let shot = std::path::Path::new("/workspace/tektogui-shots/i32-gpu-notes.png");
        if let Some(parent) = shot.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(shot, &png);
    }
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

#[cfg(feature = "quickjs")]
mod js_owner;
#[cfg(feature = "quickjs")]
pub use js_owner::JsAppOwner;

/// Attach a retained QuickJS owner to reloads. The returned handle can mount
/// the initial bundle and dispatch validated, generation-tagged clicks.
#[cfg(feature = "quickjs")]
pub fn install_js_runtime(
    handle: &std::sync::Arc<SharedTreeHandle>,
    viewport: &std::sync::Arc<std::sync::Mutex<Viewport>>,
) -> JsAppOwner {
    let owner = JsAppOwner::new(handle, viewport);
    let click_owner = owner.clone();
    *handle.click_js.lock().expect("click_js") = Some(std::sync::Arc::new(move |id| {
        click_owner.dispatch_click_locked(id.raw()).map(|_| ())
    }));
    let reload = owner.clone();
    *handle.apply_js.lock().expect("apply_js") =
        Some(std::sync::Arc::new(move |js| reload.mount(js).map(|_| ())));
    owner
}
