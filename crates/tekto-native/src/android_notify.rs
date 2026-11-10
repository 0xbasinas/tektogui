//! Android status-bar notifications through `NotificationManager`.
//!
//! No extra protocol crate: JNI (`jni` 0.22, MIT OR Apache-2.0) calls the
//! platform APIs. The activity pointer comes from `ndk-context` 0.1
//! (MIT OR Apache-2.0), which `android-activity` fills in before `android_main`.
//!
//! API 33+ (`targetSdkVersion` 33 or higher) needs the runtime permission
//! `android.permission.POST_NOTIFICATIONS`. The manifest must declare it.
//! A denial is [`NativeError::Unavailable`], not a panic. The first denied
//! call asks the activity to show the system dialog, then returns. A later
//! `show` succeeds after the user allows notifications. On API 26+ the user
//! can also block the `tekto` channel. That is `Unavailable` as well.

use crate::NativeError;

/// API level that introduced the runtime notification permission.
const POST_NOTIFICATIONS_SDK: i32 = 33;
/// API level that requires a notification channel.
const CHANNEL_SDK: i32 = 26;
/// `NotificationManager.IMPORTANCE_NONE`. A blocked channel reports this.
const IMPORTANCE_NONE: i32 = 0;

pub(crate) fn plain_text(title: &str, body: &str) -> Result<(), NativeError> {
    if title.contains('\0') || body.contains('\0') {
        return Err(NativeError::Message(
            "notifications.show: title and body must not contain NUL".into(),
        ));
    }
    Ok(())
}

/// Decide whether a notification may be posted.
///
/// `permission_granted` is ignored below API 33. The OS grants
/// `POST_NOTIFICATIONS` automatically when `targetSdkVersion` is 32 or lower.
pub(crate) fn allow_post(
    sdk: i32,
    permission_granted: bool,
    notifications_enabled: bool,
) -> Result<(), NativeError> {
    if sdk >= POST_NOTIFICATIONS_SDK && !permission_granted {
        return Err(denied());
    }
    if !notifications_enabled {
        return Err(disabled());
    }
    Ok(())
}

/// Decide whether the `tekto` channel can show a notification.
///
/// Channels exist on API 26 and later. `None` means `getNotificationChannel`
/// returned null after the channel was created. `IMPORTANCE_NONE` (0) means
/// the user blocked that channel. Creating the channel again does not raise
/// an importance the user already set.
pub(crate) fn allow_channel(sdk: i32, importance: Option<i32>) -> Result<(), NativeError> {
    if sdk < CHANNEL_SDK {
        return Ok(());
    }
    match importance {
        Some(level) if level > IMPORTANCE_NONE => Ok(()),
        Some(_) => Err(channel_blocked()),
        None => Err(channel_missing()),
    }
}

fn denied() -> NativeError {
    NativeError::Unavailable {
        capability: "notifications.show",
        platform: "android",
        reason: "POST_NOTIFICATIONS was denied",
    }
}

fn disabled() -> NativeError {
    NativeError::Unavailable {
        capability: "notifications.show",
        platform: "android",
        reason: "notifications are disabled for this app",
    }
}

fn channel_blocked() -> NativeError {
    NativeError::Unavailable {
        capability: "notifications.show",
        platform: "android",
        reason: "the tekto notification channel is blocked",
    }
}

fn channel_missing() -> NativeError {
    NativeError::Unavailable {
        capability: "notifications.show",
        platform: "android",
        reason: "the tekto notification channel is missing",
    }
}

#[cfg(target_os = "android")]
fn not_ready() -> NativeError {
    NativeError::Unavailable {
        capability: "notifications.show",
        platform: "android",
        reason: "Android activity context is not ready",
    }
}

#[cfg(target_os = "android")]
fn service_unavailable() -> NativeError {
    NativeError::Unavailable {
        capability: "notifications.show",
        platform: "android",
        reason: "Android notification service is unavailable",
    }
}

#[cfg(target_os = "android")]
mod jni_post {
    use super::{
        allow_channel, allow_post, denied, not_ready, plain_text, service_unavailable, CHANNEL_SDK,
        POST_NOTIFICATIONS_SDK,
    };

