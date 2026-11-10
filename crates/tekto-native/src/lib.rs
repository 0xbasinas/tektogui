//! Native plugins, exposed to both the Rust app API and TypeScript.
//!
//! Each capability is a small trait so platforms can be filled in one
//! at a time (Windows, Linux, Android, later macOS, iOS, and web).
//! Linux text clipboard is implemented (X11 and Wayland). Other platforms
//! still return a clear "not yet" error for clipboard. The API is stable
//! for apps and for the JS [`NativeOp`] hook.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(target_os = "linux")]
mod linux;

#[derive(Debug, Error)]
pub enum NativeError {
    #[error("{capability} is not implemented on {platform} yet")]
    Unsupported {
        capability: &'static str,
        platform: &'static str,
    },
    #[error("{0}")]
    Message(String),
    /// The platform implements this capability, but the session cannot use it
    /// (for example Linux clipboard with no X11 or Wayland display).
    #[error("{capability} is not available on {platform}: {reason}")]
    Unavailable {
        capability: &'static str,
        platform: &'static str,
        reason: &'static str,
    },
}

/// JSON ops for native plugins. Separate from the scene-tree `HostOp` list
/// because clipboard get returns text rather than node ids.
///
/// TypeScript can send the same shape later:
/// `{"op":"clipboardGetText"}` and `{"op":"clipboardSetText","text":"..."}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum NativeOp {
    ClipboardGetText,
    ClipboardSetText { text: String },
    FilesReadText { path: String },
    FilesWriteText { path: String, text: String },
    FilesReadBytes { path: String },
    FilesWriteBytes { path: String, bytes: Vec<u8> },
}

/// Run one [`NativeOp`] against [`native`].
///
/// `ClipboardGetText` returns `Some(text)` (empty string if the clipboard has
/// no text). `ClipboardSetText` returns `None` on success.
pub fn apply_native_op(op: &NativeOp) -> Result<Option<String>, NativeError> {
    let host = native();
    match op {
        NativeOp::ClipboardGetText => host.get_text().map(Some),
        NativeOp::ClipboardSetText { text } => host.set_text(text).map(|()| None),
        NativeOp::FilesReadText { path } => host.read_to_string(path).map(Some),
        NativeOp::FilesWriteText { path, text } => host.write_string(path, text).map(|()| None),
        NativeOp::FilesReadBytes { path } => host.read_bytes(path).map(|bytes| {
            Some(format!(
                "[{}]",
                bytes
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ))
        }),
        NativeOp::FilesWriteBytes { path, bytes } => host.write_bytes(path, bytes).map(|()| None),
    }
}

pub fn current_platform() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "android")]
    {
        "android"
    }
    #[cfg(target_os = "ios")]
    {
        "ios"
    }
    #[cfg(target_arch = "wasm32")]
    {
        "web"
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "macos",
        target_os = "android",
        target_os = "ios",
        target_arch = "wasm32"
    )))]
    {
        "unknown"
    }
}

fn unsupported(capability: &'static str) -> NativeError {
    NativeError::Unsupported {
        capability,
        platform: current_platform(),
    }
}

pub trait Files {
    fn read_to_string(&self, path: &str) -> Result<String, NativeError>;
    fn write_string(&self, path: &str, contents: &str) -> Result<(), NativeError>;
    fn read_bytes(&self, path: &str) -> Result<Vec<u8>, NativeError>;
    fn write_bytes(&self, path: &str, contents: &[u8]) -> Result<(), NativeError>;
    fn open_dialog(&self) -> Result<Option<String>, NativeError>;
    fn save_dialog(&self) -> Result<Option<String>, NativeError>;
}

pub trait Clipboard {
    fn get_text(&self) -> Result<String, NativeError>;
    fn set_text(&self, text: &str) -> Result<(), NativeError>;
}

pub trait Notifications {
    fn show(&self, title: &str, body: &str) -> Result<(), NativeError>;
}

pub trait SystemTray {
    fn set_label(&self, label: &str) -> Result<(), NativeError>;
}

pub trait GlobalShortcuts {
    fn register(&self, accelerator: &str, id: &str) -> Result<(), NativeError>;
}

/// Default stub used until a platform plugin is filled in.
pub struct StubNative;

