//! winit + softbuffer window that presents the CPU-rendered framebuffer.
//!
//! After every layout the AccessKit tree is rebuilt (same stable ids as
//! the scene tree). The optional `a11y-os` feature publishes that tree to
//! the OS. Live patches wake the event loop so the window and AccessKit
//! stay in sync.

use crate::{
    logical_from_physical, rgba_to_u32_softbuffer, AppRuntime, PlatformError, WindowOptions,
};
use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tekto_layout::layout_tree;
use tekto_mcp::{AppHandle, SharedTreeHandle};
use tekto_render::{paint_tree, CpuRenderer, Renderer};
use tekto_tree::Tree;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// Wakes the window when the dev server patches the tree.
#[derive(Clone, Copy, Debug)]
enum AppEvent {
    TreeChanged,
}

struct State {
    window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    renderer: CpuRenderer,
    tree: Arc<Mutex<Tree>>,
    clear: tekto_tree::Color,
    cursor: Option<(f32, f32)>,
    handle: Option<Arc<SharedTreeHandle>>,
    #[cfg(feature = "a11y-os")]
    adapter: accesskit_winit::Adapter,
}

struct App {
    opts: WindowOptions,
    runtime: AppRuntime,
    tree: Arc<Mutex<Tree>>,
    handle: Option<Arc<SharedTreeHandle>>,
    state: Option<State>,
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.opts.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.opts.width,
                self.opts.height,
            ))
            .with_visible(false);
        let window = event_loop.create_window(attrs).expect("window");
        #[cfg(feature = "a11y-os")]
        let adapter = {
            let tree = Arc::clone(&self.tree);
            accesskit_winit::Adapter::with_direct_handlers(
                event_loop,
                &window,
                LiveActivation {
                    tree: Arc::clone(&tree),
                },
                LiveActions,
                LiveDeactivate,
            )
        };
        window.set_visible(true);
        let window = Rc::new(window);
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
            cursor: None,
            handle: self.handle.clone(),
            #[cfg(feature = "a11y-os")]
            adapter,
        });
        if let Some(state) = self.state.as_mut() {
            redraw(state);
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: AppEvent) {
        if let Some(state) = self.state.as_mut() {
            state.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        #[cfg(feature = "a11y-os")]
        if let Some(state) = self.state.as_mut() {
            state.adapter.process_event(&state.window, &event);
        }
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
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(state) = self.state.as_mut() {
                    let scale = state.window.scale_factor();
                    state.cursor = Some(logical_from_physical(position.x, position.y, scale));
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(state) = self.state.as_mut() {
                    if let Some((x, y)) = state.cursor {
                        apply_pointer_click(state, x, y);
                    }
                }
            }
            _ => {}
        }
    }
}

fn publish_accesskit(
    tree: &Tree,
    #[cfg(feature = "a11y-os")] adapter: &mut accesskit_winit::Adapter,
) {
    let _ = tree;
    #[cfg(feature = "a11y-os")]
    adapter.update_if_active(|| {
        tekto_a11y::tree_update(tree).unwrap_or_else(|| accesskit::TreeUpdate {
            nodes: Vec::new(),
            tree: None,
            focus: accesskit::NodeId(0),
        })
    });
}

fn redraw(state: &mut State) {
    let size = state.window.inner_size();
    let w = size.width.max(1);
    let h = size.height.max(1);
    let _ = state.renderer.resize(w, h);
    if let Ok(mut tree) = state.tree.lock() {
        layout_tree(&mut tree, w as f32, h as f32);
        paint_tree(&mut state.renderer, &tree, state.clear);
        publish_accesskit(
            &tree,
            #[cfg(feature = "a11y-os")]
            &mut state.adapter,
        );
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

#[cfg(feature = "a11y-os")]
struct LiveActivation {
    tree: Arc<Mutex<Tree>>,
}

#[cfg(feature = "a11y-os")]
impl accesskit::ActivationHandler for LiveActivation {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        let tree = self.tree.lock().ok()?;
        tekto_a11y::tree_update(&tree)
    }
}

#[cfg(feature = "a11y-os")]
struct LiveActions;

#[cfg(feature = "a11y-os")]
impl accesskit::ActionHandler for LiveActions {
    fn do_action(&mut self, _request: accesskit::ActionRequest) {}
}

#[cfg(feature = "a11y-os")]
struct LiveDeactivate;

#[cfg(feature = "a11y-os")]
impl accesskit::DeactivationHandler for LiveDeactivate {
    fn deactivate_accessibility(&mut self) {}
}

fn apply_pointer_click(state: &mut State, x: f32, y: f32) {
    let id = {
        let Ok(tree) = state.tree.lock() else {
            return;
        };
        tree.hit(x, y)
    };
    let Some(id) = id else {
        return;
    };
    if let Some(handle) = &state.handle {
        let _ = handle.click(id);
    } else if let Ok(mut tree) = state.tree.lock() {
        let _ = tree.click(id);
    }
    state.window.request_redraw();
}

pub fn run_windowed(runtime: AppRuntime, opts: WindowOptions) -> Result<(), PlatformError> {
    let mut prepared = runtime.clone();
    prepared.layout(opts.width as f32, opts.height as f32);
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .map_err(|e| PlatformError::msg(e.to_string()))?;
    let (tree, handle) = if opts.dev_server {
        let proxy = event_loop.create_proxy();
        let (handle, ep) = crate::start_handle(&prepared, opts.width, opts.height)?;
        if let Ok(mut slot) = handle.on_change.lock() {
            *slot = Some(Arc::new(move || {
                let _ = proxy.send_event(AppEvent::TreeChanged);
            }));
        }
        eprintln!("Tekto dev server at {}", ep.base_url());
        eprintln!("Try: tekto tree    or    tekto screenshot    or    tekto click <id>");
        (Arc::clone(&handle.tree), Some(handle))
    } else {
        (Arc::new(Mutex::new(prepared.tree.clone())), None)
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        opts,
        runtime: prepared,
        tree,
        handle,
        state: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| PlatformError::msg(e.to_string()))?;
    tekto_mcp::clear_endpoint();
    Ok(())
}
