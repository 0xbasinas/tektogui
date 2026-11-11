//! Windows notification-area icon (`Shell_NotifyIconW`).
//!
//! Compiled only with the `tray` feature. No extra crate. A hidden
//! message-only window owns the icon. `Shell_NotifyIconW` adds, updates,
//! and removes it. A left or right click opens a simple popup menu
//! (`CreatePopupMenu` / `TrackPopupMenu`). The shell rejecting the icon is
//! [`NativeError::Unavailable`]. This module does not open a display.
//!
//! There is no Windows runtime test here. `cargo check --target
//! x86_64-pc-windows-gnu` only type-checks the FFI.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use crate::{NativeError, TrayMenuItem};

const SHOW: &str = "tray.show";
const REASON: &str = "Windows shell notification area is unavailable";
const MAX_ITEMS: usize = 16;
const TIP_UNITS: usize = 127;
const WAIT: Duration = Duration::from_secs(2);

const WM_APP: u32 = 0x8000;
const WM_TRAY_CMD: u32 = WM_APP + 1;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_CONTEXTMENU: u32 = 0x007B;
const WM_NULL: u32 = 0;
const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIF_MESSAGE: u32 = 0x1;
const NIF_ICON: u32 = 0x2;
const NIF_TIP: u32 = 0x4;
const MF_STRING: u32 = 0;
const TPM_RIGHTBUTTON: u32 = 0x0002;
const TPM_NONOTIFY: u32 = 0x0080;
const TPM_RETURNCMD: u32 = 0x0100;
const IDI_APPLICATION: usize = 32512;

type Handle = *mut c_void;

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[repr(C)]
struct NotifyIconData {
    size: u32,
    window: Handle,
    id: u32,
    flags: u32,
    callback: u32,
    icon: Handle,
    tip: [u16; 128],
    state: u32,
    state_mask: u32,
    info: [u16; 256],
    timeout_or_version: u32,
    info_title: [u16; 64],
    info_flags: u32,
    guid: Guid,
    balloon_icon: Handle,
}

#[repr(C)]
struct WndClass {
    style: u32,
    wnd_proc: Option<unsafe extern "system" fn(Handle, u32, usize, isize) -> isize>,
    class_extra: i32,
    window_extra: i32,
    instance: Handle,
    icon: Handle,
    cursor: Handle,
    background: Handle,
    menu_name: *const u16,
    class_name: *const u16,
}

#[repr(C)]
struct Msg {
    window: Handle,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    pt_x: i32,
    pt_y: i32,
    private: u32,
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassW(class: *const WndClass) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        param: Handle,
    ) -> Handle;
    fn DefWindowProcW(window: Handle, message: u32, wparam: usize, lparam: isize) -> isize;
    fn GetMessageW(msg: *mut Msg, window: Handle, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> isize;
    fn PostMessageW(window: Handle, message: u32, wparam: usize, lparam: isize) -> i32;
    fn DestroyWindow(window: Handle) -> i32;
    fn LoadIconW(instance: Handle, name: *const u16) -> Handle;
    fn CreatePopupMenu() -> Handle;
    fn DestroyMenu(menu: Handle) -> i32;
    fn AppendMenuW(menu: Handle, flags: u32, id: usize, text: *const u16) -> i32;
    fn TrackPopupMenu(
        menu: Handle,
        flags: u32,
        x: i32,
        y: i32,
        reserved: i32,
        window: Handle,
        rect: *const c_void,
    ) -> i32;
    fn GetCursorPos(point: *mut Point) -> i32;
    fn SetForegroundWindow(window: Handle) -> i32;
    fn GetCurrentThreadId() -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
}

#[link(name = "shell32")]
extern "system" {
    fn Shell_NotifyIconW(message: u32, data: *const NotifyIconData) -> i32;
}

enum Job {
    Show,
    Hide,
    Relabel,
}

struct Inner {
    label: String,
    items: Vec<TrayMenuItem>,
    clicks: VecDeque<String>,
    on_click: Option<fn(&str)>,
    shown: bool,
    /// HWND bits. Stored as `usize` so the mutex stays `Send`.
    window: usize,
    thread_id: u32,
    starting: bool,
    failed: bool,
    ready: bool,
    busy: bool,
    job: Option<Job>,
    job_done: bool,
    job_ok: bool,
}

