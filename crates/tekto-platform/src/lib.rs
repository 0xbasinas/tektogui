//! Platform layer.
//!
//! Desktop (Windows, Linux, macOS) uses winit + softbuffer to present
//! the CPU renderer. Android and web share the same `AppRuntime` and
//! headless path; their window shells come in later issues, but the
//! traits are here so those backends can plug in without changing apps.

use std::sync::{Arc, Mutex};
use tekto_layout::layout_tree;
use tekto_mcp::{McpError, SharedTreeHandle};
use tekto_render::{render_tree_png, render_tree_png_scaled, RenderError};
use tekto_tree::{Color, Tree};

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error(transparent)]
    Mcp(#[from] McpError),
}

impl PlatformError {
    pub fn msg(m: impl Into<String>) -> Self {
        PlatformError::Message(m.into())
    }
}

#[derive(Clone, Debug)]
pub struct WindowOptions {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub dev_server: bool,
    /// Draw the OS title bar. Set false to paint your own chrome; the app then
    /// needs a drag region and window controls.
    pub decorations: bool,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: "Tekto".into(),
            width: 640,
            height: 400,
            dev_server: true,
            decorations: true,
        }
    }
}

/// Logical size and scale used to paint a live window (and its screenshots).
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

impl Viewport {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            scale: 1.0,
        }
    }

    pub fn physical_width(self) -> u32 {
        ((self.width as f32) * self.scale.max(0.01))
            .round()
            .max(1.0) as u32
    }

    pub fn physical_height(self) -> u32 {
        ((self.height as f32) * self.scale.max(0.01))
            .round()
            .max(1.0) as u32
    }
}

/// Handle, HTTP endpoint, and live viewport returned by [`start_handle`].
pub type DevSession = (
    Arc<SharedTreeHandle>,
    tekto_mcp::DevEndpoint,
    Arc<Mutex<Viewport>>,
);

/// A running app's scene, ready to layout and paint.
#[derive(Clone)]
pub struct AppRuntime {
    pub tree: Tree,
    pub clear: Color,
    pub title: String,
}

impl AppRuntime {
    pub fn new(tree: Tree, title: impl Into<String>) -> Self {
        Self {
            tree,
            clear: Color::PAGE,
            title: title.into(),
        }
    }

    pub fn layout(&mut self, width: f32, height: f32) {
        layout_tree(&mut self.tree, width, height);
    }

    pub fn render_png(&self, width: u32, height: u32) -> Result<Vec<u8>, PlatformError> {
        let mut tree = self.tree.clone();
        layout_tree(&mut tree, width as f32, height as f32);
        Ok(render_tree_png(&tree, width, height, self.clear)?)
    }
}

/// Headless: layout, paint with the CPU renderer, optionally write a PNG.
pub fn run_headless(
    runtime: &AppRuntime,
    width: u32,
    height: u32,
    out: Option<&std::path::Path>,
) -> Result<Vec<u8>, PlatformError> {
    let png = runtime.render_png(width, height)?;
    if let Some(path) = out {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, &png).map_err(|e| PlatformError::msg(e.to_string()))?;
    }
    Ok(png)
}

/// Start the dev HTTP server for this runtime (used by headless and windowed).
pub fn start_handle(
    runtime: &AppRuntime,
    width: u32,
    height: u32,
) -> Result<DevSession, PlatformError> {
    let (handle, viewport) = create_handle(runtime, width, height);
    let ep = tekto_mcp::start_dev_server(handle.clone())?;
    Ok((handle, ep, viewport))
}

/// Create a live action handle without starting a dev server.
pub fn create_handle(
    runtime: &AppRuntime,
    width: u32,
    height: u32,
) -> (Arc<SharedTreeHandle>, Arc<Mutex<Viewport>>) {
    let mut tree = runtime.tree.clone();
    layout_tree(&mut tree, width as f32, height as f32);
    let tree = Arc::new(Mutex::new(tree));
    let clear = runtime.clear;
    let viewport = Arc::new(Mutex::new(Viewport::new(width, height)));
    let render = {
        let viewport = Arc::clone(&viewport);
        Arc::new(move |t: &Tree| {
            let vp = viewport
                .lock()
                .map(|g| *g)
                .unwrap_or(Viewport::new(width, height));
            let (lw, lh) = match t.root().and_then(|id| t.get(id)) {
                Some(n) if n.bounds.width > 0.0 && n.bounds.height > 0.0 => (
                    n.bounds.width.round().max(1.0) as u32,
                    n.bounds.height.round().max(1.0) as u32,
                ),
                _ => (vp.width, vp.height),
            };
            render_tree_png_scaled(t, lw, lh, vp.scale, clear)
                .map_err(|e| McpError::msg(e.to_string()))
        }) as Arc<dyn Fn(&Tree) -> Result<Vec<u8>, McpError> + Send + Sync>
    };
    let handle = Arc::new(SharedTreeHandle {
        tree,
        render,
        logs: Arc::new(Mutex::new(vec!["tekto dev server started".into()])),
        on_change: Mutex::new(None),
        pending_js: Mutex::new(None),
        apply_js: Mutex::new(None),
        last_writeback: Mutex::new(None),
        action_gate: Arc::new(Mutex::new(())),
        click_js: Mutex::new(None),
        input_js: Mutex::new(None),
        forget_js: Mutex::new(None),
        history: Mutex::new(None),
    });
    (handle, viewport)
}

