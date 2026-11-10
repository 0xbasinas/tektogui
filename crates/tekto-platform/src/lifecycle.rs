//! App lifecycle hooks shared by desktop and Android.
//!
//! On Android, `Resumed` / `Suspended` must eventually create and drop the
//! window (and later GPU) surface. The hooks are stubbed so that work can
//! land without changing the public app API.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether a window / GPU surface may be used. False after [`on_suspended`].
static SURFACE_LIVE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppLifecycle {
    /// The app is in the foreground. A surface may be created.
    Resumed,
    /// The app is in the background. Drop the surface; do not draw.
    Suspended,
}

/// Record a lifecycle event. Safe to call from an Activity or the desktop loop.
pub fn handle_lifecycle(event: AppLifecycle) {
    match event {
        AppLifecycle::Resumed => on_resumed(),
        AppLifecycle::Suspended => on_suspended(),
    }
}

/// The Activity (or window) is in the foreground.
pub fn on_resumed() {
    SURFACE_LIVE.store(true, Ordering::SeqCst);
}

/// The Activity is pausing. Drop any window or GPU surface before returning.
pub fn on_suspended() {
    SURFACE_LIVE.store(false, Ordering::SeqCst);
}

pub fn surface_is_live() -> bool {
    SURFACE_LIVE.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspend_clears_the_live_surface_flag() {
        on_resumed();
        assert!(surface_is_live());
        on_suspended();
        assert!(!surface_is_live());
        on_resumed();
        assert!(surface_is_live());
        on_suspended();
    }
}
