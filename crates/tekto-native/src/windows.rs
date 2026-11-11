//! Windows text clipboard and sandboxed app-data files.
//!
//! Clipboard uses Win32 directly; files use `ntdll` (`NtCreateFile`).
use crate::NativeError;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Component, Path, PathBuf};

#[link(name = "user32")]
unsafe extern "system" {
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: *mut core::ffi::c_void,
        menu: *mut core::ffi::c_void,
        instance: *mut core::ffi::c_void,
        param: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn DestroyWindow(window: *mut core::ffi::c_void) -> i32;
    fn OpenClipboard(owner: *mut core::ffi::c_void) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn IsClipboardFormatAvailable(format: u32) -> i32;
    fn GetClipboardData(format: u32) -> *mut core::ffi::c_void;
    fn SetClipboardData(format: u32, memory: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> *mut core::ffi::c_void;
    fn GlobalFree(memory: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn GlobalLock(memory: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn GlobalUnlock(memory: *mut core::ffi::c_void) -> i32;
    fn GlobalSize(memory: *mut core::ffi::c_void) -> usize;
}
const CF_UNICODETEXT: u32 = 13;
const GMEM_MOVEABLE: u32 = 2;

struct OpenClipboardGuard(*mut core::ffi::c_void);
impl OpenClipboardGuard {
    fn open(capability: &'static str) -> Result<Self, NativeError> {
        // SAFETY: STATIC is a system window class. A message-only window gives
        // EmptyClipboard a real owner, as required by SetClipboardData.
        unsafe {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                (-3isize) as *mut _,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if window.is_null() {
                return Err(clipboard_error("create owner"));
            }
            if OpenClipboard(window) == 0 {
                DestroyWindow(window);
                return Err(NativeError::Unavailable {
                    capability,
                    platform: "windows",
                    reason: "cannot open clipboard (it may be in use)",
                });
            }
            Ok(Self(window))
        }
    }
}
impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: this guard owns an open clipboard.
        unsafe {
            CloseClipboard();
            DestroyWindow(self.0);
        }
    }
}
struct GlobalLockGuard(*mut core::ffi::c_void);
impl Drop for GlobalLockGuard {
    fn drop(&mut self) {
        // SAFETY: the guard owns one successful GlobalLock call.
        unsafe {
            GlobalUnlock(self.0);
        }
    }
}
fn clipboard_error(action: &str) -> NativeError {
    NativeError::Message(format!(
        "clipboard {action}: {}",
        std::io::Error::last_os_error()
    ))
}

pub(crate) fn get_text() -> Result<String, NativeError> {
    let _clipboard = OpenClipboardGuard::open("clipboard.get")?;
    // SAFETY: clipboard is open; the handle remains valid until it closes.
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return Ok(String::new());
        }
        let memory = GetClipboardData(CF_UNICODETEXT);
        if memory.is_null() {
            return Err(clipboard_error("get"));
        }
        let size = GlobalSize(memory);
        let ptr = GlobalLock(memory).cast::<u16>();
        if ptr.is_null() {
            return Err(clipboard_error("lock"));
        }
        let _lock = GlobalLockGuard(memory);
        let units = std::slice::from_raw_parts(ptr, size / 2);
        let result = match units.iter().position(|&unit| unit == 0) {
            Some(end) => String::from_utf16(&units[..end])
                .map_err(|e| NativeError::Message(format!("clipboard: invalid UTF-16: {e}"))),
            None => Err(NativeError::Message(
                "clipboard: unterminated Unicode text".into(),
            )),
        };
        result
    }
}

pub(crate) fn set_text(text: &str) -> Result<(), NativeError> {
    if text.contains('\0') {
        return Err(NativeError::Message("clipboard: text contains NUL".into()));
    }
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: movable global memory is required by SetClipboardData; it is freed
    // on every failure and becomes owned by the system after a successful set.
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2);
        if memory.is_null() {
            return Err(clipboard_error("allocate"));
        }
        let ptr = GlobalLock(memory).cast::<u16>();
        if ptr.is_null() {
            GlobalFree(memory);
            return Err(clipboard_error("lock"));
        }
        std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
        GlobalUnlock(memory);
        let _clipboard = match OpenClipboardGuard::open("clipboard.set") {
            Ok(guard) => guard,
            Err(err) => {
                GlobalFree(memory);
                return Err(err);
            }
        };
        if EmptyClipboard() == 0 {
            GlobalFree(memory);
            return Err(clipboard_error("empty"));
        }
        if SetClipboardData(CF_UNICODETEXT, memory).is_null() {
            GlobalFree(memory);
            return Err(clipboard_error("set"));
        }
        Ok(())
    }
}

