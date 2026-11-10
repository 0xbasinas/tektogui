//! Linux text clipboard via arboard (X11, or Wayland when `WAYLAND_DISPLAY` is set).
//!
//! A missing session (headless CI, no display) is [`NativeError::Unavailable`],
//! not a panic. Other platforms do not compile this module.

use arboard::{Clipboard, Error as ArboardError};

use crate::{current_platform, NativeError};

pub(crate) fn get_text() -> Result<String, NativeError> {
    let mut clipboard = open("clipboard.get")?;
    match clipboard.get_text() {
        Ok(text) => Ok(text),
        // Empty clipboard or a non-text payload: callers get an empty string.
        Err(ArboardError::ContentNotAvailable) => Ok(String::new()),
        Err(err) => Err(map_arboard("clipboard.get", err)),
    }
}

pub(crate) fn set_text(text: &str) -> Result<(), NativeError> {
    let mut clipboard = open("clipboard.set")?;
    clipboard
        .set_text(text)
        .map_err(|err| map_arboard("clipboard.set", err))
}

fn open(capability: &'static str) -> Result<Clipboard, NativeError> {
    Clipboard::new().map_err(|err| map_arboard(capability, err))
}

pub(crate) fn map_arboard(capability: &'static str, err: ArboardError) -> NativeError {
    match err {
        ArboardError::ClipboardNotSupported => unavailable(capability),
        ArboardError::Unknown { description } if missing_session(&description) => {
            unavailable(capability)
        }
        other => NativeError::Message(format!("{capability}: {other}")),
    }
}

fn unavailable(capability: &'static str) -> NativeError {
    NativeError::Unavailable {
        capability,
        platform: current_platform(),
        reason: "no X11 or Wayland clipboard session",
    }
}

/// arboard reports a dead or missing X11 display as `Unknown` (the connect
/// error is rewritten to "X11 server connection timed out because it was
/// unreachable"). Wayland init failures mention Wayland in the description.
pub(crate) fn missing_session(description: &str) -> bool {
    let d = description.to_ascii_lowercase();
    d.contains("x11 server")
        || d.contains("unreachable")
        || d.contains("wayland")
        || d.contains("no display")
        || d.contains("connection refused")
        || d.contains("failed to connect")
        || d.contains("display")
}

/// True when this process should not open a clipboard.
///
/// Displays `:1` and `:2` are reserved in this repo and must not be used.
/// A real session check is opt-in via `TEKTO_CLIPBOARD_LIVE`.
#[cfg(test)]
pub(crate) fn skip_live_clipboard() -> bool {
    if std::env::var_os("TEKTO_CLIPBOARD_LIVE").is_some() {
        let display = std::env::var("DISPLAY").unwrap_or_default();
        return display == ":1" || display == ":2" || display == ":1.0" || display == ":2.0";
    }
    true
}
