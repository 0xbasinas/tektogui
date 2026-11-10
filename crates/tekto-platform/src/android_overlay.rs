//! Native `EditText` overlay for Android.
//!
//! winit's Android backend shows the soft keyboard from `set_ime_allowed`
//! but does not deliver `WindowEvent::Ime`. `NativeActivity` text input is
//! a no-op in android-activity. A Java `EditText` in the APK
//! (`com.tektogui.ime.FieldOverlay`) takes the IME instead. This module
//! polls it and keeps the Tekto field in sync.
//!
//! The class is packaged by cargo-apk2 from `examples/notes-android/java`.
//! If it is missing, the caller falls back to winit key events.

use crate::android_field::{self, AndroidFieldCommand, NativeFieldSample};
use jni::objects::{JObject, JString};
use jni::{jni_sig, jni_str, JavaVM};
use winit::window::Window;

pub(crate) struct Overlay {
    pub(crate) sync: android_field::AndroidFieldSync,
    /// `FieldOverlay` loaded. False uses the winit key path.
    pub(crate) ready: bool,
    missing_logged: bool,
    app: winit::platform::android::activity::AndroidApp,
}

impl Overlay {
    pub(crate) fn new(app: winit::platform::android::activity::AndroidApp) -> Self {
        Self {
            sync: android_field::AndroidFieldSync::default(),
            ready: false,
            missing_logged: false,
            app,
        }
    }
}

enum Poll {
    Sample(NativeFieldSample),
    Missing,
    /// JNI failed after the class was expected to be there. Do not write "".
    Failed,
}

/// Show, move, or hide the overlay and copy a native edit into the tree.
///
/// Returns false when the Java class is not in the APK.
pub(crate) fn sync_overlay(
    overlay: &mut Overlay,
    window: &Window,
    tree: &std::sync::Mutex<tekto_tree::Tree>,
) -> bool {
    let scale = window.scale_factor() as f32;
    let focused = tree
        .lock()
        .ok()
        .and_then(|tree| android_field::focused_text_field(&tree, scale));
    let native = match poll(&overlay.app) {
        Poll::Missing => {
            if !overlay.missing_logged {
                overlay.missing_logged = true;
                eprintln!(
                    "tekto: com.tektogui.ime.FieldOverlay is not in this APK; Android text uses key events only"
                );
            }
            overlay.ready = false;
            return false;
        }
        Poll::Failed => return overlay.ready,
        Poll::Sample(sample) => sample,
    };
    overlay.ready = true;
    let step = android_field::step_android_field(&mut overlay.sync, focused.as_ref(), &native);
    if let Some((text, caret)) = step.write {
        if let Ok(mut tree) = tree.lock() {
            if android_field::apply_field_write(&mut tree, &text, caret) {
                window.request_redraw();
            }
        }
    }
    if let Err(err) = apply_command(&overlay.app, &step.command) {
        eprintln!("tekto: android field overlay: {err}");
    }
    true
}

pub(crate) fn hide(overlay: &mut Overlay) {
    overlay.sync = android_field::AndroidFieldSync::default();
    let _ = apply_command(&overlay.app, &AndroidFieldCommand::Hide);
}

fn with_env<T>(
    app: &winit::platform::android::activity::AndroidApp,
    body: impl FnOnce(&mut jni::Env, &JObject) -> Result<T, String>,
) -> Result<T, String> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let mut output = None;
    let mut error = None;
    let attached = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        let raw = app.activity_as_ptr() as jni::sys::jobject;
        let activity = unsafe { JObject::from_raw(env, raw) };
        match body(env, &activity) {
            Ok(value) => output = Some(value),
            Err(err) => error = Some(err),
        }
        if env.exception_check() {
            env.exception_clear();
        }
        Ok(())
    });
    if let Err(err) = attached {
        return Err(err.to_string());
    }
    if let Some(err) = error {
        return Err(err);
    }
    output.ok_or_else(|| "android jni returned nothing".into())
}

fn poll(app: &winit::platform::android::activity::AndroidApp) -> Poll {
    match with_env(app, |env, _activity| read_sample(env)) {
        Ok(sample) => Poll::Sample(sample),
        Err(err) => {
            if err.contains("FieldOverlay")
                || err.contains("ClassNotFound")
                || err.contains("NoClass")
            {
                Poll::Missing
            } else {
                eprintln!("tekto: android field poll: {err}");
                Poll::Failed
            }
        }
    }
}

fn read_sample(env: &mut jni::Env) -> Result<NativeFieldSample, String> {
    let class = env
        .find_class(jni_str!("com/tektogui/ime/FieldOverlay"))
        .map_err(|err| format!("FieldOverlay {err}"))?;
    let text_value = env
        .call_static_method(
            &class,
            jni_str!("text"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )
        .map_err(|err| err.to_string())?;
    let text = match text_value {
        jni::JValueOwned::Object(obj) => {
            if obj.is_null() {
                String::new()
            } else {
                let raw = obj.as_raw();
                let js =
                    unsafe { env.as_cast_raw::<JString>(&raw) }.map_err(|err| err.to_string())?;
                let text = js
                    .mutf8_chars(env)
                    .map_err(|err| err.to_string())?
                    .to_string();
                text
            }
        }
        other => return Err(format!("FieldOverlay.text returned {other:?}")),
    };
    let caret_value = env
        .call_static_method(&class, jni_str!("caret"), jni_sig!("()I"), &[])
        .map_err(|err| err.to_string())?;
    let caret = match caret_value {
        jni::JValueOwned::Int(n) => n,
        other => return Err(format!("FieldOverlay.caret returned {other:?}")),
    };
    Ok(NativeFieldSample { text, caret })
}

fn apply_command(
    app: &winit::platform::android::activity::AndroidApp,
    command: &AndroidFieldCommand,
) -> Result<(), String> {
    with_env(app, |env, activity| {
        let class = env
            .find_class(jni_str!("com/tektogui/ime/FieldOverlay"))
            .map_err(|err| format!("FieldOverlay {err}"))?;
        let (show, push, text, caret, rect) = match command {
            AndroidFieldCommand::Hide => (
                false,
                false,
                String::new(),
                0,
                android_field::FieldRectPx {
                    x: 0,
                    y: 0,
                    w: 1,
                    h: 1,
                },
            ),
            AndroidFieldCommand::Show {
                push_text,
                text,
                caret,
                rect,
                ..
            } => (true, *push_text, text.clone(), *caret, *rect),
        };
        let jtext = env.new_string(&text).map_err(|err| err.to_string())?;
        env.call_static_method(
            &class,
            jni_str!("sync"),
            jni_sig!("(Landroid/app/Activity;ZZLjava/lang/String;IIIII)V"),
            &[
                jni::JValue::Object(activity),
                jni::JValue::Bool(show),
                jni::JValue::Bool(push),
                jni::JValue::Object(jtext.as_ref()),
                jni::JValue::Int(caret),
                jni::JValue::Int(rect.x),
                jni::JValue::Int(rect.y),
                jni::JValue::Int(rect.w),
                jni::JValue::Int(rect.h),
            ],
        )
        .map_err(|err| err.to_string())?;
        Ok(())
    })
}