impl Files for StubNative {
    fn read_to_string(&self, path: &str) -> Result<String, NativeError> {
        String::from_utf8(self.read_bytes(path)?)
            .map_err(|e| NativeError::Message(format!("files.read: invalid UTF-8: {e}")))
    }

    fn write_string(&self, path: &str, contents: &str) -> Result<(), NativeError> {
        self.write_bytes(path, contents.as_bytes())
    }

    fn read_bytes(&self, path: &str) -> Result<Vec<u8>, NativeError> {
        #[cfg(target_os = "linux")]
        {
            linux::read_file(path)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = path;
            Err(unsupported("files.read"))
        }
    }

    fn write_bytes(&self, path: &str, contents: &[u8]) -> Result<(), NativeError> {
        #[cfg(target_os = "linux")]
        {
            linux::write_file(path, contents)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (path, contents);
            Err(unsupported("files.write"))
        }
    }

    fn open_dialog(&self) -> Result<Option<String>, NativeError> {
        Err(unsupported("files.open_dialog"))
    }

    fn save_dialog(&self) -> Result<Option<String>, NativeError> {
        Err(unsupported("files.save_dialog"))
    }
}

impl Clipboard for StubNative {
    fn get_text(&self) -> Result<String, NativeError> {
        #[cfg(target_os = "linux")]
        {
            linux::get_text()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(unsupported("clipboard.get"))
        }
    }
    fn set_text(&self, text: &str) -> Result<(), NativeError> {
        #[cfg(target_os = "linux")]
        {
            linux::set_text(text)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = text;
            Err(unsupported("clipboard.set"))
        }
    }
}

impl Notifications for StubNative {
    fn show(&self, _title: &str, _body: &str) -> Result<(), NativeError> {
        Err(unsupported("notifications.show"))
    }
}

impl SystemTray for StubNative {
    fn set_label(&self, _label: &str) -> Result<(), NativeError> {
        Err(unsupported("tray.set_label"))
    }
}

impl GlobalShortcuts for StubNative {
    fn register(&self, _accelerator: &str, _id: &str) -> Result<(), NativeError> {
        Err(unsupported("shortcuts.register"))
    }
}

