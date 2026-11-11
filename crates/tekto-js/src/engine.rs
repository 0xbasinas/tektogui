//! QuickJS-ng engine (rquickjs). Only compiled with `--features quickjs`.

use rquickjs::{
    function::{Func, Opt},
    Context, Ctx, Object, Runtime, Value,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

const EXECUTION_BUDGET: Duration = Duration::from_secs(1);

use crate::{apply_native_host_op, Host, HostOp, JsError, NativeOp};

thread_local! {
    static HOST: RefCell<Option<*mut (dyn Host + 'static)>> = const { RefCell::new(None) };
}

struct HostGuard;

impl Drop for HostGuard {
    fn drop(&mut self) {
        HOST.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Extend the host pointer to `'static` for the QuickJS callbacks.
///
/// Safety: the pointer is only used on this thread until [`HostGuard`]
/// clears it, and those callbacks run synchronously inside a session call.
unsafe fn attach_host(host: &mut dyn Host) -> HostGuard {
    let ptr: *mut (dyn Host + 'static) = unsafe {
        std::mem::transmute::<*mut dyn Host, *mut (dyn Host + 'static)>(host as *mut dyn Host)
    };
    HOST.with(|slot| *slot.borrow_mut() = Some(ptr));
    HostGuard
}

fn with_host<T>(f: impl FnOnce(&mut dyn Host) -> Result<T, JsError>) -> Result<T, JsError> {
    HOST.with(|slot| {
        let ptr = slot
            .borrow()
            .ok_or_else(|| JsError::Message("JS host is not attached".into()))?;
        f(unsafe { &mut *ptr })
    })
}

fn throw<'js, T>(ctx: &Ctx<'js>, err: JsError) -> rquickjs::Result<T> {
    Err(rquickjs::Exception::throw_message(ctx, &err.to_string()))
}

/// A QuickJS session bound to its creating thread. Host access is attached only
/// while a call is running; callers must not hold a tree lock during these calls.
pub struct JsSession {
    context: Context,
    _runtime: Runtime,
    deadline: Rc<Cell<Option<Instant>>>,
    interrupted: Rc<Cell<bool>>,
}

impl JsSession {
    pub fn new() -> Result<Self, JsError> {
        let runtime = Runtime::new().map_err(|e| JsError::Message(e.to_string()))?;
        let deadline = Rc::new(Cell::new(None::<Instant>));
        let interrupted = Rc::new(Cell::new(false));
        let handler_deadline = Rc::clone(&deadline);
        let handler_interrupted = Rc::clone(&interrupted);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let expired = handler_deadline
                .get()
                .is_some_and(|end| Instant::now() >= end);
            if expired {
                handler_interrupted.set(true);
            }
            expired
        })));
        let context = Context::full(&runtime).map_err(|e| JsError::Message(e.to_string()))?;
        context
            .with(|ctx| -> rquickjs::Result<()> {
                let tekto = Object::new(ctx.clone())?;
                tekto.set("create", Func::from(js_create))?;
                tekto.set("setText", Func::from(js_set_text))?;
                tekto.set("setProp", Func::from(js_set_prop))?;
                tekto.set("append", Func::from(js_append))?;
                tekto.set("remove", Func::from(js_remove))?;
                tekto.set("openOverlay", Func::from(js_open_overlay))?;
                tekto.set("closeOverlay", Func::from(js_close_overlay))?;
                tekto.set("bindContextMenu", Func::from(js_bind_context_menu))?;
                tekto.set("bindTabPanel", Func::from(js_bind_tab_panel))?;
                tekto.set("transition", Func::from(js_transition))?;
                tekto.set("applyNativeOp", Func::from(js_apply_native_op))?;
                ctx.globals().set("tekto", tekto)?;
                ctx.eval::<(), _>(include_str!("session.js"))
            })
            .map_err(|e| JsError::Message(format_quickjs_error(&e)))?;
        Ok(Self {
            context,
            _runtime: runtime,
            deadline,
            interrupted,
        })
    }

    /// Evaluate one bundle. Replace the entire session for a reload.
    pub fn eval(&self, source: &str, host: &mut dyn Host) -> Result<(), JsError> {
        self.run(host, |ctx| ctx.eval::<(), _>(source))
    }

    /// Capture keyed signal values as JSON before replacing this session.
    pub fn snapshot_signals(&self, host: &mut dyn Host) -> Result<String, JsError> {
        self.run(host, |ctx| ctx.eval("typeof globalThis.__tektoSnapshotSignals === 'function' ? globalThis.__tektoSnapshotSignals() : '{}'"))
    }

    /// Supply the prior session's JSON state before evaluating the new bundle.
    pub fn restore_signals(&self, json: &str, host: &mut dyn Host) -> Result<(), JsError> {
        self.run(host, |ctx| {
            ctx.globals().set("__tektoReloadJson", json)?;
            ctx.eval::<(), _>("globalThis.__tektoReloadState = JSON.parse(globalThis.__tektoReloadJson); delete globalThis.__tektoReloadJson;")
        })
    }

    /// Return the candidate's restored, reset, and removed key report.
    pub fn signal_report(&self, host: &mut dyn Host) -> Result<String, JsError> {
        self.run(host, |ctx| ctx.eval("typeof globalThis.__tektoSignalReloadReport === 'function' ? globalThis.__tektoSignalReloadReport() : '{}'"))
    }

    /// Deliver one already-validated click. Returns false when no handler exists.
    /// The caller must validate that the id still belongs to the current tree.
    pub fn dispatch_click(&self, id: u64, host: &mut dyn Host) -> Result<bool, JsError> {
        self.run(host, |ctx| {
            let f: rquickjs::Function = ctx.globals().get("__tektoDispatchClick")?;
            f.call((id,))
        })
    }

    /// Deliver a committed field value to a registered input handler.
    pub fn dispatch_input(
        &self,
        id: u64,
        value: &str,
        host: &mut dyn Host,
    ) -> Result<bool, JsError> {
        self.run(host, |ctx| {
            let f: rquickjs::Function = ctx.globals().get("__tektoDispatchInput")?;
            f.call((id, value))
        })
    }

    /// Forget a removed subtree before an id can be reused. The host must pass
    /// every removed id when removal happened outside JavaScript.
    pub fn forget_node(&self, id: u64, host: &mut dyn Host) -> Result<(), JsError> {
        self.run(host, |ctx| {
            let f: rquickjs::Function = ctx.globals().get("__tektoForgetNode")?;
            f.call((id,))
        })
    }

    /// Dispose all registrations while the host is available. Call before
    /// retiring a session; dropping it alone cannot run host-backed cleanup.
    pub fn dispose(&self, host: &mut dyn Host) -> Result<(), JsError> {
        self.run(host, |ctx| {
            let f: rquickjs::Function = ctx.globals().get("__tektoDisposeSession")?;
            f.call(())
        })
    }

    fn run<T>(
        &self,
        host: &mut dyn Host,
        f: impl for<'js> FnOnce(Ctx<'js>) -> rquickjs::Result<T>,
    ) -> Result<T, JsError> {
        let occupied = HOST.with(|slot| slot.borrow().is_some());
        if occupied {
            return Err(JsError::Message("reentrant JS host call".into()));
        }
        let _guard = unsafe { attach_host(host) };
        self.interrupted.set(false);
        self.deadline.set(Some(Instant::now() + EXECUTION_BUDGET));
        let result = self.context.with(|ctx| match f(ctx.clone()) {
            Ok(value) => Ok(value),
            Err(rquickjs::Error::Exception) => {
                let exception = ctx.catch();
                let detail = exception
                    .as_exception()
                    .map(|e| format!("{}", e))
                    .unwrap_or_else(|| format!("{exception:?}"));
                Err(JsError::Message(format!("QuickJS exception: {detail}")))
            }
            Err(err) => Err(JsError::Message(format_quickjs_error(&err))),
        });
        self.deadline.set(None);
        if self.interrupted.replace(false) {
            Err(JsError::Message(
                "QuickJS execution budget exceeded (1 second)".into(),
            ))
        } else {
            result
        }
    }
}

