//! Platform layer.
//!
//! Desktop (Windows, Linux, macOS) uses winit + softbuffer to present
//! the CPU renderer. Android and web share the same `AppRuntime` and
//! headless path; their window shells come in later issues, but the
//! traits are here so those backends can plug in without changing apps.

use std::sync::{Arc, Mutex};
use tekto_layout::layout_tree;
use tekto_mcp::{McpError, SharedTreeHandle};
use tekto_render::{render_tree_png, RenderError};
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
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: "Tekto".into(),
            width: 640,
            height: 400,
            dev_server: true,
        }
    }
}

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
) -> Result<(Arc<SharedTreeHandle>, tekto_mcp::DevEndpoint), PlatformError> {
    let mut tree = runtime.tree.clone();
    layout_tree(&mut tree, width as f32, height as f32);
    let tree = Arc::new(Mutex::new(tree));
    let clear = runtime.clear;
    let render = {
        let _tree = Arc::clone(&tree);
        Arc::new(move |t: &Tree| {
            render_tree_png(t, width, height, clear).map_err(|e| McpError::msg(e.to_string()))
        }) as Arc<dyn Fn(&Tree) -> Result<Vec<u8>, McpError> + Send + Sync>
    };
    let handle = Arc::new(SharedTreeHandle {
        tree,
        render,
        logs: Arc::new(Mutex::new(vec!["tekto dev server started".into()])),
    });
    let ep = tekto_mcp::start_dev_server(handle.clone())?;
    Ok((handle, ep))
}

pub fn rgba_to_u32_softbuffer(rgba: &[u8]) -> Vec<u32> {
    rgba.as_chunks::<4>().0.iter()
        .map(|p| {
            let r = p[0] as u32;
            let g = p[1] as u32;
            let b = p[2] as u32;
            let a = p[3] as u32;
            (a << 24) | (r << 16) | (g << 8) | b
        })
        .collect()
}

#[cfg(all(
    feature = "desktop",
    not(target_os = "android"),
    not(target_arch = "wasm32"),
    not(target_os = "ios")
))]
mod desktop;
#[cfg(all(
    feature = "desktop",
    not(target_os = "android"),
    not(target_arch = "wasm32"),
    not(target_os = "ios")
))]
pub use desktop::run_windowed;

#[cfg(not(all(
    feature = "desktop",
    not(target_os = "android"),
    not(target_arch = "wasm32"),
    not(target_os = "ios")
)))]
pub fn run_windowed(_runtime: AppRuntime, _opts: WindowOptions) -> Result<(), PlatformError> {
    Err(PlatformError::msg(
        "windowed mode is not available on this target yet; use headless, or build with the desktop feature on Windows/Linux/macOS",
    ))
}