pub fn native() -> StubNative {
    StubNative
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn files_are_confined_and_ops_round_trip() {
        use std::os::unix::fs::symlink;
        if std::env::var_os("TEKTO_FILES_TEST_CHILD").is_none() {
            let dir = std::env::temp_dir().join(format!("tekto-files-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["tests::files_are_confined_and_ops_round_trip", "--exact"])
                .env("XDG_DATA_HOME", &dir)
                .env("TEKTO_FILES_TEST_CHILD", "1")
                .output()
                .unwrap();
            std::fs::remove_dir_all(&dir).unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let host = native();
        host.write_string("hello", "hello λ").unwrap();
        assert_eq!(host.read_to_string("hello").unwrap(), "hello λ");
        let op = NativeOp::FilesWriteBytes {
            path: "binary".into(),
            bytes: vec![0, 255],
        };
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(serde_json::from_str::<NativeOp>(&json).unwrap(), op);
        apply_native_op(&op).unwrap();
        assert_eq!(
            apply_native_op(&NativeOp::FilesReadBytes {
                path: "binary".into()
            })
            .unwrap()
            .unwrap(),
            "[0,255]"
        );
        assert!(host.read_to_string("binary").is_err());
        for path in ["../escape", "/tmp/escape", "a/../escape", "./hello"] {
            assert!(host.write_string(path, "bad").is_err(), "{path}");
        }
        let root =
            std::path::PathBuf::from(std::env::var_os("XDG_DATA_HOME").unwrap()).join("tekto");
        symlink("/tmp", root.join("link")).unwrap();
        assert!(host.write_string("link/escape", "bad").is_err());
        symlink("/tmp/tekto-escape", root.join("file-link")).unwrap();
        assert!(host.write_string("file-link", "bad").is_err());
        assert!(host.read_bytes("missing").is_err());
    }

    #[test]
    fn reports_a_platform_name() {
        assert!(!current_platform().is_empty());
    }

    #[test]
    fn clipboard_op_json_roundtrip() {
        let get: NativeOp = serde_json::from_str(r#"{"op":"clipboardGetText"}"#).unwrap();
        assert_eq!(get, NativeOp::ClipboardGetText);
        let set = NativeOp::ClipboardSetText {
            text: "hello".into(),
        };
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, r#"{"op":"clipboardSetText","text":"hello"}"#);
        let back: NativeOp = serde_json::from_str(&json).unwrap();
        assert_eq!(back, set);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn missing_x11_session_maps_to_unavailable() {
        let err = linux::map_arboard(
            "clipboard.get",
            arboard::Error::Unknown {
                description: "X11 server connection timed out because it was unreachable".into(),
            },
        );
        assert!(matches!(
            err,
            NativeError::Unavailable {
                capability: "clipboard.get",
                platform: "linux",
                ..
            }
        ));
        let msg = err.to_string();
        assert!(
            msg.contains("not available") && msg.contains("clipboard"),
            "{msg}"
        );
    }

    /// Headless child (spawned below): no `DISPLAY` / `WAYLAND_DISPLAY`.
    /// Must fail with [`NativeError::Unavailable`] and must not hang.
    #[cfg(target_os = "linux")]
    fn assert_headless_clipboard_error() {
        let err = native().get_text().expect_err("headless clipboard");
        assert!(
            matches!(
                err,
                NativeError::Unavailable {
                    capability: "clipboard.get",
                    ..
                }
            ),
            "{err}"
        );
        let set_err = apply_native_op(&NativeOp::ClipboardSetText {
            text: "should-not-land".into(),
        })
        .expect_err("headless set");
        assert!(
            matches!(
                set_err,
                NativeError::Unavailable {
                    capability: "clipboard.set",
                    ..
                }
            ),
            "{set_err}"
        );
    }

    /// Default test never opens displays `:1` or `:2`. On Linux it re-executes
    /// itself with the session variables cleared so a developer machine that
    /// has `DISPLAY` still covers the missing-clipboard error.
    /// Set `TEKTO_CLIPBOARD_LIVE=1` to round-trip text on the current session.
    #[test]
    fn clipboard_missing_session_or_opt_in_roundtrip() {
        let display = std::env::var("DISPLAY").unwrap_or_default();
        let reserved = matches!(display.as_str(), ":1" | ":2" | ":1.0" | ":2.0");
        assert!(
            !reserved || std::env::var_os("TEKTO_CLIPBOARD_LIVE").is_none(),
            "refusing clipboard on reserved display {display}"
        );

        #[cfg(target_os = "linux")]
        if std::env::var_os("TEKTO_CLIPBOARD_HEADLESS_CHILD").is_some() {
            assert_headless_clipboard_error();
            return;
        }

        #[cfg(target_os = "linux")]
        if std::env::var_os("TEKTO_CLIPBOARD_LIVE").is_some() && !linux::skip_live_clipboard() {
            let probe = "tektogui-clipboard-probe";
            native().set_text(probe).expect("set clipboard text");
            // A second arboard handle must still see the text after `set_text`
            // returns. Dropping the owner on X11 clears the selection when no
            // clipboard manager is running.
            let got_fresh = arboard::Clipboard::new()
                .expect("second clipboard")
                .get_text()
                .expect("text still owned after set");
            assert_eq!(got_fresh, probe);
            let got = native().get_text().expect("get clipboard text");
            assert_eq!(got, probe);
            let via = apply_native_op(&NativeOp::ClipboardGetText)
                .expect("native op")
                .expect("text");
            assert_eq!(via, probe);
            return;
        }

        #[cfg(target_os = "linux")]
        {
            let exe = std::env::current_exe().expect("test exe");
            let output = std::process::Command::new(exe)
                .args([
                    "tests::clipboard_missing_session_or_opt_in_roundtrip",
                    "--exact",
                ])
                .env_remove("DISPLAY")
                .env_remove("WAYLAND_DISPLAY")
                .env("TEKTO_CLIPBOARD_HEADLESS_CHILD", "1")
                .output()
                .expect("spawn headless clipboard test");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "headless clipboard child failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
            );
        }

        #[cfg(not(target_os = "linux"))]
        {
            let err = native().get_text().expect_err("stub");
            assert!(matches!(
                err,
                NativeError::Unsupported {
                    capability: "clipboard.get",
                    ..
                }
            ));
        }
    }
}
