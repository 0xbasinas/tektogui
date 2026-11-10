//! Android text clipboard through `ClipboardManager`.
//!
//! No extra protocol crate: JNI (`jni` 0.22, MIT OR Apache-2.0) calls the
//! platform APIs. The activity pointer comes from `ndk-context` 0.1
//! (MIT OR Apache-2.0), which `android-activity` fills in before `android_main`.
//!
//! On API 29+ the OS only lets the focused app (or the default IME) read the
//! clipboard. This plugin is not an IME. A read while the activity has no
//! window focus is [`NativeError::Unavailable`]. `setPrimaryClip` is still
//! allowed in the background. Android 12+ may show a system toast when an app
//! reads or copies; that toast is not an error. A live device check is a
//! follow-up. An empty or non-text clip reads as an empty string.

use crate::NativeError;

/// API level that hides clipboard reads from an app that does not have focus.
const CLIPBOARD_FOCUS_SDK: i32 = 29;

/// Reject text that JNI would truncate or that desktop clipboard owners treat
/// as a C string. Other platforms pass the string to `arboard`, which stops
/// at the first NUL on X11 and Windows.
pub(crate) fn plain_text(text: &str) -> Result<(), NativeError> {
    if text.contains('\0') {
        return Err(NativeError::Message(
            "clipboard.set: text must not contain NUL".into(),
        ));
    }
    Ok(())
}

/// A missing `ndk-context` handle. The process has no activity yet.
/// `capability` is `clipboard.get` or `clipboard.set`.
pub(crate) fn require_context(
    vm_present: bool,
    context_present: bool,
    capability: &'static str,
) -> Result<(), NativeError> {
    if vm_present && context_present {
        Ok(())
    } else {
        Err(not_ready(capability))
    }
}

/// `getSystemService("clipboard")` returned null.
pub(crate) fn require_service(present: bool, capability: &'static str) -> Result<(), NativeError> {
    if present {
        Ok(())
    } else {
        Err(service_unavailable(capability))
    }
}

/// Decide whether a clipboard read is allowed.
///
/// `has_focus` is `Activity.hasWindowFocus`. Below API 29 the OS does not
/// apply the focus rule, so a background read is allowed. This check does
/// not treat the process as the default IME.
pub(crate) fn allow_read(sdk: i32, has_focus: bool) -> Result<(), NativeError> {
    if sdk >= CLIPBOARD_FOCUS_SDK && !has_focus {
        return Err(background_read());
    }
    Ok(())
}

/// Turn a focused read into text. `None` is an empty clipboard or a clip
/// whose first item is not text (`ClipData.Item.getText` is null).
pub(crate) fn plan_get(
    sdk: i32,
    has_focus: bool,
    text: Option<String>,
) -> Result<String, NativeError> {
    allow_read(sdk, has_focus)?;
    Ok(text.unwrap_or_default())
}

/// Validate text before `setPrimaryClip`. Focus is not required.
pub(crate) fn plan_set(text: &str) -> Result<(), NativeError> {
    plain_text(text)
}

fn not_ready(capability: &'static str) -> NativeError {
    NativeError::Unavailable {
        capability,
        platform: "android",
        reason: "Android activity context is not ready",
    }
}

/// First item of a primary clip. Host tests use this instead of JNI.
///
/// `Text` is `ClipData.Item.getText`. `Uri`, `Intent`, and `Image` are
/// non-text items. `coerceToText` is not used: Android can turn those into
/// a URI or intent string.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ClipPayload {
    Empty,
    Text(String),
    Uri,
    Intent,
    Image,
}

/// Text from a clip item. Non-text payloads are `None` (the read becomes `""`).
#[cfg(test)]
pub(crate) fn payload_text(payload: &ClipPayload) -> Option<String> {
    match payload {
        ClipPayload::Text(text) => Some(text.clone()),
        ClipPayload::Empty | ClipPayload::Uri | ClipPayload::Intent | ClipPayload::Image => None,
    }
}

fn service_unavailable(capability: &'static str) -> NativeError {
    NativeError::Unavailable {
        capability,
        platform: "android",
        reason: "Android clipboard service is unavailable",
    }
}

fn background_read() -> NativeError {
    NativeError::Unavailable {
        capability: "clipboard.get",
        platform: "android",
        reason: "the app is in the background",
    }
}

/// In-memory stand-in for `ClipboardManager`. Host tests use this so the
/// success path and the focus rule do not need a device.
#[cfg(test)]
struct MemoryClipboard {
    sdk: i32,
    has_focus: bool,
    payload: ClipPayload,
}

