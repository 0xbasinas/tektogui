//! When a window may try wgpu / Vello.
//!
//! Android stays on the CPU painter unless `TEKTO_ANDROID_GPU=1`.
//! This repo was not run on a device or emulator. The build host has no
//! `/dev/dri`, so a Vulkan or GLES driver cannot be treated as reliable.
//! The default APK does not enable the `gpu` feature at all.
//!
//! A lost `ANativeWindow` is not a reason to present. [`crate::lifecycle`]
//! retires the surface epoch on suspend, and the window loop skips
//! `buffer_mut` / `present` for that epoch. A GPU readback error falls
//! back to the CPU painter only while the epoch is still live.

/// Pure policy used by the window loop and by tests.
///
/// `feature_on` is the `gpu` Cargo feature. `env_opt_in` is
/// `TEKTO_ANDROID_GPU=1`. Desktop may try the GPU whenever the feature
/// is on. Android may try it only with both.
pub fn gpu_paint_allowed(feature_on: bool, android: bool, env_opt_in: bool) -> bool {
    feature_on && (!android || env_opt_in)
}

/// Whether this process should construct a [`tekto_render::GpuRenderer`].
pub fn wants_gpu_paint() -> bool {
    gpu_paint_allowed(
        cfg!(feature = "gpu"),
        cfg!(target_os = "android"),
        std::env::var_os("TEKTO_ANDROID_GPU").is_some_and(|value| value == "1"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_stays_on_cpu_unless_the_feature_and_env_opt_in() {
        assert!(!gpu_paint_allowed(false, true, true));
        assert!(!gpu_paint_allowed(false, true, false));
        assert!(!gpu_paint_allowed(true, true, false));
        assert!(gpu_paint_allowed(true, true, true));
    }

    #[test]
    fn desktop_tries_gpu_when_the_feature_is_on() {
        assert!(gpu_paint_allowed(true, false, false));
        assert!(!gpu_paint_allowed(false, false, true));
    }
}
