//! Windows text clipboard and sandboxed app-data files.
//!
//! Compile-checked on Linux for `x86_64-pc-windows-gnu`. Clipboard and file
//! behavior still need a Windows machine. No new crates: clipboard uses
//! `arboard` (MIT OR Apache-2.0); files use `ntdll` (`NtCreateFile`).
use crate::NativeError;
use arboard::{Clipboard, Error};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Component, Path, PathBuf};

fn map_error(capability: &'static str, err: Error) -> NativeError {
    match err {
        Error::ClipboardNotSupported => NativeError::Unavailable {
            capability,
            platform: "windows",
            reason: "clipboard is not available in this session",
        },
        other => NativeError::Message(format!("{capability}: {other}")),
    }
}

pub(crate) fn get_text() -> Result<String, NativeError> {
    let mut clipboard = Clipboard::new().map_err(|e| map_error("clipboard.get", e))?;
    match clipboard.get_text() {
        Ok(text) => Ok(text),
        Err(Error::ContentNotAvailable) => Ok(String::new()),
        Err(e) => Err(map_error("clipboard.get", e)),
    }
}

pub(crate) fn set_text(text: &str) -> Result<(), NativeError> {
    Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .map_err(|e| map_error("clipboard.set", e))
}

fn fail(reason: String) -> NativeError {
    NativeError::Message(format!("files: {reason}"))
}

/// Relative components only. Rejects `.`, `..`, prefixes, empty segments,
/// NTFS alternate streams (`:`), and trailing dots or spaces (Win32 strips those).
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
        s.bytes()
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
const FILE_APPEND_DATA: u32 = 0x0004;
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
    let display = path.to_string_lossy();
    let nt = if let Some(rest) = display.strip_prefix(r"\\?\UNC\") {
        format!(r"\??\UNC\{rest}")
    } else if let Some(rest) = display.strip_prefix(r"\\?\") {
        format!(r"\??\{rest}")
    } else if let Some(rest) = display.strip_prefix(r"\\") {
        format!(r"\??\UNC\{rest}")
    } else {
        format!(r"\??\{display}")
    };
    wide(&nt)
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
    let mut unicode = UnicodeString {
        length: byte_len as u16,
        maximum_length: byte_len as u16,
        buffer: name.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: root
            .map(|h| h.as_raw_handle())
            .unwrap_or(std::ptr::null_mut()),
        object_name: &mut unicode,
        attributes: OBJ_CASE_INSENSITIVE,
        security_descriptor: std::ptr::null_mut(),
        security_quality_of_service: std::ptr::null_mut(),
    };
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
    let write_access = FILE_WRITE_DATA | FILE_APPEND_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
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
            "",
        ] {
            assert!(relative_parts(path).is_err(), "{path}");
        }
        assert_eq!(relative_parts(r"notes\a.txt").unwrap(), ["notes", "a.txt"]);
    }
}
