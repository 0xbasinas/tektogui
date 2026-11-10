//! First-class Rust app API for Tekto.
//!
//! TypeScript / JSX is also first-class, through `tekto-js` and the
//! `@tektogui/*` packages. Both talk to the same scene tree.

pub use tekto_a11y::{
    dump as accesskit_dump, tree_update as accesskit_tree_update, tree_update_with_focus,
    AccessKitDump,
};
pub use tekto_layout::layout_tree;
pub use tekto_mcp::{
    endpoint_path, read_endpoint, run_stdio_mcp, start_dev_server, AppHandle, DevEndpoint,
    SharedTreeHandle, ANDROID_DEV_PORT,
};
pub use tekto_native::{current_platform, native, Clipboard, Files, NativeError, Notifications};
pub use tekto_platform::lifecycle::{
    handle_lifecycle, on_resumed, on_suspended, surface_is_live, AppLifecycle,
};
#[cfg(all(feature = "desktop", target_os = "android"))]
pub use tekto_platform::{run_android, AndroidApp};
pub use tekto_platform::{
    run_headless, run_windowed, start_handle, AppRuntime, PlatformError, Viewport, WindowOptions,
};
pub use tekto_render::{paint_tree, render_tree_png, CpuRenderer, RenderError, Renderer};
pub use tekto_tree::{Color, LayoutStyle, Node, NodeId, NodeKind, NodePatch, Rect, Tree, TreeDump};
pub use tekto_widgets::{
    banner, notes_tree, text, title_on_banner, window, BANNER_HEIGHT, BANNER_PADDING, BODY_SIZE,
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
