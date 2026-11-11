//! Windows open and save dialogs via `GetOpenFileNameW` / `GetSaveFileNameW`
//! (`comdlg32`). No extra crate. Cancel is `Ok(None)`. A session that cannot
//! show the dialog is [`NativeError::Unavailable`].
//!
//! There is no Windows runtime in CI. `cargo check --target
//! x86_64-pc-windows-gnu` only type-checks this file.
use crate::NativeError;

fn unavailable(capability: &'static str) -> NativeError {
    NativeError::Unavailable {
        capability,
        platform: "windows",
        reason: "Windows common file dialog is unavailable",
    }
}

fn bad_path(capability: &'static str) -> NativeError {
    NativeError::Unavailable {
        capability,
        platform: "windows",
        reason: "file dialog did not return an absolute path",
    }
}

/// Drive path (`C:\...`) or UNC (`\\server\share\...`), including `\\?\` forms.
pub(crate) fn is_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    let rest = path.strip_prefix(r"\\").or_else(|| path.strip_prefix("//"));
    let Some(rest) = rest else {
        return false;
    };
    let Some((server, share)) = rest.split_once(['\\', '/']) else {
        return false;
    };
    !server.is_empty() && !share.is_empty()
}

/// Map a common-dialog result. `success` is the BOOL from `GetOpenFileNameW`
/// or `GetSaveFileNameW`. `error` is `CommDlgExtendedError` (0 means cancel).
pub(crate) fn interpret(
    success: bool,
    error: u32,
    file: &[u16],
    capability: &'static str,
) -> Result<Option<String>, NativeError> {
    if !success {
        return if error == 0 {
            Ok(None)
        } else {
            Err(unavailable(capability))
        };
    }
    let end = file.iter().position(|u| *u == 0).unwrap_or(file.len());
    let path = String::from_utf16(&file[..end]).map_err(|_| {
        NativeError::Message("file dialog returned a path that is not Unicode".into())
    })?;
    if path.is_empty() || !is_windows_absolute(&path) {
        return Err(bad_path(capability));
    }
    Ok(Some(path))
}

#[cfg(target_os = "windows")]
mod ffi {
    use super::interpret;
    use crate::NativeError;

    const PATH_CHARS: usize = 32 * 1024;
    use std::ffi::c_void;

    #[repr(C)]
    struct OpenFileNameW {
        struct_size: u32,
        hwnd_owner: *mut c_void,
        instance: *mut c_void,
        filter: *const u16,
        custom_filter: *mut u16,
        max_custom_filter: u32,
        filter_index: u32,
        file: *mut u16,
        max_file: u32,
        file_title: *mut u16,
        max_file_title: u32,
        initial_dir: *const u16,
        title: *const u16,
        flags: u32,
        file_offset: u16,
        file_extension: u16,
        def_ext: *const u16,
        cust_data: isize,
        hook: *mut c_void,
        template_name: *const u16,
        reserved: *mut c_void,
        reserved_flags: u32,
        flags_ex: u32,
    }

    #[link(name = "comdlg32")]
    unsafe extern "system" {
        fn GetOpenFileNameW(ofn: *mut OpenFileNameW) -> i32;
        fn GetSaveFileNameW(ofn: *mut OpenFileNameW) -> i32;
        fn CommDlgExtendedError() -> u32;
    }

    const OFN_OVERWRITEPROMPT: u32 = 0x0000_0002;
    const OFN_HIDEREADONLY: u32 = 0x0000_0004;
    const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
    const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
    const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;
    const OFN_EXPLORER: u32 = 0x0008_0000;

    pub(crate) fn show(save: bool) -> Result<Option<String>, NativeError> {
        let capability = if save {
            "files.save_dialog"
        } else {
            "files.open_dialog"
        };
        let filter: Vec<u16> = "All files\0*.*\0\0".encode_utf16().collect();
        let mut file = vec![0u16; PATH_CHARS];
        let mut ofn = OpenFileNameW {
            struct_size: std::mem::size_of::<OpenFileNameW>() as u32,
            hwnd_owner: std::ptr::null_mut(),
            instance: std::ptr::null_mut(),
            filter: filter.as_ptr(),
            custom_filter: std::ptr::null_mut(),
            max_custom_filter: 0,
            filter_index: 1,
            file: file.as_mut_ptr(),
            max_file: PATH_CHARS as u32,
            file_title: std::ptr::null_mut(),
            max_file_title: 0,
            initial_dir: std::ptr::null_mut(),
            title: std::ptr::null_mut(),
            flags: OFN_EXPLORER | OFN_HIDEREADONLY | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST,
            file_offset: 0,
            file_extension: 0,
            def_ext: std::ptr::null_mut(),
            cust_data: 0,
            hook: std::ptr::null_mut(),
            template_name: std::ptr::null_mut(),
            reserved: std::ptr::null_mut(),
            reserved_flags: 0,
            flags_ex: 0,
        };
        if save {
            ofn.flags |= OFN_OVERWRITEPROMPT;
        } else {
            ofn.flags |= OFN_FILEMUSTEXIST;
        }
        // SAFETY: `ofn` and `file` stay alive for the call. The dialog writes
        // a NUL-terminated path into `file` and does not keep the pointers.
        let success = unsafe {
            if save {
                GetSaveFileNameW(&mut ofn)
            } else {
                GetOpenFileNameW(&mut ofn)
            }
        };
        let error = if success == 0 {
            // SAFETY: no arguments; reads the last dialog error on this thread.
            unsafe { CommDlgExtendedError() }
        } else {
            0
        };
        interpret(success != 0, error, &file, capability)
    }
}

#[cfg(target_os = "windows")]
pub(crate) use ffi::show;

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    #[test]
    fn cancel_is_none_and_failure_is_unavailable() {
        let cap = "files.open_dialog";
        assert_eq!(interpret(false, 0, &[], cap).unwrap(), None);
        assert!(matches!(
            interpret(false, 0xFFFF, &wide(r"C:\a.txt"), cap),
            Err(NativeError::Unavailable {
                capability: "files.open_dialog",
                reason: "Windows common file dialog is unavailable",
                ..
            })
        ));
    }

    #[test]
    fn accepts_absolute_paths_only() {
        let cap = "files.save_dialog";
        assert_eq!(
            interpret(true, 0, &wide(r"C:\notes\a.txt"), cap).unwrap(),
            Some(r"C:\notes\a.txt".into())
        );
        assert_eq!(
            interpret(true, 0, &wide(r"\\server\share\a.txt"), cap).unwrap(),
            Some(r"\\server\share\a.txt".into())
        );
        assert!(is_windows_absolute(r"\\?\C:\notes\a.txt"));
        for path in [
            "notes.txt",
            r"C:notes.txt",
            r"\notes\a.txt",
            r"\\server",
            "",
        ] {
            assert!(
                matches!(
                    interpret(true, 0, &wide(path), cap),
                    Err(NativeError::Unavailable {
                        reason: "file dialog did not return an absolute path",
                        ..
                    })
                ),
                "{path}"
            );
        }
        let lone = [0xD800u16, 0];
        assert!(matches!(
            interpret(true, 0, &lone, cap),
            Err(NativeError::Message(_))
        ));
    }
}