pub fn eval_script(source: &str, host: &mut dyn Host) -> Result<(), JsError> {
    JsSession::new()?.eval(source, host)
}

fn format_quickjs_error(err: &rquickjs::Error) -> String {
    let text = err.to_string();
    let debug = format!("{err:?}");
    if text.contains("Exception generated by QuickJS") && debug != text {
        format!("QuickJS: {text} ({debug})")
    } else if text.is_empty() {
        format!("QuickJS error: {debug}")
    } else {
        format!("QuickJS: {text}")
    }
}

fn js_create(ctx: Ctx<'_>, kind: String) -> rquickjs::Result<u64> {
    match with_host(|host| {
        let ids = host.apply(&[HostOp::Create { kind }])?;
        ids.into_iter()
            .next()
            .ok_or_else(|| JsError::Message("create did not return an id".into()))
    }) {
        Ok(id) => Ok(id),
        Err(err) => throw(&ctx, err),
    }
}

fn js_set_text(ctx: Ctx<'_>, id: u64, text: String) -> rquickjs::Result<()> {
    match with_host(|host| {
        host.apply(&[HostOp::SetText { id, text }])?;
        Ok(())
    }) {
        Ok(()) => Ok(()),
        Err(err) => throw(&ctx, err),
    }
}

fn js_set_prop(ctx: Ctx<'_>, id: u64, key: String, value: String) -> rquickjs::Result<()> {
    match with_host(|host| {
        host.apply(&[HostOp::SetProp { id, key, value }])?;
        Ok(())
    }) {
        Ok(()) => Ok(()),
        Err(err) => throw(&ctx, err),
    }
}

