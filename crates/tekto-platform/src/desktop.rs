//! winit + softbuffer window that presents the CPU-rendered framebuffer.

use crate::{rgba_to_u32_softbuffer, AppRuntime, PlatformError, WindowOptions};
use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tekto_layout::layout_tree;
use tekto_render::{paint_tree, CpuRenderer, Renderer};
use tekto_tree::Tree;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

struct State {
    window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    renderer: CpuRenderer,
    tree: Arc<Mutex<Tree>>,
    clear: tekto_tree::Color,
}

struct App {
    opts: WindowOptions,
    runtime: AppRuntime,
    tree: Arc<Mutex<Tree>>,
    state: Option<State>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.opts.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.opts.width,
                self.opts.height,
            ));
        let window = Rc::new(event_loop.create_window(attrs).expect("window"));
        let context = Context::new(window.clone()).expect("softbuffer context");
        let surface = Surface::new(&context, window.clone()).expect("softbuffer surface");
        let size = window.inner_size();
        let renderer =
            CpuRenderer::new(size.width.max(1), size.height.max(1)).expect("cpu renderer");
        self.state = Some(State {
            window,
            surface,
            renderer,
            tree: Arc::clone(&self.tree),
            clear: self.runtime.clear,
        });
        if let Some(state) = self.state.as_mut() {
            redraw(state);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                if let Some(state) = self.state.as_mut() {
                    redraw(state);
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(state) = self.state.as_mut() {
                    let w = size.width.max(1);
                    let h = size.height.max(1);
                    let _ = state.renderer.resize(w, h);
                    if let Ok(mut tree) = state.tree.lock() {
                        layout_tree(&mut tree, w as f32, h as f32);
                    }
                    state.window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn redraw(state: &mut State) {
    let size = state.window.inner_size();
    let w = size.width.max(1);
    let h = size.height.max(1);
    let _ = state.renderer.resize(w, h);
    if let Ok(mut tree) = state.tree.lock() {
        layout_tree(&mut tree, w as f32, h as f32);
        paint_tree(&mut state.renderer, &tree, state.clear);
    }
    let Ok(rgba) = state.renderer.rgba8() else {
        return;
    };
    let Some(width) = NonZeroU32::new(w) else {
        return;
    };
    let Some(height) = NonZeroU32::new(h) else {
        return;
    };
    let _ = state.surface.resize(width, height);
    if let Ok(mut buf) = state.surface.buffer_mut() {
        let pixels = rgba_to_u32_softbuffer(&rgba);
        for (dst, src) in buf.iter_mut().zip(pixels.iter()) {
            *dst = *src;
        }
        let _ = buf.present();
    }
}

pub fn run_windowed(runtime: AppRuntime, opts: WindowOptions) -> Result<(), PlatformError> {
    let mut prepared = runtime.clone();
    prepared.layout(opts.width as f32, opts.height as f32);
    let tree = if opts.dev_server {
        let (handle, ep) = crate::start_handle(&prepared, opts.width, opts.height)?;
        eprintln!("Tekto dev server at {}", ep.base_url());
        eprintln!("Try: tekto tree    or    tekto screenshot");
        Arc::clone(&handle.tree)
    } else {
        Arc::new(Mutex::new(prepared.tree.clone()))
    };
    let event_loop = EventLoop::new().map_err(|e| PlatformError::msg(e.to_string()))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        opts,
        runtime: prepared,
        tree,
        state: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| PlatformError::msg(e.to_string()))?;
    tekto_mcp::clear_endpoint();
    Ok(())
}
