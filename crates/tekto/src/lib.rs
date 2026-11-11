//! First-class Rust app API for Tekto.
//!
//! TypeScript / JSX is also first-class, through `tekto-js` and the
//! `@tektogui/*` packages. Both talk to the same scene tree.

pub use tekto_a11y::{
    dump as accesskit_dump, tree_update as accesskit_tree_update, tree_update_with_focus,
    AccessKitDump,
};
#[cfg(feature = "quickjs")]
pub use tekto_js::compile_bytecode;
#[cfg(feature = "js")]
pub use tekto_js::{
    apply_ops, eval_module, Host, HostOp, JsError, NativeCapability, NativePermissions, TreeHost,
};
#[cfg(feature = "js")]
pub use tekto_js::{decode_bytecode, encode_bytecode, BYTECODE_ENGINE, BYTECODE_FORMAT};
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
    ease_smoothstep, reduced_motion_requested, Action, Color, FieldKey, Image, ImageError,
    ImageFit, ImeEvent, LayoutStyle, Node, NodeId, NodeKind, NodePatch, Overlay, OverlaySide,
    RecordedKey, Recording, Rect, StateInfo, Store, StoreError, StyleTransition, Tree, TreeDump,
    TreeEvent, DEFAULT_TRANSITION_MS, LIST_OVERSCAN_PX, SCROLL_WHEEL_LINE,
};

