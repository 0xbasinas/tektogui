//! Notes on Android: CPU-rendered window inside a NativeActivity APK.
//!
//! Package (needs the NDK and `cargo-apk2`, MIT OR Apache-2.0):
//!
//! ```bash
//! source ~/.android-env.sh   # if present
//! cargo apk2 build -p notes-android
//! # or: cargo run -p tekto-cli -- build --target android
//! ```

use tekto::{
    handle_lifecycle, notes_tree, on_resumed, on_suspended, render_headless_to_file,
    surface_is_live, AppLifecycle,
};

/// Called from a later (or this) Activity when the app comes to the foreground.
#[no_mangle]
pub extern "C" fn tekto_on_resumed() {
    on_resumed();
}

/// Called when the Activity is pausing. The window surface is dropped in
/// the winit `suspended` hook; this flag stays in sync.
#[no_mangle]
pub extern "C" fn tekto_on_suspended() {
    on_suspended();
}

/// True after [`tekto_on_resumed`] and false after [`tekto_on_suspended`].
#[no_mangle]
pub extern "C" fn tekto_surface_is_live() -> bool {
    surface_is_live()
}

/// Layout and paint Notes headless. Used to prove the CPU core is linked
/// into the Android `.so`.
pub fn render_notes_headless() -> Result<Vec<u8>, String> {
    handle_lifecycle(AppLifecycle::Resumed);
    let tree = notes_tree(640.0, 400.0).map_err(|e| e.to_string())?;
    let png = render_headless_to_file(&tree, 640, 400, None).map_err(|e| e.to_string())?;
    handle_lifecycle(AppLifecycle::Suspended);
    if png.starts_with(b"\x89PNG") {
        Ok(png)
    } else {
        Err("not a PNG".into())
    }
}

/// NativeActivity entry. The window and dev server start here.
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: tekto::AndroidApp) {
    let tree = match notes_tree(640.0, 400.0) {
        Ok(t) => t,
        Err(err) => {
            eprintln!("notes-android: {err}");
            return;
        }
    };
    let runtime = tekto::AppRuntime::new(tree, "Notes");
    if let Err(err) = tekto::run_android(
        runtime,
        tekto::WindowOptions {
            title: "Notes".into(),
            width: 640,
            height: 400,
            dev_server: true,
        },
        app,
    ) {
        eprintln!("notes-android: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_stubs_and_headless_notes() {
        tekto_on_resumed();
        assert!(tekto_surface_is_live());
        let png = render_notes_headless().expect("headless notes");
        assert!(png.len() > 400);
        tekto_on_suspended();
        assert!(!tekto_surface_is_live());
    }
}