    const CHANNEL_ID: &str = "tekto";
    /// `NotificationManager.IMPORTANCE_DEFAULT`.
    const IMPORTANCE_DEFAULT: i32 = 3;
    /// `PackageManager.PERMISSION_GRANTED`.
    const PERMISSION_GRANTED: i32 = 0;
    /// Stable id so a second `show` replaces the previous Tekto notice.
    const NOTIFICATION_ID: i32 = 1;
    /// Request code passed to `Activity.requestPermissions`. Not delivered back.
    const PERMISSION_REQUEST_CODE: i32 = 0x544B;
    use crate::NativeError;
    use jni::objects::{JObject, JValue};
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

    fn map_step(step: Step) -> NativeError {
        match step {
            Step::Native(err) => err,
            Step::Jni(jni::errors::Error::SecurityViolation) => denied(),
            Step::Jni(jni::errors::Error::CaughtJavaException { name, msg, .. })
                if name.contains("SecurityException") =>
            {
                let _ = msg;
                denied()
            }
            Step::Jni(jni::errors::Error::CaughtJavaException { name, msg, .. }) => {
                NativeError::Message(format!("notifications.show: {name}: {msg}"))
            }
            Step::Jni(err) => NativeError::Message(format!("notifications.show: {err}")),
        }
    }

    fn method_sig(sig: &str) -> Result<RuntimeMethodSignature, NativeError> {
        RuntimeMethodSignature::from_str(sig).map_err(|_| {
            NativeError::Message("notifications.show: internal JNI signature error".into())
        })
    }

    fn field_sig(sig: &str) -> Result<RuntimeFieldSignature, NativeError> {
        RuntimeFieldSignature::from_str(sig).map_err(|_| {
            NativeError::Message("notifications.show: internal JNI signature error".into())
        })
    }

    pub(crate) fn show(title: &str, body: &str) -> Result<(), NativeError> {
        plain_text(title, body)?;
        let (vm, context) = vm_and_context()?;
        vm.attach_current_thread(|env| post(env, context, title, body))
            .map_err(map_step)
    }

    fn vm_and_context() -> Result<(JavaVM, jobject), NativeError> {
        // `android_context` panics when `android-activity` has not run yet.
        let ctx =
            std::panic::catch_unwind(ndk_context::android_context).map_err(|_| not_ready())?;
        let vm = ctx.vm();
        let context = ctx.context();
        if vm.is_null() || context.is_null() {
            return Err(not_ready());
        }
        // SAFETY: android-activity published this JavaVM for the process.
        // `from_raw` only rejects a null pointer, which was checked above.
        let vm = unsafe { JavaVM::from_raw(vm.cast()) };
        Ok((vm, context.cast()))
    }

    fn post(
        env: &mut jni::Env<'_>,
        context_raw: jobject,
        title: &str,
        body: &str,
    ) -> Result<(), Step> {
        // SAFETY: the pointer is the global context ref from ndk-context.
        // `JObject` does not delete it on drop.
        let context = unsafe { JObject::from_raw(env, context_raw) };
        if context.is_null() {
            return Err(not_ready().into());
        }

        let version = env.find_class(JNIString::new("android/os/Build$VERSION"))?;
        let sdk_sig = field_sig("I")?;
        let sdk_sig = sdk_sig.field_signature();
        let sdk = env
            .get_static_field(&version, JNIString::new("SDK_INT"), &sdk_sig)?
            .i()?;

        let permission_granted = if sdk >= POST_NOTIFICATIONS_SDK {
            notification_permission(env, &context)?
        } else {
            true
        };
        let enabled = notifications_enabled(env, &context)?;
        allow_post(sdk, permission_granted, enabled)?;

        if sdk >= CHANNEL_SDK {
            create_channel(env, &context)?;
            let importance = channel_importance(env, &context)?;
            allow_channel(sdk, importance)?;
        }
        let notification = build_notification(env, &context, sdk, title, body)?;
        let manager = notification_manager(env, &context)?;
        let notify_sig = method_sig("(ILandroid/app/Notification;)V")?;
        let notify_sig = notify_sig.method_signature();
        env.call_method(
            &manager,
            JNIString::new("notify"),
            &notify_sig,
            &[JValue::Int(NOTIFICATION_ID), JValue::from(&notification)],
        )?;
        Ok(())
    }

