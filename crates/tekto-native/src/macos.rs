//! macOS text clipboard. Runtime behavior still needs a Mac.
use crate::NativeError;
use arboard::{Clipboard, Error};

fn map_error(capability: &'static str, err: Error) -> NativeError {
    match err {
        Error::ClipboardNotSupported => NativeError::Unavailable {
            capability,
            platform: "macos",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_error_has_platform_and_capability() {
        assert!(matches!(
            map_error("clipboard.get", Error::ClipboardNotSupported),
            NativeError::Unavailable {
                capability: "clipboard.get",
                platform: "macos",
                ..
            }
        ));
    }

    #[test]
    fn live_clipboard_roundtrip() {
        if std::env::var_os("TEKTO_CLIPBOARD_LIVE").is_none() {
            return;
        }
        set_text("Tekto clipboard check").unwrap();
        assert_eq!(get_text().unwrap(), "Tekto clipboard check");
    }
}