fn fail(reason: String) -> NativeError {
    NativeError::Message(format!("files: {reason}"))
}

/// Relative components only. Rejects `.`, `..`, prefixes, empty segments,
/// NTFS alternate streams, reserved device names, and trailing dots or spaces.
fn relative_parts(path: &str) -> Result<Vec<&str>, NativeError> {
    if path.bytes().any(|b| b == 0) {
        return Err(fail("path contains a NUL byte".into()));
    }
    let parts: Vec<_> = Path::new(path).components().collect();
    if parts.is_empty() || parts.iter().any(|p| !matches!(p, Component::Normal(_))) {
        return Err(fail(
            "path must contain only relative normal components inside the app data directory"
                .into(),
        ));
    }
    let segs: Vec<&str> = path.split(['/', '\\']).collect();
    if segs
        .iter()
        .any(|s| s.is_empty() || *s == "." || *s == ".." || s.ends_with([' ', '.']))
    {
        return Err(fail("invalid relative path".into()));
    }
    if segs.iter().any(|s| {
        let stem = s.split('.').next().unwrap_or(s).trim_end_matches(' ');
        let upper = stem.to_ascii_uppercase();
        let device = matches!(
            upper.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            upper.strip_prefix(prefix).is_some_and(|n| {
                matches!(
                    n,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
        device
            || s.bytes()
                .any(|b| matches!(b, b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|'))
    }) {
        return Err(fail("invalid relative path".into()));
    }
    Ok(segs)
}

/// `%LOCALAPPDATA%\tekto` when that variable is absolute, otherwise
/// `%USERPROFILE%\AppData\Local\tekto`.
fn app_data_dir() -> Result<PathBuf, NativeError> {
    let base = match std::env::var_os("LOCALAPPDATA").filter(|p| Path::new(p).is_absolute()) {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(
            std::env::var_os("USERPROFILE")
                .filter(|p| Path::new(p).is_absolute())
                .ok_or_else(|| {
                    fail("LOCALAPPDATA or USERPROFILE must be an absolute path".into())
                })?,
        )
        .join("AppData")
        .join("Local"),
    }
    .join("tekto");
    std::fs::create_dir_all(&base)
        .map_err(|e| fail(format!("cannot create app data directory: {e}")))?;
    Ok(base)
}

const FILE_LIST_DIRECTORY: u32 = 0x0001;
const FILE_READ_DATA: u32 = 0x0001;
const FILE_WRITE_DATA: u32 = 0x0002;
const FILE_READ_ATTRIBUTES: u32 = 0x0080;
const FILE_TRAVERSE: u32 = 0x0020;
const SYNCHRONIZE: u32 = 0x0010_0000;
const FILE_SHARE_ALL: u32 = 0x0001 | 0x0002 | 0x0004;
const FILE_OPEN: u32 = 0x0000_0001;
const FILE_CREATE: u32 = 0x0000_0002;
const FILE_DIRECTORY_FILE: u32 = 0x0000_0001;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x0000_0020;
const FILE_NON_DIRECTORY_FILE: u32 = 0x0000_0040;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const OBJ_CASE_INSENSITIVE: u32 = 0x0000_0040;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
const FILE_ATTRIBUTE_TAG_INFORMATION: u32 = 35;
const FILE_END_OF_FILE_INFORMATION: u32 = 20;
const STATUS_OBJECT_NAME_NOT_FOUND: i32 = 0xC000_0034_u32 as i32;
const STATUS_OBJECT_NAME_COLLISION: i32 = 0xC000_0035_u32 as i32;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: *mut core::ffi::c_void,
    object_name: *mut UnicodeString,
    attributes: u32,
    security_descriptor: *mut core::ffi::c_void,
    security_quality_of_service: *mut core::ffi::c_void,
}

#[repr(C)]
struct IoStatusBlock {
    status: isize,
    information: usize,
}

#[repr(C)]
struct FileAttributeTagInformation {
    file_attributes: u32,
    reparse_tag: u32,
}

#[repr(C)]
struct FileEndOfFileInformation {
    end_of_file: i64,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        file_handle: *mut *mut core::ffi::c_void,
        desired_access: u32,
        object_attributes: *mut ObjectAttributes,
        io_status_block: *mut IoStatusBlock,
        allocation_size: *mut i64,
        file_attributes: u32,
        share_access: u32,
        create_disposition: u32,
        create_options: u32,
        ea_buffer: *mut core::ffi::c_void,
        ea_length: u32,
    ) -> i32;

    fn NtQueryInformationFile(
        file_handle: *mut core::ffi::c_void,
        io_status_block: *mut IoStatusBlock,
        file_information: *mut core::ffi::c_void,
        length: u32,
        file_information_class: u32,
    ) -> i32;

    fn NtSetInformationFile(
        file_handle: *mut core::ffi::c_void,
        io_status_block: *mut IoStatusBlock,
        file_information: *mut core::ffi::c_void,
        length: u32,
        file_information_class: u32,
    ) -> i32;
}

fn wide(text: &str) -> Result<Vec<u16>, NativeError> {
    let mut units: Vec<u16> = OsStr::new(text).encode_wide().collect();
    units.push(0);
    let bytes = units.len().saturating_sub(1).saturating_mul(2);
    if bytes > u16::MAX as usize {
        return Err(fail("path is too long".into()));
    }
    Ok(units)
}

/// `\??\` form so `NtCreateFile` does not go through Win32 path parsing.
fn nt_root_name(path: &Path) -> Result<Vec<u16>, NativeError> {
    let input: Vec<u16> = path.as_os_str().encode_wide().collect();
    let extended_unc: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
    let extended: Vec<u16> = r"\\?\".encode_utf16().collect();
    let unc: Vec<u16> = r"\\".encode_utf16().collect();
    let (prefix, rest) = if input.starts_with(&extended_unc) {
        (r"\??\UNC\", &input[extended_unc.len()..])
    } else if input.starts_with(&extended) {
        (r"\??\", &input[extended.len()..])
    } else if input.starts_with(&unc) {
        (r"\??\UNC\", &input[unc.len()..])
    } else {
        (r"\??\", input.as_slice())
    };
    let mut units: Vec<u16> = prefix.encode_utf16().chain(rest.iter().copied()).collect();
    if units.len().saturating_mul(2) > u16::MAX as usize {
        return Err(fail("path is too long".into()));
    }
    units.push(0);
    Ok(units)
}

enum NtOpenError {
    NotFound,
    Collision,
    Other(NativeError),
}

fn nt_status(status: i32, what: &str) -> Result<(), NtOpenError> {
    if status >= 0 {
        Ok(())
    } else if status == STATUS_OBJECT_NAME_NOT_FOUND {
        Err(NtOpenError::NotFound)
    } else if status == STATUS_OBJECT_NAME_COLLISION {
        Err(NtOpenError::Collision)
    } else {
        Err(NtOpenError::Other(fail(format!(
            "{what} failed: NTSTATUS {status:#010x}"
        ))))
    }
}

fn nt_create(
    root: Option<&OwnedHandle>,
    mut name: Vec<u16>,
    access: u32,
    disposition: u32,
    options: u32,
) -> Result<OwnedHandle, NtOpenError> {
    let byte_len = (name.len() - 1) * 2;
    // SAFETY: zero is valid for all fields, including pointers. Initialize the
    // backing storage before passing these C-layout structs to ntdll.
    let mut unicode: UnicodeString = unsafe { std::mem::zeroed() };
    unicode.length = byte_len as u16;
    unicode.maximum_length = byte_len as u16;
    unicode.buffer = name.as_mut_ptr();
    // SAFETY: zero is valid for all fields.
    let mut attributes: ObjectAttributes = unsafe { std::mem::zeroed() };
    attributes.length = std::mem::size_of::<ObjectAttributes>() as u32;
    attributes.root_directory = root
        .map(|h| h.as_raw_handle())
        .unwrap_or(std::ptr::null_mut());
    attributes.object_name = &mut unicode;
    attributes.attributes = OBJ_CASE_INSENSITIVE;
    let mut io = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut handle = std::ptr::null_mut();
    // SAFETY: name and the structs stay alive for the call. A non-negative
    // status transfers ownership of `handle` to the caller.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &mut attributes,
            &mut io,
            std::ptr::null_mut(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_ALL,
            disposition,
            options,
            std::ptr::null_mut(),
            0,
        )
    };
    nt_status(status, "NtCreateFile")?;
    if handle.is_null() {
        return Err(NtOpenError::Other(fail(
            "NtCreateFile returned a null handle".into(),
        )));
    }
    // SAFETY: ntdll returned an owned handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn ensure_not_reparse(handle: &OwnedHandle) -> Result<(), NativeError> {
    let mut info = FileAttributeTagInformation {
        file_attributes: 0,
        reparse_tag: 0,
    };
    let mut io = IoStatusBlock {
        status: 0,
        information: 0,
    };
    // SAFETY: `handle` is open and `info` outlives the query.
    let status = unsafe {
        NtQueryInformationFile(
            handle.as_raw_handle(),
            &mut io,
            (&mut info as *mut FileAttributeTagInformation).cast(),
            std::mem::size_of::<FileAttributeTagInformation>() as u32,
            FILE_ATTRIBUTE_TAG_INFORMATION,
        )
    };
    if status < 0 {
        return Err(fail(format!(
            "cannot query file attributes: NTSTATUS {status:#010x}"
        )));
    }
    if info.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(fail(format!(
            "refusing reparse point (junction, symlink, or mount): tag {:#010x}",
            info.reparse_tag
        )));
    }
    Ok(())
}

fn truncate(handle: &OwnedHandle) -> Result<(), NativeError> {
    let mut end = FileEndOfFileInformation { end_of_file: 0 };
    let mut io = IoStatusBlock {
        status: 0,
        information: 0,
    };
    // SAFETY: `handle` is open for write and `end` outlives the call.
    let status = unsafe {
        NtSetInformationFile(
            handle.as_raw_handle(),
            &mut io,
            (&mut end as *mut FileEndOfFileInformation).cast(),
            std::mem::size_of::<FileEndOfFileInformation>() as u32,
            FILE_END_OF_FILE_INFORMATION,
        )
    };
    if status < 0 {
        return Err(fail(format!("cannot truncate: NTSTATUS {status:#010x}")));
    }
    Ok(())
}

fn open_root() -> Result<OwnedHandle, NativeError> {
    let dir = app_data_dir()?;
    let name = nt_root_name(&dir)?;
    let handle = nt_create(
        None,
        name,
        FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
    )
    .map_err(|e| match e {
        NtOpenError::Other(err) => err,
        NtOpenError::NotFound | NtOpenError::Collision => {
            fail("cannot open app data directory".into())
        }
    })?;
    ensure_not_reparse(&handle)?;
    Ok(handle)
}

/// Open `path` under the app-data root. Each component is opened relative to
/// the previous handle with `FILE_OPEN_REPARSE_POINT` and rejected if it is a
/// reparse point, so a junction cannot retarget later components.
fn file_at(path: &str, write: bool) -> Result<std::fs::File, NativeError> {
    let parts = relative_parts(path)?;
    let (file_name, parents) = parts.split_last().expect("relative path is non-empty");
    let mut dir = open_root()?;
    for part in parents {
        let child = nt_create(
            Some(&dir),
            wide(part)?,
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
        )
        .map_err(|e| match e {
            NtOpenError::Other(err) => err,
            NtOpenError::NotFound | NtOpenError::Collision => fail(format!("cannot open {path:?}")),
        })?;
        ensure_not_reparse(&child)?;
        dir = child;
    }
    let name = wide(file_name)?;
    let file_opts =
        FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT;
    if !write {
        let file = nt_create(
            Some(&dir),
            name,
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            file_opts,
        )
        .map_err(|e| match e {
            NtOpenError::Other(err) => err,
            NtOpenError::NotFound | NtOpenError::Collision => fail(format!("cannot read {path:?}")),
        })?;
        ensure_not_reparse(&file)?;
        return Ok(std::fs::File::from(file));
    }
    let write_access = FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
    match nt_create(Some(&dir), name.clone(), write_access, FILE_OPEN, file_opts) {
        Ok(file) => {
            ensure_not_reparse(&file)?;
            truncate(&file)?;
            Ok(std::fs::File::from(file))
        }
        Err(NtOpenError::NotFound) => {
            match nt_create(Some(&dir), name, write_access, FILE_CREATE, file_opts) {
                Ok(file) => {
                    ensure_not_reparse(&file)?;
                    Ok(std::fs::File::from(file))
                }
                Err(NtOpenError::Collision) => Err(fail(format!(
                    "refusing to replace an existing name at {path:?}"
                ))),
                Err(NtOpenError::NotFound) => Err(fail(format!("cannot write {path:?}"))),
                Err(NtOpenError::Other(err)) => Err(err),
            }
        }
        Err(NtOpenError::Collision) => Err(fail(format!("cannot write {path:?}"))),
        Err(NtOpenError::Other(err)) => Err(err),
    }
}

pub(crate) fn read_file(path: &str) -> Result<Vec<u8>, NativeError> {
    use std::io::Read;
    let mut bytes = Vec::new();
    file_at(path, false)?
        .read_to_end(&mut bytes)
        .map_err(|e| NativeError::Message(format!("files.read {path:?}: {e}")))?;
    Ok(bytes)
}

pub(crate) fn write_file(path: &str, bytes: &[u8]) -> Result<(), NativeError> {
    use std::io::Write;
    file_at(path, true)?
        .write_all(bytes)
        .map_err(|e| NativeError::Message(format!("files.write {path:?}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::relative_parts;

    #[test]
    fn rejects_paths_that_leave_the_app_data_directory() {
        for path in [
            r"..\escape",
            r"a\..\escape",
            r"\tmp\escape",
            r"C:\escape",
            r"\\server\share",
            r".\hello",
            "a:",
            "note.txt:stream",
            "trail.",
            "trail ",
            "CON",
            "folder/nul.txt",
            "com1.log",
            "LPT²",
            "",
        ] {
            assert!(relative_parts(path).is_err(), "{path}");
        }
        assert_eq!(relative_parts(r"notes\a.txt").unwrap(), ["notes", "a.txt"]);
        assert!(relative_parts("COM10.txt").is_ok());
    }

    #[test]
    fn file_roundtrip_in_isolated_app_data() {
        use crate::{native, Files};
        if std::env::var_os("TEKTO_WINDOWS_FILES_CHILD").is_none() {
            let dir =
                std::env::temp_dir().join(format!("tekto-windows-files-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "windows::tests::file_roundtrip_in_isolated_app_data",
                    "--exact",
                ])
                .env("LOCALAPPDATA", &dir)
                .env("TEKTO_WINDOWS_FILES_CHILD", "1")
                .output()
                .unwrap();
            std::fs::remove_dir_all(&dir).unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("1 passed"),
                "child did not run the expected test: {}",
                String::from_utf8_lossy(&output.stdout)
            );
            return;
        }
        let host = native();
        host.write_string("unicode.txt", "λ 漢字 😀").unwrap();
        assert_eq!(host.read_to_string("unicode.txt").unwrap(), "λ 漢字 😀");
        host.write_string("unicode.txt", "x").unwrap();
        assert_eq!(host.read_to_string("unicode.txt").unwrap(), "x");
        host.write_bytes("bytes", &[0, 128, 255]).unwrap();
        assert_eq!(host.read_bytes("bytes").unwrap(), [0, 128, 255]);
        assert!(host.read_to_string("bytes").is_err());
        assert!(host.read_bytes("missing").is_err());
        assert!(host.write_string("missing/child", "x").is_err());
        assert!(host.write_string("../escape", "x").is_err());
    }
}