#[cfg(test)]
impl MemoryClipboard {
    fn get_text(&self) -> Result<String, NativeError> {
        plan_get(self.sdk, self.has_focus, payload_text(&self.payload))
    }

    fn set_text(&mut self, text: &str) -> Result<(), NativeError> {
        plan_set(text)?;
        self.payload = ClipPayload::Text(text.to_string());
        Ok(())
    }
}

#[cfg(target_os = "android")]
mod jni_clip {
    use super::{not_ready, plan_get, plan_set, require_context, require_service};
    use crate::NativeError;
    use jni::objects::{JObject, JString, JValue};
    use jni::signature::{RuntimeFieldSignature, RuntimeMethodSignature};
    use jni::strings::JNIString;
    use jni::sys::jobject;
    use jni::JavaVM;

    enum Step {
        Native(NativeError),
        Jni(jni::errors::Error),
    }

    impl From<NativeError> for Step {
        fn from(value: NativeError) -> Self {
            Step::Native(value)
        }
    }

    impl From<jni::errors::Error> for Step {
        fn from(value: jni::errors::Error) -> Self {
            Step::Jni(value)
        }
    }

    fn denied(capability: &'static str) -> NativeError {
        NativeError::Unavailable {
            capability,
            platform: "android",
            reason: "clipboard access was denied",
        }
    }

    fn map_step(capability: &'static str, step: Step) -> NativeError {
        match step {
            Step::Native(err) => err,
            Step::Jni(jni::errors::Error::SecurityViolation) => denied(capability),
            Step::Jni(jni::errors::Error::CaughtJavaException { name, msg, .. })
                if name.contains("SecurityException") =>
            {
                let _ = msg;
                denied(capability)
            }
            Step::Jni(jni::errors::Error::CaughtJavaException { name, msg, .. }) => {
                NativeError::Message(format!("{capability}: {name}: {msg}"))
            }
            Step::Jni(err) => NativeError::Message(format!("{capability}: {err}")),
        }
    }

    fn method_sig(sig: &str) -> Result<RuntimeMethodSignature, NativeError> {
        RuntimeMethodSignature::from_str(sig)
            .map_err(|_| NativeError::Message("clipboard: internal JNI signature error".into()))
    }

    fn field_sig(sig: &str) -> Result<RuntimeFieldSignature, NativeError> {
        RuntimeFieldSignature::from_str(sig)
            .map_err(|_| NativeError::Message("clipboard: internal JNI signature error".into()))
    }

    pub(crate) fn get_text() -> Result<String, NativeError> {
        let (vm, context) = vm_and_context("clipboard.get")?;
        vm.attach_current_thread(|env| read_clip(env, context))
            .map_err(|step| map_step("clipboard.get", step))
    }

    pub(crate) fn set_text(text: &str) -> Result<(), NativeError> {
        plan_set(text)?;
        let (vm, context) = vm_and_context("clipboard.set")?;
        vm.attach_current_thread(|env| write_clip(env, context, text))
            .map_err(|step| map_step("clipboard.set", step))
    }

    fn vm_and_context(capability: &'static str) -> Result<(JavaVM, jobject), NativeError> {
        // `android_context` panics when `android-activity` has not run yet.
        let ctx = std::panic::catch_unwind(ndk_context::android_context)
            .map_err(|_| not_ready(capability))?;
        let vm = ctx.vm();
        let context = ctx.context();
        require_context(!vm.is_null(), !context.is_null(), capability)?;
        // SAFETY: android-activity published this JavaVM for the process.
        // `from_raw` only wraps the pointer. Null was rejected above.
        let vm = unsafe { JavaVM::from_raw(vm.cast()) };
        Ok((vm, context.cast()))
    }

    fn read_clip(env: &mut jni::Env<'_>, context_raw: jobject) -> Result<String, Step> {
        // SAFETY: the pointer is the global context ref from ndk-context.
        // `JObject` does not delete it on drop.
        let context = unsafe { JObject::from_raw(env, context_raw) };
        require_context(true, !context.is_null(), "clipboard.get")?;
        let sdk = sdk_int(env)?;
        let has_focus = window_focus(env, &context)?;
        super::allow_read(sdk, has_focus)?;
        let manager = clipboard_manager(env, &context, "clipboard.get")?;
        let text = primary_text(env, &manager)?;
        Ok(plan_get(sdk, has_focus, text)?)
    }

