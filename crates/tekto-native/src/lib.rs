//! Native plugins, exposed to both the Rust app API and TypeScript.
//!
//! Each capability is a small trait so platforms can be filled in one
//! at a time (Windows, Linux, Android, later macOS, iOS, and web).
//! Implementations in this first slice are stubs that return a clear
//! "not yet" error, but the API is stable for apps and for agents.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NativeError {
    #[error("{capability} is not implemented on {platform} yet")]
    Unsupported {
        capability: &'static str,
        platform: &'static str,
    },
    #[error("{0}")]
    Message(String),
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
        // Plain file read works on desktop even before dialogs exist.
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::read_to_string(path).map_err(|e| NativeError::Message(e.to_string()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            Err(unsupported("files.read"))
        }
    }

    fn write_string(&self, path: &str, contents: &str) -> Result<(), NativeError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::write(path, contents).map_err(|e| NativeError::Message(e.to_string()))
        }
        #[cfg(target_arch = "wasm32")]
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
        Err(unsupported("clipboard.get"))
    }
    fn set_text(&self, _text: &str) -> Result<(), NativeError> {
        Err(unsupported("clipboard.set"))
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

    #[test]
    fn reports_a_platform_name() {
        assert!(!current_platform().is_empty());
    }
}
