//! Notes on Android: the CPU core plus `Resumed` / `Suspended` stubs.
//!
//! Compile the shared library with:
//!
//! ```bash
//! source ~/.android-env.sh   # if present
//! cargo ndk -t arm64-v8a check -p notes-android
//! # or:
//! cargo check -p notes-android --target aarch64-linux-android
//! ```
//!
//! There is no Activity / APK wrapper yet, so this does not install or
//! open a window on a device. A later NativeActivity or GameActivity can
//! call the `tekto_on_resumed` / `tekto_on_suspended` symbols and, in
//! Month 2, create or drop the surface there.

use tekto::{
    handle_lifecycle, notes_tree, on_resumed, on_suspended, render_headless_to_file,
    surface_is_live, AppLifecycle,
};

/// Called from a later Activity when the app comes to the foreground.
#[no_mangle]
pub extern "C" fn tekto_on_resumed() {
    on_resumed();
}

/// Called from a later Activity when the app is pausing. Drops the
/// surface flag; Month 2 will drop the real window / GPU surface here.
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
/// into the Android `.so`. Returns whether a PNG was produced.
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