    fn notification_permission(
        env: &mut jni::Env<'_>,
        context: &JObject<'_>,
    ) -> Result<bool, Step> {
        let name = env.new_string("android.permission.POST_NOTIFICATIONS")?;
        let sig = method_sig("(Ljava/lang/String;)I")?;
        let sig = sig.method_signature();
        let result = env
            .call_method(
                context,
                JNIString::new("checkSelfPermission"),
                &sig,
                &[JValue::from(&name)],
            )?
            .i()?;
        if result == PERMISSION_GRANTED {
            return Ok(true);
        }
        request_permission(env, context, &name)?;
        Ok(false)
    }

    fn request_permission(
        env: &mut jni::Env<'_>,
        context: &JObject<'_>,
        permission: &jni::objects::JString<'_>,
    ) -> Result<(), Step> {
        let activity = env.find_class(JNIString::new("android/app/Activity"))?;
        if !env.is_instance_of(context, &activity)? {
            return Ok(());
        }
        let string_class = env.find_class(JNIString::new("java/lang/String"))?;
        let names = env.new_object_array(1, &string_class, permission)?;
        let sig = method_sig("([Ljava/lang/String;I)V")?;
        let sig = sig.method_signature();
        env.call_method(
            context,
            JNIString::new("requestPermissions"),
            &sig,
            &[JValue::from(&names), JValue::Int(PERMISSION_REQUEST_CODE)],
        )?;
        Ok(())
    }

    fn notifications_enabled(env: &mut jni::Env<'_>, context: &JObject<'_>) -> Result<bool, Step> {
        let manager = notification_manager(env, context)?;
        let sig = method_sig("()Z")?;
        let sig = sig.method_signature();
        Ok(env
            .call_method(
                &manager,
                JNIString::new("areNotificationsEnabled"),
                &sig,
                &[],
            )?
            .z()?)
    }

    fn notification_manager<'local>(
        env: &mut jni::Env<'local>,
        context: &JObject<'_>,
    ) -> Result<JObject<'local>, Step> {
        let name = env.new_string("notification")?;
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
        if service.is_null() {
            return Err(service_unavailable().into());
        }
        Ok(service)
    }

    fn create_channel(env: &mut jni::Env<'_>, context: &JObject<'_>) -> Result<(), Step> {
        let manager = notification_manager(env, context)?;
        let channel_class = env.find_class(JNIString::new("android/app/NotificationChannel"))?;
        let id = env.new_string(CHANNEL_ID)?;
        let label = env.new_string("Tekto")?;
        let ctor = method_sig("(Ljava/lang/String;Ljava/lang/CharSequence;I)V")?;
        let ctor = ctor.method_signature();
        let channel = env.new_object(
            &channel_class,
            &ctor,
            &[
                JValue::from(&id),
                JValue::from(&label),
                JValue::Int(IMPORTANCE_DEFAULT),
            ],
        )?;
        let sig = method_sig("(Landroid/app/NotificationChannel;)V")?;
        let sig = sig.method_signature();
        env.call_method(
            &manager,
            JNIString::new("createNotificationChannel"),
            &sig,
            &[JValue::from(&channel)],
        )?;
        Ok(())
    }

    /// Importance of channel `tekto` after `createNotificationChannel`.
    ///
    /// `None` when the manager has no such channel. `IMPORTANCE_NONE` when
    /// the user blocked it. An existing channel keeps the user's importance.
    fn channel_importance(
        env: &mut jni::Env<'_>,
        context: &JObject<'_>,
    ) -> Result<Option<i32>, Step> {
        let manager = notification_manager(env, context)?;
        let id = env.new_string(CHANNEL_ID)?;
        let sig = method_sig("(Ljava/lang/String;)Landroid/app/NotificationChannel;")?;
        let sig = sig.method_signature();
        let channel = env
            .call_method(
                &manager,
                JNIString::new("getNotificationChannel"),
                &sig,
                &[JValue::from(&id)],
            )?
            .l()?;
        if channel.is_null() {
            return Ok(None);
        }
        let importance_sig = method_sig("()I")?;
        let importance_sig = importance_sig.method_signature();
        Ok(Some(
            env.call_method(
                &channel,
                JNIString::new("getImportance"),
                &importance_sig,
                &[],
            )?
            .i()?,
        ))
    }

    fn build_notification<'local>(
        env: &mut jni::Env<'local>,
        context: &JObject<'_>,
        sdk: i32,
        title: &str,
        body: &str,
    ) -> Result<JObject<'local>, Step> {
        let builder_class = env.find_class(JNIString::new("android/app/Notification$Builder"))?;
        let builder = if sdk >= CHANNEL_SDK {
            let id = env.new_string(CHANNEL_ID)?;
            let ctor = method_sig("(Landroid/content/Context;Ljava/lang/String;)V")?;
            let ctor = ctor.method_signature();
            env.new_object(
                &builder_class,
                &ctor,
                &[JValue::Object(context), JValue::from(&id)],
            )?
        } else {
            let ctor = method_sig("(Landroid/content/Context;)V")?;
            let ctor = ctor.method_signature();
            env.new_object(&builder_class, &ctor, &[JValue::Object(context)])?
        };
        let title = env.new_string(title)?;
        let title_sig = method_sig("(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;")?;
        let title_sig = title_sig.method_signature();
        let builder = env
            .call_method(
                &builder,
                JNIString::new("setContentTitle"),
                &title_sig,
                &[JValue::from(&title)],
            )?
            .l()?;
        let body = env.new_string(body)?;
        let builder = env
            .call_method(
                &builder,
                JNIString::new("setContentText"),
                &title_sig,
                &[JValue::from(&body)],
            )?
            .l()?;
        let icon = small_icon(env)?;
        let icon_sig = method_sig("(I)Landroid/app/Notification$Builder;")?;
        let icon_sig = icon_sig.method_signature();
        let builder = env
            .call_method(
                &builder,
                JNIString::new("setSmallIcon"),
                &icon_sig,
                &[JValue::Int(icon)],
            )?
            .l()?;
        let build_sig = method_sig("()Landroid/app/Notification;")?;
        let build_sig = build_sig.method_signature();
        Ok(env
            .call_method(&builder, JNIString::new("build"), &build_sig, &[])?
            .l()?)
    }

    fn small_icon(env: &mut jni::Env<'_>) -> Result<i32, Step> {
        let class = env.find_class(JNIString::new("android/R$drawable"))?;
        let sig = field_sig("I")?;
        let sig = sig.field_signature();
        Ok(env
            .get_static_field(&class, JNIString::new("ic_dialog_info"), &sig)?
            .i()?)
    }
}