struct Tray {
    inner: Mutex<Inner>,
    cv: Condvar,
}

fn tray() -> &'static Tray {
    static TRAY: OnceLock<Tray> = OnceLock::new();
    TRAY.get_or_init(|| Tray {
        inner: Mutex::new(Inner {
            label: String::new(),
            items: Vec::new(),
            clicks: VecDeque::new(),
            on_click: None,
            shown: false,
            window: 0,
            thread_id: 0,
            starting: false,
            failed: false,
            ready: false,
            busy: false,
            job: None,
            job_done: false,
            job_ok: false,
        }),
        cv: Condvar::new(),
    })
}

fn lock() -> std::sync::MutexGuard<'static, Inner> {
    tray()
        .inner
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn unavailable() -> NativeError {
    NativeError::Unavailable {
        capability: SHOW,
        platform: "windows",
        reason: REASON,
    }
}

fn reject_nul(text: &str, capability: &str) -> Result<(), NativeError> {
    if text.contains('\0') {
        Err(NativeError::Message(format!(
            "{capability}: text contains a NUL byte"
        )))
    } else {
        Ok(())
    }
}

pub(crate) fn check_menu(label: &str, menu: &[TrayMenuItem]) -> Result<(), NativeError> {
    reject_nul(label, SHOW)?;
    if label.encode_utf16().count() > TIP_UNITS {
        return Err(NativeError::Message(format!(
            "{SHOW}: label must fit in {TIP_UNITS} UTF-16 units"
        )));
    }
    if menu.len() > MAX_ITEMS {
        return Err(NativeError::Message(format!(
            "{SHOW}: menu has more than {MAX_ITEMS} items"
        )));
    }
    let mut seen = Vec::new();
    for item in menu {
        reject_nul(&item.id, SHOW)?;
        reject_nul(&item.label, SHOW)?;
        if item.id.is_empty() {
            return Err(NativeError::Message(format!("{SHOW}: menu id is empty")));
        }
        if seen.iter().any(|id| id == &item.id) {
            return Err(NativeError::Message(format!(
                "{SHOW}: duplicate menu id {}",
                item.id
            )));
        }
        seen.push(item.id.as_str());
    }
    Ok(())
}

fn class_name() -> Vec<u16> {
    "TektoTray\0".encode_utf16().collect()
}

fn tip(label: &str) -> [u16; 128] {
    let mut out = [0u16; 128];
    for (slot, unit) in out.iter_mut().zip(label.encode_utf16()) {
        *slot = unit;
    }
    out
}

fn icon_data(window: Handle, label: &str) -> NotifyIconData {
    // SAFETY: zeroed is a valid empty NOTIFYICONDATAW; we fill the fields we use.
    let mut data: NotifyIconData = unsafe { std::mem::zeroed() };
    data.size = std::mem::size_of::<NotifyIconData>() as u32;
    data.window = window;
    data.id = 1;
    data.flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.callback = WM_APP;
    data.tip = tip(label);
    // SAFETY: LoadIconW with a system icon id does not need a module.
    data.icon = unsafe { LoadIconW(ptr::null_mut(), IDI_APPLICATION as *const u16) };
    data
}

fn shell(message: u32, window: Handle, label: &str) -> bool {
    let data = icon_data(window, label);
    if data.icon.is_null() {
        return false;
    }
    // SAFETY: `data` is a complete NOTIFYICONDATAW for this window and icon id.
    unsafe { Shell_NotifyIconW(message, &data) != 0 }
}

fn open_menu(window: Handle) {
    let items = lock().items.clone();
    if items.is_empty() {
        return;
    }
    // SAFETY: the menu is created and destroyed on this call. Item strings
    // live until AppendMenuW returns.
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        for (index, item) in items.iter().enumerate() {
            let mut wide: Vec<u16> = item.label.encode_utf16().collect();
            wide.push(0);
            AppendMenuW(menu, MF_STRING, index + 1, wide.as_ptr());
        }
        let mut point = Point { x: 0, y: 0 };
        GetCursorPos(&mut point);
        SetForegroundWindow(window);
        let picked = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_NONOTIFY | TPM_RETURNCMD,
            point.x,
            point.y,
            0,
            window,
            ptr::null(),
        );
        PostMessageW(window, WM_NULL, 0, 0);
        DestroyMenu(menu);
        if picked > 0 {
            let index = (picked as usize) - 1;
            if let Some(item) = items.get(index) {
                record_click(item.id.clone());
            }
        }
    }
}

