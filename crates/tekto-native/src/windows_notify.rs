//! Windows Shell_NotifyIconW balloon notifications (Windows Vista and later).
use crate::NativeError;
use std::ffi::c_void;
use std::ptr;
use std::sync::mpsc;
use std::time::Duration;

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

#[link(name = "user32")]
extern "system" {
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
    fn DestroyWindow(window: Handle) -> i32;
    fn LoadIconW(instance: Handle, name: *const u16) -> Handle;
}
#[link(name = "shell32")]
extern "system" {
    fn Shell_NotifyIconW(message: u32, data: *const NotifyIconData) -> i32;
}

fn text<const N: usize>(value: &str, field: &str) -> Result<[u16; N], NativeError> {
    let encoded: Vec<u16> = value.encode_utf16().collect();
    if encoded.contains(&0) || encoded.len() >= N {
        return Err(NativeError::Message(format!(
            "notifications.show: {field} must contain no NUL and fit in {} UTF-16 units",
            N - 1
        )));
    }
    let mut result = [0; N];
    result[..encoded.len()].copy_from_slice(&encoded);
    Ok(result)
}

fn unavailable() -> NativeError {
    NativeError::Unavailable {
        capability: "notifications.show",
        platform: "windows",
        reason: "Windows shell notification area is unavailable",
    }
}

pub(crate) fn show(title: &str, body: &str) -> Result<(), NativeError> {
    let title = text(title, "title")?;
    let body = text(body, "body")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("tekto-notification".into())
        .spawn(move || {
            // A hidden top-level window owns the icon until the balloon has time to display.
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            // SAFETY: pointers are valid for the calls; the window is destroyed on this thread.
            unsafe {
                let window = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                if window.is_null() {
                    let _ = sender.send(false);
                    return;
                }
                let mut data: NotifyIconData = std::mem::zeroed();
                data.size = std::mem::size_of::<NotifyIconData>() as u32;
                data.window = window;
                data.id = 1;
                data.icon = LoadIconW(ptr::null_mut(), 32512usize as *const u16); // IDI_APPLICATION
                if data.icon.is_null() {
                    let _ = sender.send(false);
                    DestroyWindow(window);
                    return;
                }
                data.flags = 0x12; // NIF_ICON | NIF_INFO
                data.info = body;
                data.info_title = title;
                data.info_flags = 1; // NIIF_INFO
                let added = Shell_NotifyIconW(0, &data) != 0; // NIM_ADD
                let _ = sender.send(added);
                if added {
                    std::thread::sleep(Duration::from_secs(30));
                    Shell_NotifyIconW(2, &data); // NIM_DELETE
                }
                DestroyWindow(window);
            }
        })
        .map_err(|_| unavailable())?;
    if receiver.recv().unwrap_or(false) {
        Ok(())
    } else {
        Err(unavailable())
    }
}

#[cfg(test)]
mod tests {
    use super::text;

    #[test]
    fn notification_text_checks_utf16_limits_and_nul() {
        assert!(text::<4>("abc", "title").is_ok());
        assert!(text::<4>("abcd", "title").is_err());
        assert!(text::<4>("a\0b", "title").is_err());
        assert!(text::<4>("😀😀", "title").is_err());
    }
}