pub use tekto_widgets::{
    apply_axis, banner, button, checkbox, dialog, image, list, list_item, menu, menu_item,
    notes_tree, progress, radio, radio_group, responsive_axis, scroll, select, select_option,
    slider, split_pane, switch, tab, tab_list, tab_panel, text, text_area, text_field,
    title_on_banner, window, Axis, KeyedList, ListIds, ListRow, SplitPane, BANNER_HEIGHT,
    BANNER_PADDING, BODY_SIZE, LIST_GAP, LIST_ROW_PADDING, PAGE_PADDING, STACK_GAP, TITLE_SIZE,
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

    /// Controls, a scrolling list and an open menu overlay must paint alike
    /// on CPU and GPU.
    #[test]
    fn gpu_matches_cpu_for_controls_list_and_overlay() {
        let mut gpu = match GpuRenderer::new(640, 400) {
            Ok(renderer) => renderer,
            Err(err) => {
                eprintln!("gpu parity: no adapter: {err}");
                return;
            }
        };
        let mut tree = notes_tree(640.0, 400.0).unwrap();
        let root = tree.root().unwrap();
        let save = button(&mut tree, root, "Save").unwrap();
        checkbox(&mut tree, root, "Pinned", true).unwrap();
        switch(&mut tree, root, "Sync", true).unwrap();
        progress(&mut tree, root, "Upload", 0.4).unwrap();
        slider(&mut tree, root, "Size", 0.6).unwrap();
        let group = radio_group(&mut tree, root, "Sort").unwrap();
        radio(&mut tree, group, "Date", true).unwrap();
        radio(&mut tree, group, "Name", false).unwrap();
        let rows = [
            ListRow {
                label: "One",
                height: 36.0,
            },
            ListRow {
                label: "Two",
                height: 36.0,
            },
            ListRow {
                label: "Three",
                height: 36.0,
            },
        ];
        list(&mut tree, root, f32::NAN, 60.0, &rows).unwrap();
        let ctx = menu(&mut tree, "Actions").unwrap();
        menu_item(&mut tree, ctx, "Copy").unwrap();
        menu_item(&mut tree, ctx, "Paste").unwrap();
        layout_tree(&mut tree, 640.0, 400.0);
        tree.open_overlay(ctx, save, OverlaySide::Below).unwrap();
        layout_tree(&mut tree, 640.0, 400.0);
        let mut cpu = CpuRenderer::new(640, 400).unwrap();
        paint_tree(&mut cpu, &tree, Color::PAGE);
        paint_tree(&mut gpu, &tree, Color::PAGE);
        let (a, b) = (cpu.rgba8().unwrap(), gpu.rgba8().unwrap());
        let differing = a
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0.iter())
            .filter(|(x, y)| x.iter().zip(*y).any(|(p, q)| p.abs_diff(*q) > 48))
            .count();
        let total = a.len() / 4;
        assert!(
            differing * 100 < total * 3,
            "cpu and gpu differ on {differing} of {total} pixels"
        );
    }

    /// A modal dialog and an open select dropdown, each at scale 1.0 and 1.25
    /// (a common Windows display scale), must paint alike on CPU and GPU.
    #[test]
    fn gpu_matches_cpu_for_dialog_select_and_display_scale() {
        for scale in [1.0_f32, 1.25] {
            for dialog_case in [true, false] {
                let (w, h) = ((640.0 * scale) as u32, (400.0 * scale) as u32);
                let mut gpu = match GpuRenderer::new(w, h) {
                    Ok(renderer) => renderer,
                    Err(err) => {
                        eprintln!("gpu parity: no adapter: {err}");
                        return;
                    }
                };
                gpu.set_scale(scale);
                let mut cpu = CpuRenderer::new(w, h).unwrap();
                cpu.set_scale(scale);
                let mut tree = notes_tree(640.0, 400.0).unwrap();
                let root = tree.root().unwrap();
                let name = if dialog_case { "dialog" } else { "select" };
                if dialog_case {
                    let anchor = button(&mut tree, root, "Delete").unwrap();
                    let panel = dialog(&mut tree, "Confirm").unwrap();
                    text(&mut tree, panel, "Delete this note?", BODY_SIZE, Color::INK).unwrap();
                    button(&mut tree, panel, "Cancel").unwrap();
                    layout_tree(&mut tree, 640.0, 400.0);
                    tree.open_overlay(panel, anchor, OverlaySide::Center)
                        .unwrap();
                } else {
                    let pick = select(&mut tree, root, "Sort").unwrap();
                    select_option(&mut tree, pick, "Date", true).unwrap();
                    select_option(&mut tree, pick, "Name", false).unwrap();
                    layout_tree(&mut tree, 640.0, 400.0);
                    tree.open_select(pick).unwrap();
                }
                layout_tree(&mut tree, 640.0, 400.0);
                paint_tree(&mut cpu, &tree, Color::PAGE);
                paint_tree(&mut gpu, &tree, Color::PAGE);
                let (a, b) = (cpu.rgba8().unwrap(), gpu.rgba8().unwrap());
                let differing = a
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(b.as_chunks::<4>().0.iter())
                    .filter(|(x, y)| x.iter().zip(*y).any(|(p, q)| p.abs_diff(*q) > 48))
                    .count();
                let total = a.len() / 4;
                assert!(
                    differing * 100 < total * 3,
                    "{name} at scale {scale}: cpu and gpu differ on {differing} of {total} pixels"
                );
            }
        }
    }

    /// The same tree (tabs, multiline field, selection, IME composition)
    /// painted by CPU and GPU must agree on most pixels. Anti-aliasing and
    /// glyph rasterizers differ, so allow a small per-pixel and total drift.
    #[test]
    fn gpu_matches_cpu_for_tabs_fields_and_composition() {
        let mut gpu = match GpuRenderer::new(640, 400) {
            Ok(renderer) => renderer,
            Err(err) => {
                eprintln!("gpu parity: no adapter: {err}");
                return;
            }
        };
        let mut tree = notes_tree(640.0, 400.0).unwrap();
        let root = tree.root().unwrap();
        let tabs = tab_list(&mut tree, root, "Sections").unwrap();
        let edit = tab(&mut tree, tabs, "Edit", true).unwrap();
        let about = tab(&mut tree, tabs, "About", false).unwrap();
        let panel = tab_panel(&mut tree, root, edit, "Edit panel").unwrap();
        let title = text_field(&mut tree, panel, "Title", "Groceries").unwrap();
        let body = text_area(
            &mut tree,
            panel,
            "Body",
            "Milk
Eggs
Bread",
            f32::NAN,
            120.0,
        )
        .unwrap();
        let other = tab_panel(&mut tree, root, about, "About panel").unwrap();
        text(&mut tree, other, "About", BODY_SIZE, Color::INK).unwrap();
        tree.set_field_selection(body, 1, 8).unwrap();
        tree.set_field_selection(title, 9, 9).unwrap();
        tree.apply_ime(ImeEvent::Preedit {
            text: "ab".into(),
            cursor: Some((0, 1)),
        })
        .unwrap();
        layout_tree(&mut tree, 640.0, 400.0);
        let mut cpu = CpuRenderer::new(640, 400).unwrap();
        paint_tree(&mut cpu, &tree, Color::PAGE);
        paint_tree(&mut gpu, &tree, Color::PAGE);
        let (a, b) = (cpu.rgba8().unwrap(), gpu.rgba8().unwrap());
        assert_eq!(a.len(), b.len());
        let differing = a
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0.iter())
            .filter(|(x, y)| x.iter().zip(*y).any(|(p, q)| p.abs_diff(*q) > 48))
            .count();
        let total = a.len() / 4;
        assert!(
            differing * 100 < total * 3,
            "cpu and gpu differ on {differing} of {total} pixels"
        );
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
    let input_owner = owner.clone();
    *handle.input_js.lock().expect("input_js") = Some(std::sync::Arc::new(move |id, value| {
        input_owner
            .dispatch_input_locked(id.raw(), value)
            .map(|_| ())
    }));
    let forget_owner = owner.clone();
    *handle.forget_js.lock().expect("forget_js") = Some(std::sync::Arc::new(move |id| {
        forget_owner.forget_node_locked(id.raw())
    }));
    let reload = owner.clone();
    *handle.apply_js.lock().expect("apply_js") =
        Some(std::sync::Arc::new(move |js| reload.mount(js).map(|_| ())));
    owner
}