    fn write_clip(env: &mut jni::Env<'_>, context_raw: jobject, text: &str) -> Result<(), Step> {
        let context = unsafe { JObject::from_raw(env, context_raw) };
        require_context(true, !context.is_null(), "clipboard.set")?;
        let manager = clipboard_manager(env, &context, "clipboard.set")?;
        let clip_class = env.find_class(JNIString::new("android/content/ClipData"))?;
        let label = env.new_string("text")?;
        let value = env.new_string(text)?;
        let new_sig = method_sig(
            "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;",
        )?;
        let new_sig = new_sig.method_signature();
        let clip = env
            .call_static_method(
                &clip_class,
                JNIString::new("newPlainText"),
                &new_sig,
                &[JValue::from(&label), JValue::from(&value)],
            )?
            .l()?;
        let set_sig = method_sig("(Landroid/content/ClipData;)V")?;
        let set_sig = set_sig.method_signature();
        env.call_method(
            &manager,
            JNIString::new("setPrimaryClip"),
            &set_sig,
            &[JValue::from(&clip)],
        )?;
        Ok(())
    }

    fn sdk_int(env: &mut jni::Env<'_>) -> Result<i32, Step> {
        let version = env.find_class(JNIString::new("android/os/Build$VERSION"))?;
        let sdk_sig = field_sig("I")?;
        let sdk_sig = sdk_sig.field_signature();
        Ok(env
            .get_static_field(&version, JNIString::new("SDK_INT"), &sdk_sig)?
            .i()?)
    }

    /// `true` when the context is an `Activity` that has window focus.
    /// Any other context counts as unfocused. API 29+ then refuses the read.
    fn window_focus(env: &mut jni::Env<'_>, context: &JObject<'_>) -> Result<bool, Step> {
        let activity = env.find_class(JNIString::new("android/app/Activity"))?;
        if !env.is_instance_of(context, &activity)? {
            return Ok(false);
        }
        let sig = method_sig("()Z")?;
        let sig = sig.method_signature();
        Ok(env
            .call_method(context, JNIString::new("hasWindowFocus"), &sig, &[])?
            .z()?)
    }

    fn clipboard_manager<'local>(
        env: &mut jni::Env<'local>,
        context: &JObject<'_>,
        capability: &'static str,
    ) -> Result<JObject<'local>, Step> {
        let name = env.new_string("clipboard")?;
        let sig = method_sig("(Ljava/lang/String;)Ljava/lang/Object;")?;
        let sig = sig.method_signature();
        let service = env
            .call_method(
                context,
                JNIString::new("getSystemService"),
                &sig,
                &[JValue::from(&name)],
            )?
            .l()?;
        require_service(!service.is_null(), capability)?;
        Ok(service)
    }

    fn primary_text(env: &mut jni::Env<'_>, manager: &JObject<'_>) -> Result<Option<String>, Step> {
        let has_sig = method_sig("()Z")?;
        let has_sig = has_sig.method_signature();
        let has = env
            .call_method(manager, JNIString::new("hasPrimaryClip"), &has_sig, &[])?
            .z()?;
        if !has {
            return Ok(None);
        }
        let get_sig = method_sig("()Landroid/content/ClipData;")?;
        let get_sig = get_sig.method_signature();
        let clip = env
            .call_method(manager, JNIString::new("getPrimaryClip"), &get_sig, &[])?
            .l()?;
        if clip.is_null() {
            return Ok(None);
        }
        let count_sig = method_sig("()I")?;
        let count_sig = count_sig.method_signature();
        let count = env
            .call_method(&clip, JNIString::new("getItemCount"), &count_sig, &[])?
            .i()?;
        if count <= 0 {
            return Ok(None);
        }
        let item_sig = method_sig("(I)Landroid/content/ClipData$Item;")?;
        let item_sig = item_sig.method_signature();
        let item = env
            .call_method(
                &clip,
                JNIString::new("getItemAt"),
                &item_sig,
                &[JValue::Int(0)],
            )?
            .l()?;
        if item.is_null() {
            return Ok(None);
        }
        // `getText` is null for a URI, intent, or image item. Do not call
        // `coerceToText`: that can return the URI or an intent string.
        let text_sig = method_sig("()Ljava/lang/CharSequence;")?;
        let text_sig = text_sig.method_signature();
        let chars = env
            .call_method(&item, JNIString::new("getText"), &text_sig, &[])?
            .l()?;
        if chars.is_null() {
            return Ok(None);
        }
        Ok(Some(char_sequence_string(env, &chars)?))
    }

    fn char_sequence_string(env: &mut jni::Env<'_>, value: &JObject<'_>) -> Result<String, Step> {
        let sig = method_sig("()Ljava/lang/String;")?;
        let sig = sig.method_signature();
        let string_obj = env
            .call_method(value, JNIString::new("toString"), &sig, &[])?
            .l()?;
        if string_obj.is_null() {
            return Ok(String::new());
        }
        // SAFETY: `toString` on a non-null CharSequence returns a String.
        // `from_raw` does not take ownership of the local ref.
        let jstring = unsafe { JString::from_raw(env, string_obj.as_raw()) };
        Ok(jstring.try_to_string(env)?)
    }
}