#[cfg(target_os = "android")]
pub(crate) use jni_post::show;

#[cfg(test)]
mod tests {
    use super::{allow_channel, allow_post, plain_text};
    use crate::NativeError;

    #[test]
    fn android_permission_denial_is_unavailable() {
        let err = allow_post(33, false, true).expect_err("denied");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    capability: "notifications.show",
                    platform: "android",
                    reason: "POST_NOTIFICATIONS was denied",
                }
            ),
            "{err}"
        );
        let err = allow_post(34, false, false).expect_err("denied before disabled");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    reason: "POST_NOTIFICATIONS was denied",
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn android_disabled_notifications_are_unavailable() {
        let err = allow_post(30, true, false).expect_err("disabled");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    capability: "notifications.show",
                    platform: "android",
                    reason: "notifications are disabled for this app",
                }
            ),
            "{err}"
        );
        let err = allow_post(33, true, false).expect_err("disabled on 33");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    reason: "notifications are disabled for this app",
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn android_blocked_channel_is_unavailable() {
        let err = allow_channel(26, Some(0)).expect_err("importance none");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    capability: "notifications.show",
                    platform: "android",
                    reason: "the tekto notification channel is blocked",
                }
            ),
            "{err}"
        );
        let err = allow_channel(33, Some(0)).expect_err("blocked on 33");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    reason: "the tekto notification channel is blocked",
                    ..
                }
            ),
            "{err}"
        );
        let err = allow_channel(26, None).expect_err("missing channel");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    reason: "the tekto notification channel is missing",
                    ..
                }
            ),
            "{err}"
        );
        assert!(allow_channel(25, Some(0)).is_ok());
        assert!(allow_channel(26, Some(3)).is_ok());
        assert!(allow_channel(34, Some(1)).is_ok());
    }

    #[test]
    fn android_post_is_allowed_when_the_os_allows_it() {
        assert!(allow_post(32, false, true).is_ok());
        assert!(allow_post(33, true, true).is_ok());
        assert!(allow_post(24, true, true).is_ok());
    }

    #[test]
    fn android_notification_text_rejects_nul() {
        assert!(plain_text("Saved", "Note stored").is_ok());
        assert!(matches!(
            plain_text("a\0b", "body"),
            Err(NativeError::Message(_))
        ));
        assert!(matches!(
            plain_text("title", "a\0b"),
            Err(NativeError::Message(_))
        ));
    }
}