fn record_click(id: String) {
    let hook = {
        let mut state = lock();
        state.clicks.push_back(id.clone());
        state.on_click
    };
    if let Some(hook) = hook {
        hook(&id);
    }
}

fn run_job(window: Handle) {
    let (job, label) = {
        let mut state = lock();
        (state.job.take(), state.label.clone())
    };
    let ok = match job {
        Some(Job::Show) => {
            let added = if lock().shown {
                shell(NIM_MODIFY, window, &label) || shell(NIM_ADD, window, &label)
            } else {
                shell(NIM_ADD, window, &label) || {
                    shell(NIM_DELETE, window, &label);
                    shell(NIM_ADD, window, &label)
                }
            };
            lock().shown = added;
            added
        }
        Some(Job::Hide) => {
            if lock().shown {
                shell(NIM_DELETE, window, &label);
            }
            lock().shown = false;
            true
        }
        Some(Job::Relabel) if lock().shown => shell(NIM_MODIFY, window, &label),
        Some(Job::Relabel) => true,
        None => true,
    };
    let mut state = lock();
    state.job_ok = ok;
    state.job_done = true;
    tray().cv.notify_all();
}

unsafe extern "system" fn wnd_proc(
    window: Handle,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    match message {
        WM_TRAY_CMD => {
            run_job(window);
            0
        }
        WM_APP => {
            let event = lparam as u32;
            if matches!(
                event,
                WM_LBUTTONUP | WM_RBUTTONUP | WM_LBUTTONDBLCLK | WM_CONTEXTMENU
            ) {
                open_menu(window);
            }
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

fn pump() {
    let class = class_name();
    // SAFETY: class name is NUL-terminated and outlives RegisterClassW.
    // WS_POPUP with a zero size is not shown, so the user never sees a window.
    // A real HWND owns the icon; a message-only window is a poor Shell_NotifyIcon owner.
    let window = unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let class = WndClass {
            style: 0,
            wnd_proc: Some(wnd_proc),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: ptr::null_mut(),
            cursor: ptr::null_mut(),
            background: ptr::null_mut(),
            menu_name: ptr::null(),
            class_name: class.as_ptr(),
        };
        RegisterClassW(&class);
        CreateWindowExW(
            0,
            class.class_name,
            class.class_name,
            0x8000_0000, // WS_POPUP
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null_mut(),
        )
    };
    {
        let mut state = lock();
        state.thread_id = unsafe { GetCurrentThreadId() };
        if window.is_null() {
            state.failed = true;
        } else {
            state.window = window as usize;
        }
        state.ready = true;
        tray().cv.notify_all();
    }
    if window.is_null() {
        return;
    }
    let mut msg = Msg {
        window: ptr::null_mut(),
        message: 0,
        wparam: 0,
        lparam: 0,
        time: 0,
        pt_x: 0,
        pt_y: 0,
        private: 0,
    };
    // SAFETY: `msg` is a full MSG. The loop ends when the window is destroyed.
    while unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    unsafe { DestroyWindow(window) };
}

fn ensure_thread() -> Result<(), NativeError> {
    let spawn = {
        let mut state = lock();
        if state.ready && !state.failed && state.window != 0 {
            return Ok(());
        }
        if state.failed {
            return Err(unavailable());
        }
        if state.starting {
            false
        } else {
            state.starting = true;
            true
        }
    };
    if spawn
        && thread::Builder::new()
            .name("tekto-tray".into())
            .spawn(pump)
            .is_err()
    {
        let mut state = lock();
        state.failed = true;
        state.ready = true;
        tray().cv.notify_all();
        return Err(unavailable());
    }
    let state = lock();
    let (state, _) = tray()
        .cv
        .wait_timeout_while(state, WAIT, |state| !state.ready)
        .unwrap_or_else(|poison| poison.into_inner());
    if state.ready && !state.failed && state.window != 0 {
        Ok(())
    } else {
        Err(unavailable())
    }
}

fn on_tray_thread() -> bool {
    let id = lock().thread_id;
    id != 0 && id == unsafe { GetCurrentThreadId() }
}

fn submit(job: Job) -> Result<(), NativeError> {
    ensure_thread()?;
    if on_tray_thread() {
        {
            let mut state = lock();
            state.job = Some(job);
        }
        let window = lock().window as Handle;
        run_job(window);
        return if lock().job_ok {
            Ok(())
        } else {
            Err(unavailable())
        };
    }
    let window = {
        let state = lock();
        let (mut state, wait) = tray()
            .cv
            .wait_timeout_while(state, WAIT, |state| state.busy)
            .unwrap_or_else(|poison| poison.into_inner());
        if wait.timed_out() {
            return Err(unavailable());
        }
        state.busy = true;
        state.job = Some(job);
        state.job_done = false;
        state.job_ok = false;
        state.window as Handle
    };
    // SAFETY: `window` is the hidden tray window on the tray thread.
    if unsafe { PostMessageW(window, WM_TRAY_CMD, 0, 0) } == 0 {
        let mut state = lock();
        state.busy = false;
        tray().cv.notify_all();
        return Err(unavailable());
    }
    let state = lock();
    let (mut state, wait) = tray()
        .cv
        .wait_timeout_while(state, WAIT, |state| !state.job_done)
        .unwrap_or_else(|poison| poison.into_inner());
    let ok = state.job_ok && !wait.timed_out();
    state.busy = false;
    tray().cv.notify_all();
    if ok {
        Ok(())
    } else {
        Err(unavailable())
    }
}

pub(crate) fn set_label(label: &str) -> Result<(), NativeError> {
    reject_nul(label, "tray.set_label")?;
    if label.encode_utf16().count() > TIP_UNITS {
        return Err(NativeError::Message(format!(
            "tray.set_label: label must fit in {TIP_UNITS} UTF-16 units"
        )));
    }
    lock().label = label.to_string();
    if lock().shown {
        submit(Job::Relabel)?;
    }
    Ok(())
}

pub(crate) fn set_on_click(on_click: Option<fn(&str)>) -> Result<(), NativeError> {
    lock().on_click = on_click;
    Ok(())
}

pub(crate) fn take_click() -> Result<Option<String>, NativeError> {
    Ok(lock().clicks.pop_front())
}

pub(crate) fn hide_icon() -> Result<(), NativeError> {
    let started = {
        let state = lock();
        state.shown || state.window != 0
    };
    if started {
        submit(Job::Hide)?;
    }
    Ok(())
}

pub(crate) fn show_icon(label: &str, menu: &[TrayMenuItem]) -> Result<(), NativeError> {
    check_menu(label, menu)?;
    {
        let mut state = lock();
        state.label = label.to_string();
        state.items = menu.to_vec();
    }
    submit(Job::Show)
}

#[cfg(test)]
mod tests {
    use super::{check_menu, TIP_UNITS};
    use crate::TrayMenuItem;

    fn item(id: &str, label: &str) -> TrayMenuItem {
        TrayMenuItem {
            id: id.into(),
            label: label.into(),
        }
    }

    #[test]
    fn menu_rejects_nul_empty_duplicate_and_long_tip() {
        assert!(check_menu("Tekto", &[item("quit", "Quit")]).is_ok());
        assert!(check_menu("a\0b", &[]).is_err());
        assert!(check_menu("Tekto", &[item("", "Quit")]).is_err());
        assert!(check_menu("Tekto", &[item("a\0", "Quit")]).is_err());
        assert!(check_menu("Tekto", &[item("quit", "Q\0")]).is_err());
        assert!(check_menu("Tekto", &[item("quit", "Quit"), item("quit", "Again")]).is_err());
        let long = "x".repeat(TIP_UNITS + 1);
        assert!(check_menu(&long, &[]).is_err());
        assert!(check_menu(&"x".repeat(TIP_UNITS), &[]).is_ok());
    }
}
