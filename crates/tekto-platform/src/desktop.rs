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
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{
    ElementState, Ime, KeyEvent, Modifiers, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{ImePurpose, Window, WindowId};

/// Wakes the window when the dev server patches the tree.
#[derive(Clone, Copy, Debug)]
enum AppEvent {
    TreeChanged,
}

struct State {
    /// Dropped before [`State::window`] so the Android `ANativeWindow` is
    /// released while the winit window handle is still alive.
    surface: Surface<Rc<Window>, Rc<Window>>,
    window: Rc<Window>,
    epoch: crate::lifecycle::SurfaceEpoch,
    renderer: Painter,
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
        let renderer = Painter::new(size.width.max(1), size.height.max(1));
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
        // The commit echo, when the OS sends one, is already in this burst.
        // A key after the queue goes idle is the user's next keystroke.
        if let Some(state) = self.state.as_mut() {
            crate::expire_ime_echo(&mut state.last_ime_commit);
            sync_ime(state);
        }
        let running = self
            .state
            .as_ref()
            .and_then(|state| state.tree.lock().ok())
            .is_some_and(|tree| tree.animations_running());
        if running {
            if let Some(state) = self.state.as_mut() {
                state.window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(16),
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
                    let (x, y) = logical_from_physical(position.x, position.y, scale);
                    let op = state.pointer.on_cursor_moved(x, y);
                    deliver_pointer(state, op);
                }
            }
            WindowEvent::MouseInput {
                state: button_state,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(state) = self.state.as_mut() {
                    let pressed = button_state == ElementState::Pressed;
                    let op = state.pointer.on_left_button(pressed);
                    deliver_pointer(state, op);
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
    fn new(width: u32, height: u32) -> Self {
        if crate::gpu_policy::wants_gpu_paint() {
            #[cfg(feature = "gpu")]
            {
                match tekto_render::GpuRenderer::new(width, height) {
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
            Painter::Gpu(renderer) => renderer.resize(width, height),
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
    if let Ok(mut vp) = state.viewport.lock() {
        *vp = Viewport {
            width: lw.round().max(1.0) as u32,
            height: lh.round().max(1.0) as u32,
            scale,
        };
    }
    #[cfg(target_os = "android")]
    let insets = window_insets(state, scale);
    #[cfg(not(target_os = "android"))]
    let insets = window_insets(&*state, scale);
    if let Ok(mut tree) = state.tree.lock() {
        layout_with_insets(&mut tree, lw, lh, insets);
        let now_ms = state.anim_origin.elapsed().as_secs_f64() * 1000.0;
        tree.advance(now_ms);
    }
}

fn redraw(state: &mut State) {
    // Suspend clears the epoch. Do not lock or present a retired buffer.
    if !crate::lifecycle::may_present(state.epoch) {
        return;
    }
    relayout_window(state);
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
    let _ = state.surface.resize(width, height);
    match state.surface.buffer_mut() {
        Ok(mut buf) => {
            let pixels = rgba_to_u32_softbuffer(&rgba);
            for (dst, src) in buf.iter_mut().zip(pixels.iter()) {
                *dst = *src;
            }
            // present() fails if the Android window was destroyed after
            // the epoch check. Ignore that error; do not panic.
            let _ = buf.present();
        }
        Err(err) => {
            eprintln!("tekto: window surface unavailable ({err}); not presenting");
        }
    }
    sync_ime(state);
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
        let effect = apply_pointer_op(&mut state.pointer, &mut tree, op, now);
        let clicked = effect
            .click_at
            .and_then(|(x, y)| {
                crate::pointer_route::click_pointer(&mut tree, x, y)
                    .ok()
                    .flatten()
            })
            .is_some();
        effect.redraw || clicked
    } else {
        false
    };
    if changed {
        state.window.request_redraw();
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
    let changed = state
        .tree
        .lock()
        .ok()
        .and_then(|mut tree| tree.apply_ime(event).ok())
        .unwrap_or(false);
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
        let (changed, click) = crate::key_route::route_key(
            &mut tree,
            &event.logical_key,
            event.text.as_deref(),
            event.state,
            event.repeat,
            state.modifiers.state(),
            &mut state.last_ime_commit,
        );
        changed || click.is_some_and(|id| tree.click(id).is_ok())
    } else {
        false
    };
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
