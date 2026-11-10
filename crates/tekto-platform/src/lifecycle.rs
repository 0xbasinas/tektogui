//! App lifecycle hooks shared by desktop and Android.
//!
//! On Android, winit delivers `resumed` when the native window is created
//! (`InitWindow`) and `suspended` when that window is destroyed
//! (`TerminateWindow`). The CPU softbuffer holds an `ANativeWindow`. It must
//! be dropped inside `suspended`, before the winit window, or the next
//! resume locks a destroyed surface and can crash or leak a buffer.
//!
//! At most one [`SurfaceEpoch`] is current. A stale epoch cannot present.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Whether a window / GPU surface may be used. False after [`on_suspended`].
static SURFACE_LIVE: AtomicBool = AtomicBool::new(false);

/// Epoch of the single current surface. Zero means no surface is held.
static HELD_EPOCH: AtomicU64 = AtomicU64::new(0);

/// Monotonic source for [`SurfaceEpoch`]. Starts at 1 so 0 stays "none".
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppLifecycle {
    /// The app is in the foreground. A surface may be created.
    Resumed,
    /// The app is in the background. Drop the surface; do not draw.
    Suspended,
}

/// Identity of one acquired window surface.
///
/// Only the latest epoch may present. Dropping it (or a newer acquire)
/// retires the previous buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceEpoch(u64);

/// Record a lifecycle event. Safe to call from an Activity or the desktop loop.
pub fn handle_lifecycle(event: AppLifecycle) {
    match event {
        AppLifecycle::Resumed => on_resumed(),
        AppLifecycle::Suspended => on_suspended(),
    }
}

/// The Activity (or window) is in the foreground.
///
/// This flag alone does not allocate a buffer. Window code must
/// [`begin_surface`] before it presents.
pub fn on_resumed() {
    SURFACE_LIVE.store(true, Ordering::SeqCst);
}

/// The Activity is pausing. Drop any window or GPU surface before returning.
///
/// Clears the held epoch so a buffer acquired before this call cannot present.
pub fn on_suspended() {
    HELD_EPOCH.store(0, Ordering::SeqCst);
    SURFACE_LIVE.store(false, Ordering::SeqCst);
}

pub fn surface_is_live() -> bool {
    SURFACE_LIVE.load(Ordering::SeqCst)
}

/// Acquire the single live surface slot.
///
/// If a previous epoch was still held, it is retired. Callers must drop that
/// buffer before presenting with the new epoch. The open count never exceeds 1.
pub(crate) fn begin_surface() -> SurfaceEpoch {
    let epoch = NEXT_EPOCH.fetch_add(1, Ordering::SeqCst);
    HELD_EPOCH.store(epoch, Ordering::SeqCst);
    SURFACE_LIVE.store(true, Ordering::SeqCst);
    SurfaceEpoch(epoch)
}

/// Release `epoch` if it is still the current surface.
///
/// A second release, or a release of a retired epoch, does not clear a newer
/// surface and does not underflow the open count.
pub(crate) fn end_surface(epoch: SurfaceEpoch) -> bool {
    let closed = HELD_EPOCH
        .compare_exchange(epoch.0, 0, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok();
    if closed {
        SURFACE_LIVE.store(false, Ordering::SeqCst);
    }
    closed
}

/// True only for the epoch that currently owns the buffer.
pub(crate) fn may_present(epoch: SurfaceEpoch) -> bool {
    SURFACE_LIVE.load(Ordering::SeqCst) && HELD_EPOCH.load(Ordering::SeqCst) == epoch.0
}

/// `1` when a surface epoch is held, `0` otherwise.
#[cfg(test)]
fn open_surface_count() -> u64 {
    if HELD_EPOCH.load(Ordering::SeqCst) == 0 {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The epoch counters are process-global. Keep these tests serialized.
    static LOCK: Mutex<()> = Mutex::new(());

    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            NEXT_EPOCH.store(1, Ordering::SeqCst);
            HELD_EPOCH.store(0, Ordering::SeqCst);
            SURFACE_LIVE.store(false, Ordering::SeqCst);
        }
    }

    fn guard() -> (std::sync::MutexGuard<'static, ()>, Reset) {
        let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        NEXT_EPOCH.store(1, Ordering::SeqCst);
        HELD_EPOCH.store(0, Ordering::SeqCst);
        SURFACE_LIVE.store(false, Ordering::SeqCst);
        (lock, Reset)
    }

    #[test]
    fn suspend_clears_the_live_surface_flag() {
        let _guard = guard();
        on_resumed();
        assert!(surface_is_live());
        on_suspended();
        assert!(!surface_is_live());
        on_resumed();
        assert!(surface_is_live());
        on_suspended();
        assert!(!surface_is_live());
        assert_eq!(open_surface_count(), 0);
    }

    #[test]
    fn suspend_drops_the_buffer_and_resume_reacquires_once() {
        let _guard = guard();
        let first = begin_surface();
        assert!(surface_is_live());
        assert!(may_present(first));
        assert_eq!(open_surface_count(), 1);

        assert!(end_surface(first));
        assert!(!surface_is_live());
        assert!(!may_present(first));
        assert_eq!(open_surface_count(), 0);
        // A second suspend must not underflow or revive the retired buffer.
        assert!(!end_surface(first));
        on_suspended();
        assert_eq!(open_surface_count(), 0);

        let second = begin_surface();
        assert_ne!(first, second);
        assert!(may_present(second));
        assert!(!may_present(first));
        assert_eq!(open_surface_count(), 1);
        assert!(end_surface(second));
        assert!(!surface_is_live());
    }

    #[test]
    fn resume_replaces_a_held_surface_without_leaking_a_second_buffer() {
        let _guard = guard();
        let stale = begin_surface();
        let current = begin_surface();
        assert_eq!(open_surface_count(), 1);
        assert!(!may_present(stale));
        assert!(may_present(current));
        // Ending the retired epoch must not drop the replacement.
        assert!(!end_surface(stale));
        assert!(may_present(current));
        assert!(surface_is_live());
        on_suspended();
        assert!(!may_present(current));
        assert!(!surface_is_live());
        assert_eq!(open_surface_count(), 0);
    }

    #[test]
    fn repeated_suspend_resume_cycles_stay_dark_between_frames() {
        let _guard = guard();
        for _ in 0..5 {
            handle_lifecycle(AppLifecycle::Resumed);
            let epoch = begin_surface();
            assert!(may_present(epoch));
            handle_lifecycle(AppLifecycle::Suspended);
            assert!(!surface_is_live());
            assert!(!may_present(epoch));
            assert_eq!(open_surface_count(), 0);
        }
    }
}
