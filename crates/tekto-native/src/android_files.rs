//! Android app-private files, rooted at Context.getFilesDir().
#[cfg(target_os = "android")]
use crate::NativeError;

fn file_at(
    base: &std::path::Path,
    path: &str,
    write: bool,
) -> Result<std::fs::File, crate::NativeError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::path::{Component, Path};

    let fail = |reason: String| crate::NativeError::Message(format!("files: {reason}"));
    let parts: Vec<_> = Path::new(path).components().collect();
    if parts.is_empty() || parts.iter().any(|p| !matches!(p, Component::Normal(_))) {
        return Err(fail(
            "path must contain only relative normal components inside the app data directory"
                .into(),
        ));
    }
    // Reject redundant separators and dot components too, since Path normalizes them.
    if path
        .split('/')
        .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(fail("invalid relative path".into()));
    }
    use std::os::unix::fs::OpenOptionsExt;
    let mut dir = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(base)
        .map_err(|e| fail(format!("cannot open app data directory: {e}")))?;
    for (i, part) in parts.iter().enumerate() {
        let Component::Normal(name) = part else {
            unreachable!()
        };
        use std::os::unix::ffi::OsStrExt;
        let name =
            CString::new(name.as_bytes()).map_err(|_| fail("path contains a NUL byte".into()))?;
        let last = i + 1 == parts.len();
        let flags = if last && write {
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC
        } else {
            libc::O_RDONLY
        } | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | if last { 0 } else { libc::O_DIRECTORY };
        // SAFETY: the directory fd and NUL-terminated name remain valid for the call.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags, 0o600) };
        if fd < 0 {
            return Err(fail(format!(
                "cannot {} {path:?}: {}",
                if write { "write" } else { "read" },
                std::io::Error::last_os_error()
            )));
        }
        // SAFETY: openat returned a new owned file descriptor.
        dir = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    Ok(dir)
}

fn read_at(base: &std::path::Path, path: &str) -> Result<Vec<u8>, crate::NativeError> {
    use std::io::Read;
    let mut bytes = Vec::new();
    file_at(base, path, false)?
        .read_to_end(&mut bytes)
        .map_err(|e| crate::NativeError::Message(format!("files.read {path:?}: {e}")))?;
    Ok(bytes)
}

fn write_at(base: &std::path::Path, path: &str, bytes: &[u8]) -> Result<(), crate::NativeError> {
    use std::io::Write;
    file_at(base, path, true)?
        .write_all(bytes)
        .map_err(|e| crate::NativeError::Message(format!("files.write {path:?}: {e}")))
}

#[cfg(target_os = "android")]
fn root(capability: &'static str) -> Result<std::path::PathBuf, NativeError> {
    use jni::objects::{JObject, JString};
    use jni::signature::RuntimeMethodSignature;
    use jni::strings::JNIString;
    let unavailable = || NativeError::Unavailable {
        capability,
        platform: "android",
        reason: "Android activity context is not ready",
    };
    let ctx = std::panic::catch_unwind(ndk_context::android_context).map_err(|_| unavailable())?;
    if ctx.vm().is_null() || ctx.context().is_null() {
        return Err(unavailable());
    }
    // SAFETY: ndk-context publishes the process VM and a global context reference.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let context = ctx.context();
    vm.attach_current_thread(|env| -> Result<_, jni::errors::Error> {
        // SAFETY: context is the global reference published by ndk-context.
        let context = unsafe { JObject::from_raw(env, context.cast()) };
        let sig = RuntimeMethodSignature::from_str("()Ljava/io/File;").expect("valid signature");
        let file = env
            .call_method(
                &context,
                JNIString::new("getFilesDir"),
                sig.method_signature(),
                &[],
            )?
            .l()?;
        if file.is_null() {
            return Ok(None);
        }
        let sig =
            RuntimeMethodSignature::from_str("()Ljava/lang/String;").expect("valid signature");
        let path = env
            .call_method(
                &file,
                JNIString::new("getAbsolutePath"),
                sig.method_signature(),
                &[],
            )?
            .l()?;
        if path.is_null() {
            return Ok(None);
        }
        // SAFETY: File.getAbsolutePath returns a String.
        let path = unsafe { JString::from_raw(env, path.as_raw()) };
        Ok(Some(path.try_to_string(env)?))
    })
    .map_err(|err| NativeError::Message(format!("{capability}: {err}")))?
    .filter(|p| std::path::Path::new(p).is_absolute())
    .map(std::path::PathBuf::from)
    .ok_or(NativeError::Unavailable {
        capability,
        platform: "android",
        reason: "Android app files directory is unavailable",
    })
}

#[cfg(target_os = "android")]
pub(crate) fn read_file(path: &str) -> Result<Vec<u8>, NativeError> {
    read_at(&root("files.read")?, path)
}
#[cfg(target_os = "android")]
pub(crate) fn write_file(path: &str, bytes: &[u8]) -> Result<(), NativeError> {
    write_at(&root("files.write")?, path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    #[test]
    fn private_files_are_confined() {
        let root = std::env::temp_dir().join(format!("tekto-android-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("dir")).unwrap();
        write_at(&root, "dir/data", &[0, 255]).unwrap();
        assert_eq!(read_at(&root, "dir/data").unwrap(), [0, 255]);
        for path in [
            "",
            "/tmp/x",
            "../x",
            "dir/../x",
            "dir//data",
            "dir/./data",
            "bad\0name",
        ] {
            assert!(write_at(&root, path, b"x").is_err(), "{path:?}");
        }
        symlink("/tmp", root.join("link")).unwrap();
        assert!(write_at(&root, "link/escape", b"x").is_err());
        symlink("data", root.join("dir/alias")).unwrap();
        assert!(read_at(&root, "dir/alias").is_err());
        assert!(write_at(&root, "missing/file", b"x").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