fn js_append(ctx: Ctx<'_>, parent: u64, child: u64) -> rquickjs::Result<()> {
    match with_host(|host| {
        host.apply(&[HostOp::Append { parent, child }])?;
        Ok(())
    }) {
        Ok(()) => Ok(()),
        Err(err) => throw(&ctx, err),
    }
}

fn js_open_overlay(ctx: Ctx<'_>, id: u64, anchor: u64, side: String) -> rquickjs::Result<()> {
    match with_host(|host| {
        host.apply(&[HostOp::OpenOverlay { id, anchor, side }])?;
        Ok(())
    }) {
        Ok(()) => Ok(()),
        Err(err) => throw(&ctx, err),
    }
}

fn js_bind_tab_panel(ctx: Ctx<'_>, tab: u64, panel: u64) -> rquickjs::Result<()> {
    match with_host(|host| {
        host.apply(&[HostOp::BindTabPanel { tab, panel }])?;
        Ok(())
    }) {
        Ok(()) => Ok(()),
        Err(err) => throw(&ctx, err),
    }
}

fn js_bind_context_menu(ctx: Ctx<'_>, target: u64, menu: u64) -> rquickjs::Result<()> {
    match with_host(|host| {
        host.apply(&[HostOp::BindContextMenu { target, menu }])?;
        Ok(())
    }) {
        Ok(()) => Ok(()),
        Err(err) => throw(&ctx, err),
    }
}

fn js_close_overlay(ctx: Ctx<'_>) -> rquickjs::Result<()> {
    match with_host(|host| {
        host.apply(&[HostOp::CloseOverlay])?;
        Ok(())
    }) {
        Ok(()) => Ok(()),
        Err(err) => throw(&ctx, err),
    }
}

/// `tekto.transition(id, key, value, durationMs)` or
/// `tekto.transition(id, { opacity, x, y, width, height, durationMs })`.
///
/// Each call merges onto the node. A later call does not cancel other props.
/// Pass `0` for `durationMs` to snap. The window loop advances time.
fn js_transition<'js>(
    ctx: Ctx<'js>,
    id: u64,
    spec_or_key: Value<'js>,
    value: Opt<Value<'js>>,
    duration_ms: Opt<u32>,
) -> rquickjs::Result<()> {
    let (opacity, x, y, width, height, duration) = if spec_or_key.is_object() {
        let Some(obj) = spec_or_key.as_object() else {
            return throw(
                &ctx,
                JsError::Message("transition spec must be an object".into()),
            );
        };
        (
            read_prop(&ctx, obj, "opacity")?,
            read_prop(&ctx, obj, "x")?,
            read_prop(&ctx, obj, "y")?,
            read_prop(&ctx, obj, "width")?,
            read_prop(&ctx, obj, "height")?,
            read_duration(&ctx, obj)?.or(Some(DEFAULT_JS_DURATION_MS)),
        )
    } else {
        let Some(text) = spec_or_key.as_string() else {
            return throw(
                &ctx,
                JsError::Message("transition key must be a string or a props object".into()),
            );
        };
        let key = match text.to_string() {
            Ok(key) => key,
            Err(err) => {
                return throw(&ctx, JsError::Message(err.to_string()));
            }
        };
        let raw = match value.0 {
            Some(value) => value_to_string(&ctx, &value)?,
            None => {
                return throw(&ctx, JsError::Message("transition needs a value".into()));
            }
        };
        let mut opacity = None;
        let mut x = None;
        let mut y = None;
        let mut width = None;
        let mut height = None;
        match key.as_str() {
            "opacity" => opacity = Some(raw),
            "x" => x = Some(raw),
            "y" => y = Some(raw),
            "width" => width = Some(raw),
            "height" => height = Some(raw),
            other => {
                return throw(
                    &ctx,
                    JsError::Message(format!(
                        "transition key must be opacity, x, y, width, or height, got `{other}`"
                    )),
                );
            }
        }
        let duration = duration_ms.0.or(Some(DEFAULT_JS_DURATION_MS));
        (opacity, x, y, width, height, duration)
    };
    match with_host(|host| {
        host.apply(&[HostOp::Transition {
            id,
            opacity,
            x,
            y,
            width,
            height,
            duration_ms: duration,
        }])?;
        Ok(())
    }) {
        Ok(()) => Ok(()),
        Err(err) => throw(&ctx, err),
    }
}

