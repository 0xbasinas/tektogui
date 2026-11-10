//! winit + softbuffer window that presents the CPU-rendered framebuffer.
//!
//! After every layout the AccessKit tree is rebuilt (same stable ids as
//! the scene tree). The optional `a11y-os` feature publishes that tree to
//! the OS. Live patches wake the event loop so the window and AccessKit
//! stay in sync.

use crate::{
    logical_from_physical, rgba_to_u32_softbuffer, AppRuntime, PlatformError, Viewport,
    WindowOptions,
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
    viewport: Arc<Mutex<Viewport>>,
    #[cfg(feature = "a11y-os")]
    adapter: accesskit_winit::Adapter,
}

struct App {
    opts: WindowOptions,
    runtime: AppRuntime,
    tree: Arc<Mutex<Tree>>,
    handle: Option<Arc<SharedTreeHandle>>,
    viewport: Arc<Mutex<Viewport>>,
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
            ));
        #[cfg(not(target_os = "android"))]
        let attrs = attrs.with_visible(false);
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
            viewport: Arc::clone(&self.viewport),
            #[cfg(feature = "a11y-os")]
            adapter,
        });
        if let Some(state) = self.state.as_mut() {
            redraw(state);
        }
        crate::lifecycle::on_resumed();
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        // Drop the window and softbuffer surface. On Android the surface is
        // invalid after Suspended; Month 2 GPU backends must do the same.
        self.state = None;
        crate::lifecycle::on_suspended();
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
            WindowEvent::CloseRequested => event_loop.exit(), // run_windowed then clear_endpoint()
            WindowEvent::RedrawRequested => {
                if let Some(state) = self.state.as_mut() {
                    redraw(state);
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(state) = self.state.as_mut() {
                    relayout_window(state);
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

fn window_logical_and_physical(window: &Window) -> (f32, f32, f32, u32, u32) {
    let physical = window.inner_size();
    let pw = physical.width.max(1);
    let ph = physical.height.max(1);
    let scale = (window.scale_factor().max(0.01)) as f32;
    let lw = pw as f32 / scale;
    let lh = ph as f32 / scale;
    (lw, lh, scale, pw, ph)
}

fn relayout_window(state: &mut State) {
    let (lw, lh, scale, pw, ph) = window_logical_and_physical(&state.window);
    let _ = state.renderer.resize(pw, ph);
    state.renderer.set_scale(scale);
    if let Ok(mut vp) = state.viewport.lock() {
        *vp = Viewport {
            width: lw.round().max(1.0) as u32,
            height: lh.round().max(1.0) as u32,
            scale,
        };
    }
    if let Ok(mut tree) = state.tree.lock() {
        layout_tree(&mut tree, lw, lh);
    }
}

fn redraw(state: &mut State) {
    relayout_window(state);
    let (_lw, _lh, _scale, pw, ph) = window_logical_and_physical(&state.window);
    if let Ok(tree) = state.tree.lock() {
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
    let Some(width) = NonZeroU32::new(pw) else {
        return;
    };
    let Some(height) = NonZeroU32::new(ph) else {
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

#[cfg(target_os = "android")]
pub fn run_android(
    runtime: AppRuntime,
    opts: WindowOptions,
    android_app: winit::platform::android::activity::AndroidApp,
) -> Result<(), PlatformError> {
    run_event_loop(runtime, opts, Some(android_app))
}

pub fn run_windowed(runtime: AppRuntime, opts: WindowOptions) -> Result<(), PlatformError> {
    #[cfg(target_os = "android")]
    {
        let _ = (runtime, opts);
        return Err(PlatformError::msg(
            "on Android, call tekto::run_android(...) from android_main",
        ));
    }
    #[cfg(not(target_os = "android"))]
    run_event_loop(runtime, opts, None)
}

#[cfg(target_os = "android")]
type AndroidAppArg = winit::platform::android::activity::AndroidApp;
#[cfg(not(target_os = "android"))]
type AndroidAppArg = ();

fn run_event_loop(
    runtime: AppRuntime,
    opts: WindowOptions,
    android_app: Option<AndroidAppArg>,
) -> Result<(), PlatformError> {
    let mut prepared = runtime.clone();
    prepared.layout(opts.width as f32, opts.height as f32);
    let mut builder = EventLoop::<AppEvent>::with_user_event();
    #[cfg(target_os = "android")]
    {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        let app = android_app
            .ok_or_else(|| PlatformError::msg("AndroidApp is required to start the event loop"))?;
        builder.with_android_app(app);
    }
    #[cfg(not(target_os = "android"))]
    let _ = android_app;
    let event_loop = builder
        .build()
        .map_err(|e| PlatformError::msg(e.to_string()))?;
    let (tree, handle, viewport) = if opts.dev_server {
        let proxy = event_loop.create_proxy();
        match crate::start_handle(&prepared, opts.width, opts.height) {
            Ok((handle, ep, viewport)) => {
                if let Ok(mut slot) = handle.on_change.lock() {
                    *slot = Some(Arc::new(move || {
                        let _ = proxy.send_event(AppEvent::TreeChanged);
                    }));
                }
                eprintln!("Tekto dev server at {}", ep.base_url());
                eprintln!("Try: tekto tree    or    tekto screenshot    or    tekto click <id>");
                (Arc::clone(&handle.tree), Some(handle), viewport)
            }
            Err(err) => {
                // Still show the window if the loopback server cannot bind
                // (seen on Android before INTERNET was in the manifest).
                eprintln!("Tekto dev server failed: {err}");
                (
                    Arc::new(Mutex::new(prepared.tree.clone())),
                    None,
                    Arc::new(Mutex::new(Viewport::new(opts.width, opts.height))),
                )
            }
        }
    } else {
        (
            Arc::new(Mutex::new(prepared.tree.clone())),
            None,
            Arc::new(Mutex::new(Viewport::new(opts.width, opts.height))),
        )
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    struct EndpointGuard;
    impl Drop for EndpointGuard {
        fn drop(&mut self) {
            tekto_mcp::clear_endpoint();
        }
    }
    let _endpoint_guard = EndpointGuard;
    let mut app = App {
        opts,
        runtime: prepared,
        tree,
        handle,
        viewport,
        state: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| PlatformError::msg(e.to_string()))?;
    tekto_mcp::clear_endpoint();
    Ok(())
}