pub fn rgba_to_u32_softbuffer(rgba: &[u8]) -> Vec<u32> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let r = p[0] as u32;
            let g = p[1] as u32;
            let b = p[2] as u32;
            let a = p[3] as u32;
            (a << 24) | (r << 16) | (g << 8) | b
        })
        .collect()
}

mod gpu_policy;
#[cfg(any(
    test,
    all(
        feature = "desktop",
        not(target_arch = "wasm32"),
        not(target_os = "ios")
    )
))]
pub mod pointer_route;

#[cfg(all(
    feature = "desktop",
    not(target_arch = "wasm32"),
    not(target_os = "ios")
))]
pub mod key_route;
pub use gpu_policy::{gpu_paint_allowed, wants_gpu_paint};

#[cfg(any(test, all(feature = "desktop", target_os = "android")))]
mod android_field;
mod android_insets;
pub use android_insets::{insets_from_physical, layout_with_insets, EdgeInsets, WindowInsets};
#[cfg(all(feature = "desktop", target_os = "android"))]
mod android_overlay;
#[cfg(all(
    feature = "desktop",
    not(target_arch = "wasm32"),
    not(target_os = "ios")
))]
mod desktop;
#[cfg(all(feature = "desktop", not(target_os = "android")))]
pub use desktop::run_windowed_with_handle;
#[cfg(all(
    feature = "desktop",
    target_os = "android",
    not(target_arch = "wasm32"),
    not(target_os = "ios")
))]
pub use desktop::{run_android, run_android_with_session};
#[cfg(all(
    feature = "desktop",
    not(target_arch = "wasm32"),
    not(target_os = "ios")
))]
pub use desktop::{run_windowed, run_windowed_with_session};

#[cfg(not(all(
    feature = "desktop",
    not(target_arch = "wasm32"),
    not(target_os = "ios")
)))]
pub fn run_windowed(_runtime: AppRuntime, _opts: WindowOptions) -> Result<(), PlatformError> {
    Err(PlatformError::msg(
        "windowed mode is not available on this target yet; use headless, or build with the desktop feature",
    ))
}

#[cfg(not(all(
    feature = "desktop",
    not(target_arch = "wasm32"),
    not(target_os = "ios")
)))]
pub fn run_windowed_with_session(
    _runtime: AppRuntime,
    _opts: WindowOptions,
    _session: Option<DevSession>,
) -> Result<(), PlatformError> {
    run_windowed(_runtime, _opts)
}

/// The NativeActivity app handle passed to `android_main`.
#[cfg(all(feature = "desktop", target_os = "android"))]
pub use winit::platform::android::activity::AndroidApp;

/// Convert a physical cursor position to logical pixels using the window
/// scale factor. Used so hit-testing matches layout (which is in logical px).
/// Whether a `KeyboardInput` string should be inserted after an IME commit.
///
/// Returns false once when `text` repeats `last_ime_commit`, then clears it.
/// Windows can deliver the committed string again as a key event in the same
/// burst. Call [`expire_ime_echo`] when the event queue goes idle so a later
/// press of the same string is real typing (Linux usually has no echo).
/// Any other string is accepted and the latch is cleared.
pub fn accept_keyboard_text(last_ime_commit: &mut Option<String>, text: &str) -> bool {
    if last_ime_commit.as_deref() == Some(text) {
        *last_ime_commit = None;
        return false;
    }
    *last_ime_commit = None;
    !text.is_empty()
}

/// Drop a pending IME echo. A matching key after this is genuine.
pub fn expire_ime_echo(last_ime_commit: &mut Option<String>) {
    *last_ime_commit = None;
}

pub fn logical_from_physical(x: f64, y: f64, scale_factor: f64) -> (f32, f32) {
    let scale = if scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    ((x / scale) as f32, (y / scale) as f32)
}

#[cfg(test)]
mod tests {
    use super::{accept_keyboard_text, expire_ime_echo, logical_from_physical};

    #[test]
    fn ime_commit_echo_is_dropped_once() {
        let mut latch = Some("é".to_string());
        assert!(!accept_keyboard_text(&mut latch, "é"));
        assert!(latch.is_none());
        assert!(accept_keyboard_text(&mut latch, "é"));
        let mut latch = Some("啊不".to_string());
        assert!(accept_keyboard_text(&mut latch, "a"));
        assert!(latch.is_none());
    }

    #[test]
    fn genuine_key_after_idle_is_not_swallowed_by_ime_commit() {
        // Commit of "a" with no echo in the burst, then a real "a".
        let mut latch = Some("a".to_string());
        expire_ime_echo(&mut latch);
        assert!(latch.is_none());
        assert!(accept_keyboard_text(&mut latch, "a"));
        assert_eq!(latch, None);
    }

    #[test]
    fn logical_from_physical_divides_by_scale() {
        let (x, y) = logical_from_physical(200.0, 100.0, 2.0);
        assert!((x - 100.0).abs() < f32::EPSILON);
        assert!((y - 50.0).abs() < f32::EPSILON);
    }
}

pub mod lifecycle;