const DEFAULT_JS_DURATION_MS: u32 = 180;

fn read_prop<'js>(
    ctx: &Ctx<'js>,
    obj: &Object<'js>,
    key: &str,
) -> rquickjs::Result<Option<String>> {
    if !obj.contains_key(key)? {
        return Ok(None);
    }
    let value: Value = obj.get(key)?;
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    Ok(Some(value_to_string(ctx, &value)?))
}

fn read_duration<'js>(ctx: &Ctx<'js>, obj: &Object<'js>) -> rquickjs::Result<Option<u32>> {
    let key = if obj.contains_key("durationMs")? {
        "durationMs"
    } else if obj.contains_key("duration_ms")? {
        "duration_ms"
    } else {
        return Ok(None);
    };
    let value: Value = obj.get(key)?;
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    let Some(n) = value.as_number() else {
        return throw(ctx, JsError::Message(format!("{key} must be a number")));
    };
    if !n.is_finite() || n < 0.0 {
        return throw(ctx, JsError::Message(format!("{key} must be a number")));
    }
    Ok(Some(n as u32))
}

fn value_to_string<'js>(ctx: &Ctx<'js>, value: &Value<'js>) -> rquickjs::Result<String> {
    if let Some(n) = value.as_number() {
        return Ok(n.to_string());
    }
    if let Some(text) = value.as_string() {
        return match text.to_string() {
            Ok(text) => Ok(text),
            Err(err) => throw(ctx, JsError::Message(err.to_string())),
        };
    }
    throw(
        ctx,
        JsError::Message("transition value must be a number".into()),
    )
}

/// `tekto.applyNativeOp(json)` runs one [`NativeOp`].
///
/// `json` is the same tagged object Rust uses (`{"op":"shortcutsRegister",...}`).
/// A string result is returned as a string. No result is JavaScript `null`.
/// Errors throw. Headless shortcut register throws `not available`.
/// A build without the feature throws `not implemented`.
fn js_apply_native_op<'js>(ctx: Ctx<'js>, spec: String) -> rquickjs::Result<Value<'js>> {
    let op = match serde_json::from_str::<NativeOp>(&spec) {
        Ok(op) => op,
        Err(err) => {
            return throw(&ctx, JsError::Message(format!("native op JSON: {err}")));
        }
    };
    match apply_native_host_op(&op) {
        Ok(None) => Ok(Value::new_null(ctx)),
        Ok(Some(text)) => {
            let js = match rquickjs::String::from_str(ctx.clone(), &text) {
                Ok(js) => js,
                Err(err) => return throw(&ctx, JsError::Message(err.to_string())),
            };
            Ok(Value::from_string(js))
        }
        Err(err) => throw(&ctx, err),
    }
}

fn js_remove(ctx: Ctx<'_>, id: u64) -> rquickjs::Result<()> {
    match with_host(|host| {
        host.apply(&[HostOp::Remove { id }])?;
        Ok(())
    }) {
        Ok(()) => Ok(()),
        Err(err) => throw(&ctx, err),
    }
}
