//! winit + softbuffer window. The default paint path is the CPU renderer.
//! With the `gpu` feature, desktop wgpu / Vello paints the frame and the
//! same softbuffer blit presents it. If no adapter is available, or a
//! readback fails while the surface epoch is still live, the window falls
//! back to the CPU renderer.
//!
//! Android does not try the GPU unless `TEKTO_ANDROID_GPU=1` (and the
//! `gpu` feature). Drivers were not proven in this repo. A retired surface
//! epoch never calls `buffer_mut` or `present`.
//!
//! After every layout the AccessKit tree is rebuilt (same stable ids as
//! the scene tree). The optional `a11y-os` feature publishes that tree to
//! the OS. Live patches wake the event loop so the window and AccessKit
//! stay in sync.

use crate::android_insets::{layout_with_insets, WindowInsets};
use crate::pointer_route::{apply_pointer_op, FingerPhase, PointerOp, PointerSession};
use crate::{
    logical_from_physical, rgba_to_u32_softbuffer, AppRuntime, PlatformError, Viewport,
    WindowOptions,
};
use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tekto_mcp::SharedTreeHandle;
use tekto_render::{paint_tree, CpuRenderer, Renderer};
use tekto_tree::{ImeEvent, Tree, SCROLL_WHEEL_LINE};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{
    ElementState, Ime, KeyEvent, Modifiers, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{CursorIcon, ImePurpose, ResizeDirection, Window, WindowId};

/// Wakes the window when the dev server patches the tree.
#[derive(Clone, Copy, Debug)]
enum AppEvent {
    TreeChanged,
}

/// One edge-resize gesture on an undecorated window.
#[derive(Clone, Copy)]
struct ResizeSession {
    direction: ResizeDirection,
    /// Physical screen position of the pointer when the press started.
    start: (f64, f64),
    /// Physical outer position at the start.
    origin: (i32, i32),
    /// Physical inner size at the start.
    size: (u32, u32),
    scale: f64,
    /// Last rect sent to the OS, so a still pointer sends nothing.
    last: (i32, i32, u32, u32),
    /// Newest rect waiting for the next frame.
    pending: Option<(i32, i32, u32, u32)>,
}

/// Distance from an edge that starts a resize, in logical pixels.
const RESIZE_EDGE: f32 = 6.0;
/// Minimum logical content size while resizing.
const MIN_RESIZE_WIDTH: f32 = 240.0;
const MIN_RESIZE_HEIGHT: f32 = 160.0;
/// Distance from a monitor edge that snaps the window, in logical pixels.
const SNAP_EDGE: f32 = 8.0;

struct State {
    /// Dropped before [`State::window`] so the Android `ANativeWindow` is
    /// released while the winit window handle is still alive.
    surface: Surface<Rc<Window>, Rc<Window>>,
    /// Dropped before `window`: a wgpu surface created from the HWND must
    /// go away while the window still exists.
    renderer: Painter,
    window: Rc<Window>,
    epoch: crate::lifecycle::SurfaceEpoch,
    tree: Arc<Mutex<Tree>>,
    clear: tekto_tree::Color,
    /// Mouse and touch share one gesture. See [`crate::pointer_route`].
    pointer: PointerSession,
    modifiers: Modifiers,
    /// `Window::set_ime_allowed` follows text-field focus (desktop).
    ime_allowed: bool,
    /// Android `EditText` overlay. Absent on desktop.
    #[cfg(target_os = "android")]
    overlay: Option<crate::android_overlay::Overlay>,
    /// Last IME commit, so a following key event does not insert it again.
    last_ime_commit: Option<String>,
    handle: Option<Arc<SharedTreeHandle>>,
    viewport: Arc<Mutex<Viewport>>,
    /// Origin for [`Tree::advance`]. Zero at the first frame of this surface.
    anim_origin: Instant,
    /// Set by a close control; the event loop exits on the next wake.
    close_requested: bool,
    /// Active edge resize for an undecorated window.
    resize: Option<ResizeSession>,
    /// Last press on the drag region, for double-click to maximise.
    last_title_press: Option<(Instant, (f32, f32))>,
    /// A bar drag is in progress and may end at a screen edge.
    snap_drag: bool,
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
        // A second `resumed` (new native window without a paired drop) must
        // not keep the previous softbuffer. Release it before creating another.
        if self.state.is_some() {
            self.release_surface();
        }
        let attrs = Window::default_attributes()
            .with_title(self.opts.title.clone())
            .with_decorations(self.opts.decorations)
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
        let renderer = Painter::new(&window, size.width.max(1), size.height.max(1));
        let epoch = crate::lifecycle::begin_surface();
        self.state = Some(State {
            surface,
            window,
            epoch,
            renderer,
            tree: Arc::clone(&self.tree),
            clear: self.runtime.clear,
            pointer: PointerSession::default(),
            modifiers: Modifiers::default(),
            ime_allowed: false,
            #[cfg(target_os = "android")]
            overlay: Some(crate::android_overlay::Overlay::new({
                use winit::platform::android::ActiveEventLoopExtAndroid;
                event_loop.android_app().clone()
            })),
            last_ime_commit: None,
            handle: self.handle.clone(),
            viewport: Arc::clone(&self.viewport),
            anim_origin: Instant::now(),
            close_requested: false,
            resize: None,
            last_title_press: None,
            snap_drag: false,
            #[cfg(feature = "a11y-os")]
            adapter,
        });
        if let Some(state) = self.state.as_mut() {
            redraw(state);
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        // Drop the softbuffer surface, then the window. On Android the
        // ANativeWindow is invalid after Suspended; presenting into it crashes.
        self.release_surface();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self
            .state
            .as_ref()
            .is_some_and(|state| state.close_requested)
        {
            event_loop.exit();
            return;
        }
        // The commit echo, when the OS sends one, is already in this burst.
        // A key after the queue goes idle is the user's next keystroke.
        if let Some(state) = self.state.as_mut() {
            crate::expire_ime_echo(&mut state.last_ime_commit);
            sync_ime(state);
        }
        let (running, next_blink) = self
            .state
            .as_ref()
            .and_then(|state| state.tree.lock().ok())
            .map(|tree| (tree.animations_running(), tree.next_caret_toggle_ms()))
            .unwrap_or((false, None));
        if running {
            if let Some(state) = self.state.as_mut() {
                state.window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(16),
            ));
        } else if let Some(ms) = next_blink {
            // Only the caret has to change; wake at the next on/off edge.
            if let Some(state) = self.state.as_mut() {
                state.window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(ms.max(1.0).round() as u64),
            ));
        } else {
            let field_focused = self.state.as_ref().is_some_and(|state| {
                #[cfg(target_os = "android")]
                {
                    state.overlay.as_ref().is_some_and(|overlay| overlay.ready)
                        && state.tree.lock().ok().is_some_and(|tree| {
                            tree.focus
                                .and_then(|id| tree.get(id))
                                .is_some_and(|node| node.kind == tekto_tree::NodeKind::TextField)
                        })
                }
                #[cfg(not(target_os = "android"))]
                {
                    let _ = state;
                    false
                }
            });
            if field_focused {
                // Poll the native EditText. It has no callback into this thread.
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(16),
                ));
            } else {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: AppEvent) {
        if let Some(handle) = self.handle.as_ref() {
            handle.apply_pending_js();
        }
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
            WindowEvent::Focused(false) => {
                if let Some(state) = self.state.as_mut() {
                    deliver_pointer(state, PointerOp::End { commit: false });
                    state.pointer = PointerSession::default();
                }
            }
            WindowEvent::CloseRequested => event_loop.exit(), // run_windowed then clear_endpoint()
            WindowEvent::RedrawRequested => {
                if let Some(state) = self.state.as_mut() {
                    redraw(state);
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(state) = self.state.as_mut() {
                    // The redraw path re-reads the viewport and lays out once.
                    // Laying out here as well doubled the work on every step
                    // of a live resize and made the window look frozen.
                    state.window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(state) = self.state.as_mut() {
                    let scale = state.window.scale_factor();
                    let (x, y) = logical_from_physical(position.x, position.y, scale);
                    if state.resize.is_some() {
                        if let Ok(origin) = state.window.outer_position() {
                            resize_follow(
                                state,
                                (origin.x as f64 + position.x, origin.y as f64 + position.y),
                            );
                        }
                        return;
                    }
                    let op = state.pointer.on_cursor_moved(x, y);
                    deliver_pointer(state, op);
                    if !state.window.is_decorated() {
                        let size = state.window.inner_size();
                        let width = size.width as f32 / scale as f32;
                        let height = size.height as f32 / scale as f32;
                        let direction = edge_direction((x, y), width, height);
                        state.window.set_cursor(resize_cursor(direction));
                    }
                }
            }
            WindowEvent::MouseInput {
                state: button_state,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(state) = self.state.as_mut() {
                    let pressed = button_state == ElementState::Pressed;
                    if pressed {
                        let scale = state.window.scale_factor();
                        let cursor = state
                            .pointer
                            .cursor
                            .filter(|_| !state.pointer.swallow_mouse);
                        if let (Some(cursor), false) = (cursor, state.window.is_decorated()) {
                            let size = state.window.inner_size();
                            let width = size.width as f32 / scale as f32;
                            let height = size.height as f32 / scale as f32;
                            if let Some(direction) = edge_direction(cursor, width, height) {
                                if let Ok(origin) = state.window.outer_position() {
                                    let screen = (
                                        origin.x as f64 + cursor.0 as f64 * scale,
                                        origin.y as f64 + cursor.1 as f64 * scale,
                                    );
                                    begin_resize(state, direction, screen);
                                    capture_mouse(&state.window);
                                }
                                return;
                            }
                        }
                        let drag = cursor.is_some_and(|(x, y)| {
                            state
                                .tree
                                .lock()
                                .ok()
                                .is_some_and(|tree| tree.drag_region_at(x, y))
                        });
                        if drag {
                            if let Some((x, y)) = cursor {
                                let now = Instant::now();
                                let double = state.last_title_press.is_some_and(|(then, at)| {
                                    now.duration_since(then) < Duration::from_millis(400)
                                        && (x - at.0).abs() <= 6.0
                                        && (y - at.1).abs() <= 6.0
                                });
                                if double {
                                    state.last_title_press = None;
                                    let maximized = state.window.is_maximized();
                                    state.window.set_maximized(!maximized);
                                    return;
                                }
                                state.last_title_press = Some((now, (x, y)));
                            }
                            state.snap_drag = true;
                            let _ = state.window.drag_window();
                            return;
                        }
                    } else {
                        if let Some(session) = state.resize.take() {
                            if let Some(rect) = session.pending {
                                set_window_rect(&state.window, rect.0, rect.1, rect.2, rect.3);
                            }
                            release_mouse();
                            return;
                        }
                        if state.snap_drag {
                            state.snap_drag = false;
                            snap_window(state);
                            return;
                        }
                    }
                    let op = state.pointer.on_left_button(pressed);
                    deliver_pointer(state, op);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                if let Some(state) = self.state.as_mut() {
                    if let Some((x, y)) = state
                        .pointer
                        .cursor
                        .filter(|_| !state.pointer.swallow_mouse)
                    {
                        let changed = if let Some(handle) = &state.handle {
                            handle
                                .route_action(|tree| (tree.context_menu_at(x, y).is_some(), None))
                                .unwrap_or(false)
                        } else {
                            state.tree.lock().ok().is_some_and(|mut tree| {
                                let had_overlay = tree.overlay().is_some();
                                tree.context_menu_at(x, y).is_some() || had_overlay
                            })
                        };
                        if changed {
                            state.window.request_redraw();
                        }
                    }
                }
            }
            WindowEvent::Touch(touch) => {
                if let Some(state) = self.state.as_mut() {
                    let scale = state.window.scale_factor();
                    let (x, y) = logical_from_physical(touch.location.x, touch.location.y, scale);
                    let phase = match touch.phase {
                        TouchPhase::Started => FingerPhase::Started,
                        TouchPhase::Moved => FingerPhase::Moved,
                        TouchPhase::Ended => FingerPhase::Ended,
                        TouchPhase::Cancelled => FingerPhase::Cancelled,
                    };
                    let op = state.pointer.on_touch(touch.id, phase, x, y);
                    deliver_pointer(state, op);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(state) = self.state.as_mut() {
                    if let Some((x, y)) = state.pointer.cursor {
                        let scale = state.window.scale_factor();
                        let dy = wheel_delta_logical(delta, scale);
                        if dy != 0.0 {
                            apply_wheel(state, x, y, dy);
                        }
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                if let Some(state) = self.state.as_mut() {
                    state.modifiers = modifiers;
                }
            }
            // Plain keys, plus IME preedit / commit when a text field is focused.
            // The candidate window is only positioned (`set_ime_cursor_area`);
            // drawing that popup is the OS input method's job.
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(state) = self.state.as_mut() {
                    // The Android overlay owns the soft keyboard while a text
                    // field is focused. Key events would insert the same
                    // character a second time. Keys still work when the
                    // overlay class is loaded but no text field is focused.
                    #[cfg(target_os = "android")]
                    {
                        let class_ready = state.overlay.as_ref().is_some_and(|o| o.ready);
                        let text_field_focused = state.tree.lock().ok().is_some_and(|tree| {
                            tree.focus
                                .and_then(|id| tree.get(id))
                                .is_some_and(|node| node.kind == tekto_tree::NodeKind::TextField)
                        });
                        if crate::android_field::overlay_suppresses_keys(
                            class_ready,
                            text_field_focused,
                        ) {
                            sync_ime(state);
                            return;
                        }
                    }
                    apply_field_key(state, &event);
                    sync_ime(state);
                }
            }
            WindowEvent::Ime(ime) => {
                if let Some(state) = self.state.as_mut() {
                    apply_ime(state, ime);
                    sync_ime(state);
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

/// CPU by default. Desktop `gpu` tries Vello first. Android stays on CPU
/// unless [`crate::gpu_policy::wants_gpu_paint`] is true.
enum Painter {
    Cpu(Box<CpuRenderer>),
    #[cfg(feature = "gpu")]
    Gpu(Box<tekto_render::GpuRenderer>),
}

impl Painter {
    fn new(window: &Window, width: u32, height: u32) -> Self {
        #[cfg(not(feature = "gpu"))]
        let _ = window;
        if crate::gpu_policy::wants_gpu_paint() {
            #[cfg(feature = "gpu")]
            {
                use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
                let direct = match (window.window_handle(), window.display_handle()) {
                    (Ok(window_handle), Ok(display_handle)) => {
                        tekto_render::GpuRenderer::new_for_window(
                            width,
                            height,
                            window_handle.as_raw(),
                            display_handle.as_raw(),
                        )
                    }
                    _ => tekto_render::GpuRenderer::new(width, height),
                };
                match direct {
                    Ok(renderer) => {
                        eprintln!(
                            "tekto: painting with wgpu / Vello ({})",
                            renderer.adapter_name()
                        );
                        return Painter::Gpu(Box::new(renderer));
                    }
                    Err(err) => {
                        eprintln!("tekto: GPU renderer unavailable ({err}); using CPU");
                    }
                }
            }
        } else {
            #[cfg(all(feature = "gpu", target_os = "android"))]
            note_android_cpu_default();
        }
        Self::cpu(width, height)
    }

    fn cpu(width: u32, height: u32) -> Self {
        Painter::Cpu(Box::new(
            CpuRenderer::new(width, height).expect("cpu renderer"),
        ))
    }

    fn scale(&self) -> f32 {
        match self {
            Painter::Cpu(renderer) => renderer.scale(),
            #[cfg(feature = "gpu")]
            Painter::Gpu(renderer) => renderer.scale(),
        }
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), tekto_render::RenderError> {
        match self {
            Painter::Cpu(renderer) => renderer.resize(width, height),
            #[cfg(feature = "gpu")]
            Painter::Gpu(renderer) => {
                renderer.resize(width, height)?;
                renderer.configure_surface(width, height);
                Ok(())
            }
        }
    }

    fn set_scale(&mut self, scale: f32) {
        match self {
            Painter::Cpu(renderer) => renderer.set_scale(scale),
            #[cfg(feature = "gpu")]
            Painter::Gpu(renderer) => renderer.set_scale(scale),
        }
    }

    fn paint(&mut self, tree: &Tree, clear: tekto_tree::Color) {
        match self {
            Painter::Cpu(renderer) => paint_tree(renderer.as_mut(), tree, clear),
            #[cfg(feature = "gpu")]
            Painter::Gpu(renderer) => paint_tree(renderer.as_mut(), tree, clear),
        }
    }

    fn rgba8(&self) -> Result<Vec<u8>, tekto_render::RenderError> {
        match self {
            Painter::Cpu(renderer) => renderer.rgba8(),
            #[cfg(feature = "gpu")]
            Painter::Gpu(renderer) => renderer.rgba8(),
        }
    }

    /// Present straight to the window surface when the GPU path has one.
    fn present_window(&mut self) -> Option<Result<bool, tekto_render::RenderError>> {
        match self {
            Painter::Cpu(_) => None,
            #[cfg(feature = "gpu")]
            Painter::Gpu(renderer) => Some(renderer.present_window()),
        }
    }
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

#[cfg(target_os = "android")]
fn window_insets(state: &mut State, scale: f32) -> WindowInsets {
    let Some(overlay) = state.overlay.as_mut() else {
        return WindowInsets::default();
    };
    let px = crate::android_overlay::read_insets(overlay);
    crate::android_insets::insets_from_physical(px, scale)
}

#[cfg(not(target_os = "android"))]
fn window_insets(_state: &State, _scale: f32) -> WindowInsets {
    WindowInsets::default()
}

fn relayout_window(state: &mut State) {
    let (lw, lh, scale, pw, ph) = window_logical_and_physical(&state.window);
    let _ = state.renderer.resize(pw, ph);
    state.renderer.set_scale(scale);
    let next = Viewport {
        width: lw.round().max(1.0) as u32,
        height: lh.round().max(1.0) as u32,
        scale,
    };
    let mut viewport_changed = false;
    if let Ok(mut vp) = state.viewport.lock() {
        viewport_changed = vp.width != next.width
            || vp.height != next.height
            || (vp.scale - next.scale).abs() > f32::EPSILON;
        *vp = next;
    }
    // Android always lays out, because insets can change every frame.
    #[cfg(target_os = "android")]
    let _ = viewport_changed;
    #[cfg(target_os = "android")]
    let insets = window_insets(state, scale);
    #[cfg(not(target_os = "android"))]
    let insets = window_insets(&*state, scale);
    if let Ok(mut tree) = state.tree.lock() {
        // A colour or opacity tween only repaints. Skipping layout keeps
        // Taffy and Parley off the frame path for those animations.
        #[cfg(target_os = "android")]
        let needs_layout = true;
        #[cfg(not(target_os = "android"))]
        let needs_layout = viewport_changed || tree.layout_dirty();
        if needs_layout {
            layout_with_insets(&mut tree, lw, lh, insets);
        }
        let now_ms = state.anim_origin.elapsed().as_secs_f64() * 1000.0;
        tree.advance(now_ms);
    }
}

fn redraw(state: &mut State) {
    // Suspend clears the epoch. Do not lock or present a retired buffer.
    if !crate::lifecycle::may_present(state.epoch) {
        return;
    }
    let frame_start = Instant::now();
    // One window rect per painted frame keeps geometry and content in step.
    if let Some(session) = state.resize {
        if let Some(rect) = session.pending {
            set_window_rect(&state.window, rect.0, rect.1, rect.2, rect.3);
            if let Some(session) = state.resize.as_mut() {
                session.last = rect;
                session.pending = None;
            }
        }
    }
    relayout_window(state);
    let layout_done = Instant::now();
    if !crate::lifecycle::may_present(state.epoch) {
        return;
    }
    let (_lw, _lh, _scale, pw, ph) = window_logical_and_physical(&state.window);
    if let Ok(tree) = state.tree.lock() {
        state.renderer.paint(&tree, state.clear);
        publish_accesskit(
            &tree,
            #[cfg(feature = "a11y-os")]
            &mut state.adapter,
        );
    }
    let paint_done = Instant::now();
    match state.renderer.present_window() {
        Some(Ok(true)) => {
            if frame_log_enabled() {
                eprintln!(
                    "tekto frame {:.2} ms (layout {:.2}, paint {:.2}, gpu present {:.2})",
                    frame_start.elapsed().as_secs_f64() * 1000.0,
                    (layout_done - frame_start).as_secs_f64() * 1000.0,
                    (paint_done - layout_done).as_secs_f64() * 1000.0,
                    (Instant::now() - paint_done).as_secs_f64() * 1000.0
                );
            }
            sync_ime(state);
            return;
        }
        Some(Ok(false)) | None => {}
        Some(Err(err)) => {
            eprintln!("tekto: GPU present failed ({err}); using softbuffer");
        }
    }
    let rgba = match state.renderer.rgba8() {
        Ok(pixels) => pixels,
        Err(err) => {
            // A destroyed window must not be locked. Other GPU failures
            // paint once more on the CPU path, then present only if the
            // epoch is still the live surface.
            if !crate::lifecycle::may_present(state.epoch)
                || tekto_render::classify_present_failure(&err.to_string())
                    == tekto_render::PresentFailure::SurfaceLost
            {
                eprintln!("tekto: surface lost ({err}); skipping present");
                return;
            }
            eprintln!("tekto: GPU frame failed ({err}); using CPU");
            let scale = state.renderer.scale();
            state.renderer = Painter::cpu(pw, ph);
            state.renderer.set_scale(scale);
            if let Ok(tree) = state.tree.lock() {
                state.renderer.paint(&tree, state.clear);
            }
            match state.renderer.rgba8() {
                Ok(pixels) => pixels,
                Err(err) => {
                    eprintln!("tekto: CPU frame failed ({err}); skipping present");
                    return;
                }
            }
        }
    };
    if !crate::lifecycle::may_present(state.epoch) {
        return;
    }
    let Some(width) = NonZeroU32::new(pw) else {
        return;
    };
    let Some(height) = NonZeroU32::new(ph) else {
        return;
    };
    let resize_start = Instant::now();
    let _ = state.surface.resize(width, height);
    let resize_done = Instant::now();
    match state.surface.buffer_mut() {
        Ok(mut buf) => {
            let pixels = rgba_to_u32_softbuffer(&rgba);
            let convert_done = Instant::now();
            for (dst, src) in buf.iter_mut().zip(pixels.iter()) {
                *dst = *src;
            }
            let copy_done = Instant::now();
            // present() fails if the Android window was destroyed after
            // the epoch check. Ignore that error; do not panic.
            let _ = buf.present();
            let present_done = Instant::now();
            if frame_log_enabled() {
                eprintln!(
                    "tekto present: surface {:.2} ms, convert {:.2} ms, blit {:.2} ms, present {:.2} ms",
                    (resize_done - resize_start).as_secs_f64() * 1000.0,
                    (convert_done - resize_done).as_secs_f64() * 1000.0,
                    (copy_done - convert_done).as_secs_f64() * 1000.0,
                    (present_done - copy_done).as_secs_f64() * 1000.0
                );
            }
        }
        Err(err) => {
            eprintln!("tekto: window surface unavailable ({err}); not presenting");
        }
    }
    if frame_log_enabled() {
        eprintln!(
            "tekto frame {:.2} ms (layout {:.2}, paint {:.2}, present {:.2})",
            frame_start.elapsed().as_secs_f64() * 1000.0,
            (layout_done - frame_start).as_secs_f64() * 1000.0,
            (paint_done - layout_done).as_secs_f64() * 1000.0,
            (Instant::now() - paint_done).as_secs_f64() * 1000.0
        );
    }
    sync_ime(state);
}

/// `TEKTO_FRAME_LOG=1` prints one line per painted frame (debug builds).
fn frame_log_enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| cfg!(debug_assertions) && std::env::var_os("TEKTO_FRAME_LOG").is_some())
}

#[cfg(all(feature = "gpu", target_os = "android"))]
fn note_android_cpu_default() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static NOTED: AtomicBool = AtomicBool::new(false);
    if NOTED.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!(
        "tekto: Android stays on the CPU renderer. Build with --features gpu and set TEKTO_ANDROID_GPU=1 to try wgpu / Vello. Drivers were not proven here (no device GPU, no emulator). A lost ANativeWindow skips present."
    );
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

impl App {
    /// Retire the current buffer. Surface first, then the winit window.
    fn release_surface(&mut self) {
        #[cfg(target_os = "android")]
        if let Some(state) = self.state.as_mut() {
            if let Some(overlay) = state.overlay.as_mut() {
                crate::android_overlay::hide(overlay);
            }
        }
        let Some(state) = self.state.take() else {
            crate::lifecycle::on_suspended();
            return;
        };
        let State {
            surface,
            window,
            epoch,
            ..
        } = state;
        // Retire the epoch before unlocking the native window so a redraw
        // cannot present into the buffer we are about to free.
        let _ = crate::lifecycle::end_surface(epoch);
        drop(surface);
        drop(window);
    }
}

fn wheel_delta_logical(delta: MouseScrollDelta, scale: f64) -> f32 {
    match delta {
        // Positive line delta is a wheel notch away from the user: reveal content above.
        MouseScrollDelta::LineDelta(_, lines) => -lines * SCROLL_WHEEL_LINE,
        MouseScrollDelta::PixelDelta(pos) => {
            let scale = if scale.abs() < 0.01 { 1.0 } else { scale };
            -(pos.y as f32) / scale as f32
        }
    }
}

fn pointer_ms(state: &State) -> f64 {
    state.anim_origin.elapsed().as_secs_f64() * 1000.0
}

fn deliver_pointer(state: &mut State, op: PointerOp) {
    if matches!(op, PointerOp::Ignore) {
        return;
    }
    let now = pointer_ms(state);
    let changed = if let Some(handle) = &state.handle {
        crate::pointer_route::deliver_pointer(handle, &mut state.pointer, op, now).unwrap_or(true)
    } else if let Ok(mut tree) = state.tree.lock() {
        let had_overlay = tree.overlay().is_some();
        let effect = apply_pointer_op(&mut state.pointer, &mut tree, op, now);
        let clicked = effect
            .click_at
            .and_then(|(x, y)| {
                crate::pointer_route::click_pointer(&mut tree, x, y)
                    .ok()
                    .flatten()
            })
            .is_some();
        effect.redraw || clicked || (had_overlay && tree.overlay().is_none())
    } else {
        false
    };
    let control = {
        state
            .tree
            .lock()
            .ok()
            .and_then(|mut tree| tree.take_window_control())
    };
    if let Some(control) = control {
        apply_window_control(state, control);
    }
    if changed {
        state.window.request_redraw();
    }
}

fn apply_window_control(state: &mut State, control: tekto_tree::WindowControl) {
    match control {
        tekto_tree::WindowControl::Minimize => state.window.set_minimized(true),
        tekto_tree::WindowControl::Maximize => {
            let maximized = state.window.is_maximized();
            state.window.set_maximized(!maximized);
        }
        tekto_tree::WindowControl::Close => state.close_requested = true,
        tekto_tree::WindowControl::None => {}
    }
}

fn edge_direction(cursor: (f32, f32), width: f32, height: f32) -> Option<ResizeDirection> {
    let left = cursor.0 <= RESIZE_EDGE;
    let right = cursor.0 >= width - RESIZE_EDGE;
    let top = cursor.1 <= RESIZE_EDGE;
    let bottom = cursor.1 >= height - RESIZE_EDGE;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, _) => Some(ResizeDirection::West),
        (_, true, _, _) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

fn resize_cursor(direction: Option<ResizeDirection>) -> CursorIcon {
    match direction {
        Some(ResizeDirection::West | ResizeDirection::East) => CursorIcon::EwResize,
        Some(ResizeDirection::North | ResizeDirection::South) => CursorIcon::NsResize,
        Some(ResizeDirection::NorthEast | ResizeDirection::SouthWest) => CursorIcon::NeswResize,
        Some(ResizeDirection::NorthWest | ResizeDirection::SouthEast) => CursorIcon::NwseResize,
        None => CursorIcon::Default,
    }
}

/// Keep receiving cursor moves after the pointer leaves the window.
#[cfg(target_os = "windows")]
fn capture_mouse(window: &Window) {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    unsafe extern "system" {
        fn SetCapture(hwnd: isize) -> isize;
    }
    if let Ok(handle) = window.window_handle() {
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            // Safety: the HWND belongs to this window; capture is released on mouse up.
            unsafe {
                SetCapture(handle.hwnd.get());
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn capture_mouse(_window: &Window) {}

#[cfg(target_os = "windows")]
fn release_mouse() {
    unsafe extern "system" {
        fn ReleaseCapture() -> i32;
    }
    // Safety: releases this thread's capture, which only begin_resize sets.
    unsafe {
        ReleaseCapture();
    }
}

#[cfg(not(target_os = "windows"))]
fn release_mouse() {}

/// Physical screen position of the pointer, independent of window movement.
#[cfg(target_os = "windows")]
fn screen_cursor() -> Option<(f64, f64)> {
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    unsafe extern "system" {
        fn GetCursorPos(point: *mut Point) -> i32;
    }
    let mut point = Point { x: 0, y: 0 };
    // Safety: `point` is a valid POINT for the duration of the call.
    if unsafe { GetCursorPos(&mut point) } != 0 {
        Some((point.x as f64, point.y as f64))
    } else {
        None
    }
}

#[cfg(not(target_os = "windows"))]
fn screen_cursor() -> Option<(f64, f64)> {
    None
}

/// Move and size the window in one step where the platform allows it.
#[cfg(target_os = "windows")]
fn set_window_rect(window: &Window, x: i32, y: i32, width: u32, height: u32) {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    unsafe extern "system" {
        fn SetWindowPos(
            hwnd: isize,
            after: isize,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
    }
    if let Ok(handle) = window.window_handle() {
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            // Safety: the HWND belongs to this window. One call keeps position
            // and size in step, so a resize frame does not render at a stale rect.
            unsafe {
                SetWindowPos(
                    handle.hwnd.get(),
                    0,
                    x,
                    y,
                    width as i32,
                    height as i32,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn set_window_rect(window: &Window, x: i32, y: i32, width: u32, height: u32) {
    window.set_outer_position(PhysicalPosition::new(x, y));
    let _ = window.request_inner_size(PhysicalSize::new(width, height));
}

fn begin_resize(state: &mut State, direction: ResizeDirection, screen: (f64, f64)) {
    let screen = screen_cursor().unwrap_or(screen);
    let Ok(origin) = state.window.outer_position() else {
        return;
    };
    let size = state.window.inner_size();
    state.resize = Some(ResizeSession {
        direction,
        start: screen,
        origin: (origin.x, origin.y),
        size: (size.width, size.height),
        scale: state.window.scale_factor(),
        last: (origin.x, origin.y, size.width, size.height),
        pending: None,
    });
}

fn resize_follow(state: &mut State, screen: (f64, f64)) {
    // Global cursor position does not lag when the window itself moves.
    let screen = screen_cursor().unwrap_or(screen);
    let Some(session) = state.resize else {
        return;
    };
    let dx = screen.0 - session.start.0;
    let dy = screen.1 - session.start.1;
    let mut x = session.origin.0 as f64;
    let mut y = session.origin.1 as f64;
    let mut width = session.size.0 as f64;
    let mut height = session.size.1 as f64;
    let west = matches!(
        session.direction,
        ResizeDirection::West | ResizeDirection::NorthWest | ResizeDirection::SouthWest
    );
    let east = matches!(
        session.direction,
        ResizeDirection::East | ResizeDirection::NorthEast | ResizeDirection::SouthEast
    );
    let north = matches!(
        session.direction,
        ResizeDirection::North | ResizeDirection::NorthWest | ResizeDirection::NorthEast
    );
    let south = matches!(
        session.direction,
        ResizeDirection::South | ResizeDirection::SouthWest | ResizeDirection::SouthEast
    );
    if east {
        width += dx;
    }
    if west {
        x += dx;
        width -= dx;
    }
    if south {
        height += dy;
    }
    if north {
        y += dy;
        height -= dy;
    }
    let min_width = MIN_RESIZE_WIDTH as f64 * session.scale;
    let min_height = MIN_RESIZE_HEIGHT as f64 * session.scale;
    if width < min_width {
        if west {
            x -= min_width - width;
        }
        width = min_width;
    }
    if height < min_height {
        if north {
            y -= min_height - height;
        }
        height = min_height;
    }
    let rect = (
        x.round() as i32,
        y.round() as i32,
        width.round() as u32,
        height.round() as u32,
    );
    if rect == session.last || session.pending == Some(rect) {
        return;
    }
    // Hold the newest rect for the next frame instead of resizing the window
    // more often than it can paint; that backlog is what felt laggy.
    if let Some(session) = state.resize.as_mut() {
        session.pending = Some(rect);
    }
    state.window.request_redraw();
}

/// Snap a dragged window to halves, quarters, or maximised at a monitor edge.
fn snap_window(state: &mut State) {
    if state.window.is_maximized() {
        return;
    }
    let Some(cursor) = state.pointer.cursor else {
        return;
    };
    let Ok(origin) = state.window.outer_position() else {
        return;
    };
    let Some(monitor) = state.window.current_monitor() else {
        return;
    };
    let scale = state.window.scale_factor();
    // The pointer session stores window-relative coordinates; snap tests the screen.
    let cx = origin.x as f64 + cursor.0 as f64 * scale;
    let cy = origin.y as f64 + cursor.1 as f64 * scale;
    let pos = monitor.position();
    let size = monitor.size();
    let left = pos.x as f64;
    let top = pos.y as f64;
    let width = size.width as f64;
    let height = size.height as f64;
    let edge = SNAP_EDGE as f64 * scale;
    let horizontal = if cx <= left + edge {
        Some(0)
    } else if cx >= left + width - edge {
        Some(1)
    } else {
        None
    };
    let vertical = if cy <= top + edge {
        Some(0)
    } else if cy >= top + height - edge {
        Some(1)
    } else {
        None
    };
    let half_width = width / 2.0;
    let half_height = height / 2.0;
    match (horizontal, vertical) {
        (Some(hx), Some(vy)) => {
            let x = if hx == 0 { left } else { left + half_width };
            let y = if vy == 0 { top } else { top + half_height };
            state
                .window
                .set_outer_position(PhysicalPosition::new(x as i32, y as i32));
            let _ = state
                .window
                .request_inner_size(PhysicalSize::new(half_width as u32, half_height as u32));
        }
        (None, Some(0)) => state.window.set_maximized(true),
        (Some(hx), None) => {
            let x = if hx == 0 { left } else { left + half_width };
            state
                .window
                .set_outer_position(PhysicalPosition::new(x as i32, top as i32));
            let _ = state
                .window
                .request_inner_size(PhysicalSize::new(half_width as u32, height as u32));
        }
        _ => {}
    }
}

fn apply_wheel(state: &mut State, x: f32, y: f32, delta_y: f32) {
    let changed = state
        .tree
        .lock()
        .ok()
        .and_then(|mut tree| tree.scroll_at(x, y, delta_y))
        .is_some();
    if changed {
        state.window.request_redraw();
    }
}

/// Turn the platform IME on only while a text field is focused, and tell it
/// where the caret is so a candidate window can sit nearby.
///
/// Linux and Windows use winit `set_ime_allowed`. Android uses a native
/// `EditText` (`android_overlay`); winit there does not deliver IME text.
/// The candidate popup itself is drawn by the OS.
fn sync_ime(state: &mut State) {
    #[cfg(target_os = "android")]
    {
        if let Some(overlay) = state.overlay.as_mut() {
            if crate::android_overlay::sync_overlay(overlay, &state.window, &state.tree) {
                return;
            }
        }
    }
    let focused_field = state.tree.lock().ok().and_then(|tree| {
        let id = tree.focus?;
        let node = tree.get(id)?;
        if node.kind != tekto_tree::NodeKind::TextField {
            return None;
        }
        let inset = node.layout.padding;
        let x = node.bounds.x + inset + node.caret_x;
        let y = node.bounds.y + inset;
        let h = (node.bounds.height - 2.0 * inset).max(1.0);
        Some((x, y, h))
    });
    let allow = focused_field.is_some();
    if state.ime_allowed != allow {
        state.window.set_ime_allowed(allow);
        if allow {
            state.window.set_ime_purpose(ImePurpose::Normal);
        }
        state.ime_allowed = allow;
        if !allow {
            state.last_ime_commit = None;
        }
    }
    if let Some((x, y, h)) = focused_field {
        state.window.set_ime_cursor_area(
            LogicalPosition::new(x as f64, y as f64),
            LogicalSize::new(1.0, h as f64),
        );
    }
}

fn apply_ime(state: &mut State, ime: Ime) {
    let event = match ime {
        Ime::Enabled => ImeEvent::Enabled,
        Ime::Disabled => ImeEvent::Disabled,
        Ime::Preedit(text, cursor) => {
            state.last_ime_commit = None;
            ImeEvent::Preedit { text, cursor }
        }
        Ime::Commit(text) => {
            if !text.is_empty() {
                state.last_ime_commit = Some(text.clone());
            }
            ImeEvent::Commit(text)
        }
    };
    let changed = if let Some(handle) = &state.handle {
        handle
            .route_action(|tree| (tree.apply_ime(event).unwrap_or(false), None))
            .unwrap_or(true)
    } else {
        state
            .tree
            .lock()
            .ok()
            .and_then(|mut tree| tree.apply_ime(event).ok())
            .unwrap_or(false)
    };
    if changed {
        state.window.request_redraw();
    }
}

fn apply_field_key(state: &mut State, event: &KeyEvent) {
    let changed = if let Some(handle) = &state.handle {
        crate::key_route::deliver_key(
            handle,
            &event.logical_key,
            event.text.as_deref(),
            event.state,
            event.repeat,
            state.modifiers.state(),
            &mut state.last_ime_commit,
        )
        .unwrap_or(true)
    } else if let Ok(mut tree) = state.tree.lock() {
        if let Ok(Some(changed)) = crate::key_route::route_clipboard(
            &mut tree,
            &tekto_native::native(),
            &event.logical_key,
            event.state,
            state.modifiers.state(),
        ) {
            if changed {
                state.window.request_redraw();
            }
            return;
        }
        let (changed, click) = crate::key_route::route_key(
            &mut tree,
            &event.logical_key,
            event.text.as_deref(),
            event.state,
            event.repeat,
            state.modifiers.state(),
            &mut state.last_ime_commit,
        );
        if let Some(id) = click {
            let _ = tree.click(id);
        }
        changed
    } else {
        false
    };
    let control = {
        state
            .tree
            .lock()
            .ok()
            .and_then(|mut tree| tree.take_window_control())
    };
    if let Some(control) = control {
        apply_window_control(state, control);
    }
    if changed {
        state.window.request_redraw();
    }
}

#[cfg(target_os = "android")]
pub fn run_android(
    runtime: AppRuntime,
    opts: WindowOptions,
    android_app: winit::platform::android::activity::AndroidApp,
) -> Result<(), PlatformError> {
    run_android_with_session(runtime, opts, android_app, None)
}

/// Same as [`run_android`], but attaches an existing dev-server session so
/// the NativeActivity can accept `POST /reload` JS bundles over adb forward.
#[cfg(target_os = "android")]
pub fn run_android_with_session(
    runtime: AppRuntime,
    opts: WindowOptions,
    android_app: winit::platform::android::activity::AndroidApp,
    session: Option<crate::DevSession>,
) -> Result<(), PlatformError> {
    run_event_loop(runtime, opts, Some(android_app), session, None)
}

pub fn run_windowed(runtime: AppRuntime, opts: WindowOptions) -> Result<(), PlatformError> {
    run_windowed_with_session(runtime, opts, None)
}

/// Start a window using an already-running dev session (so `tekto dev --js`
/// can attach a JS reload hook before the event loop starts).
#[cfg(target_os = "android")]
pub fn run_windowed_with_session(
    runtime: AppRuntime,
    opts: WindowOptions,
    session: Option<crate::DevSession>,
) -> Result<(), PlatformError> {
    let _ = (runtime, opts, session);
    Err(PlatformError::msg(
        "on Android, call tekto::run_android(...) from android_main",
    ))
}

#[cfg(not(target_os = "android"))]
pub fn run_windowed_with_session(
    runtime: AppRuntime,
    opts: WindowOptions,
    session: Option<crate::DevSession>,
) -> Result<(), PlatformError> {
    run_event_loop(runtime, opts, None, session, None)
}

/// Start a window with a live handle without requiring an HTTP server.
#[cfg(not(target_os = "android"))]
pub fn run_windowed_with_handle(
    runtime: AppRuntime,
    opts: WindowOptions,
    handle: Arc<SharedTreeHandle>,
    viewport: Arc<Mutex<Viewport>>,
) -> Result<(), PlatformError> {
    run_event_loop(runtime, opts, None, None, Some((handle, viewport)))
}

#[cfg(target_os = "android")]
type AndroidAppArg = winit::platform::android::activity::AndroidApp;
#[cfg(not(target_os = "android"))]
type AndroidAppArg = ();

fn run_event_loop(
    runtime: AppRuntime,
    opts: WindowOptions,
    android_app: Option<AndroidAppArg>,
    existing_session: Option<crate::DevSession>,
    standalone: Option<(Arc<SharedTreeHandle>, Arc<Mutex<Viewport>>)>,
) -> Result<(), PlatformError> {
    let mut prepared = runtime.clone();
    if tekto_tree::reduced_motion_requested() {
        prepared.tree.set_reduced_motion(true);
    }
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
    let (tree, handle, viewport) = if let Some((handle, ep, viewport)) = existing_session {
        let proxy = event_loop.create_proxy();
        if let Ok(mut slot) = handle.on_change.lock() {
            *slot = Some(std::sync::Arc::new(move || {
                let _ = proxy.send_event(AppEvent::TreeChanged);
            }));
        }
        eprintln!("Tekto dev server at {}", ep.base_url());
        eprintln!("Try: tekto tree    or    tekto screenshot    or    tekto click <id>");
        (std::sync::Arc::clone(&handle.tree), Some(handle), viewport)
    } else if let Some((handle, viewport)) = standalone {
        let proxy = event_loop.create_proxy();
        if let Ok(mut slot) = handle.on_change.lock() {
            *slot = Some(Arc::new(move || {
                let _ = proxy.send_event(AppEvent::TreeChanged);
            }));
        }
        (Arc::clone(&handle.tree), Some(handle), viewport)
    } else if opts.dev_server {
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
    let run_result = event_loop.run_app(&mut app);
    // Desktop often exits without `suspended`. Still drop the buffer.
    app.release_surface();
    run_result.map_err(|e| PlatformError::msg(e.to_string()))?;
    tekto_mcp::clear_endpoint();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_and_corners_map_to_resize_directions() {
        let (width, height) = (400.0, 300.0);
        assert_eq!(
            edge_direction((2.0, 150.0), width, height),
            Some(ResizeDirection::West)
        );
        assert_eq!(
            edge_direction((398.0, 150.0), width, height),
            Some(ResizeDirection::East)
        );
        assert_eq!(
            edge_direction((200.0, 2.0), width, height),
            Some(ResizeDirection::North)
        );
        assert_eq!(
            edge_direction((200.0, 298.0), width, height),
            Some(ResizeDirection::South)
        );
        assert_eq!(
            edge_direction((2.0, 2.0), width, height),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            edge_direction((398.0, 2.0), width, height),
            Some(ResizeDirection::NorthEast)
        );
        assert_eq!(
            edge_direction((2.0, 298.0), width, height),
            Some(ResizeDirection::SouthWest)
        );
        assert_eq!(
            edge_direction((398.0, 298.0), width, height),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(edge_direction((200.0, 150.0), width, height), None);
        // A button near the top-right stays outside the corner band in the middle.
        assert_eq!(edge_direction((390.0, 40.0), width, height), None);
    }
}