#[cfg(target_os = "android")]
pub(crate) use jni_clip::{get_text, set_text};

#[cfg(test)]
mod tests {
    use super::{
        allow_read, payload_text, plan_get, plan_set, require_context, require_service,
        ClipPayload, MemoryClipboard,
    };
    use crate::NativeError;

    #[test]
    fn android_clipboard_round_trip_through_a_fake_manager() {
        let mut clip = MemoryClipboard {
            sdk: 34,
            has_focus: true,
            payload: ClipPayload::Empty,
        };
        assert_eq!(clip.get_text().expect("empty read"), "");
        clip.set_text("hello").expect("set");
        assert_eq!(clip.get_text().expect("get"), "hello");
        clip.set_text("").expect("clear");
        assert_eq!(clip.get_text().expect("cleared"), "");
    }

    #[test]
    fn android_clipboard_read_in_background_is_unavailable() {
        let err = allow_read(29, false).expect_err("api 29");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    capability: "clipboard.get",
                    platform: "android",
                    reason: "the app is in the background",
                }
            ),
            "{err}"
        );
        let mut clip = MemoryClipboard {
            sdk: 34,
            has_focus: false,
            payload: ClipPayload::Text("secret".into()),
        };
        let err = clip.get_text().expect_err("background get");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    reason: "the app is in the background",
                    ..
                }
            ),
            "{err}"
        );
        clip.set_text("still written").expect("background set");
        assert_eq!(clip.payload, ClipPayload::Text("still written".into()));
        assert!(allow_read(28, false).is_ok());
        assert!(allow_read(29, true).is_ok());
        assert_eq!(
            plan_get(28, false, Some("old".into())).expect("pre-29"),
            "old"
        );
    }

    #[test]
    fn android_clipboard_missing_context_and_service() {
        let err = require_context(false, false, "clipboard.get").expect_err("no vm");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    capability: "clipboard.get",
                    platform: "android",
                    reason: "Android activity context is not ready",
                }
            ),
            "{err}"
        );
        let err = require_context(false, false, "clipboard.set").expect_err("no vm on set");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    capability: "clipboard.set",
                    platform: "android",
                    reason: "Android activity context is not ready",
                }
            ),
            "{err}"
        );
        assert!(require_context(true, false, "clipboard.get").is_err());
        assert!(require_context(false, true, "clipboard.set").is_err());
        assert!(require_context(true, true, "clipboard.get").is_ok());
        assert!(require_context(true, true, "clipboard.set").is_ok());

        let err = require_service(false, "clipboard.get").expect_err("no service");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    capability: "clipboard.get",
                    platform: "android",
                    reason: "Android clipboard service is unavailable",
                }
            ),
            "{err}"
        );
        let err = require_service(false, "clipboard.set").expect_err("no service on set");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    capability: "clipboard.set",
                    reason: "Android clipboard service is unavailable",
                    ..
                }
            ),
            "{err}"
        );
        assert!(require_service(true, "clipboard.get").is_ok());
    }

    #[test]
    fn android_clipboard_rejects_nul_and_non_text() {
        let err = plan_set("a\0b").expect_err("nul");
        let NativeError::Message(msg) = err else {
            panic!("expected Message");
        };
        assert!(msg.contains("NUL"), "{msg}");
        let mut clip = MemoryClipboard {
            sdk: 33,
            has_focus: true,
            payload: ClipPayload::Empty,
        };
        assert!(clip.set_text("a\0b").is_err());
        assert_eq!(clip.payload, ClipPayload::Empty);
        assert_eq!(plan_get(33, true, None).expect("non-text"), "");
        assert!(plan_set("ok").is_ok());
    }

    #[test]
    fn android_clipboard_non_text_clip_reads_as_empty() {
        for payload in [ClipPayload::Uri, ClipPayload::Intent, ClipPayload::Image] {
            assert_eq!(payload_text(&payload), None, "{payload:?}");
            let clip = MemoryClipboard {
                sdk: 34,
                has_focus: true,
                payload: payload.clone(),
            };
            assert_eq!(clip.get_text().expect("non-text read"), "", "{payload:?}");
        }
        assert_eq!(
            payload_text(&ClipPayload::Text("plain".into())).as_deref(),
            Some("plain")
        );
        assert_eq!(
            plan_get(34, true, payload_text(&ClipPayload::Image)).expect("image"),
            ""
        );
    }
}
