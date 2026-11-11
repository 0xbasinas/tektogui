//! Windows toast notifications (WinRT `ToastNotificationManager`).
//!
//! Toasts need an AppUserModelID. We set one for the process and register it
//! under `HKCU\Software\Classes\AppUserModelId` so Windows can show a name and
//! so Do not disturb priority rules have a stable app identity to match.
use crate::NativeError;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

/// Stable id for apps built on Tekto. Priority rules match on this.
const APP_ID: &str = "Tekto.App";

fn unavailable(reason: &'static str) -> NativeError {
    NativeError::Unavailable {
        capability: "notifications.show",
        platform: "windows",
        reason,
    }
}

fn escape(value: &str) -> Result<String, NativeError> {
    if value.contains('\0') {
        return Err(NativeError::Message(
            "notifications.show: text must contain no NUL".into(),
        ));
    }
    Ok(value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;"))
}

fn toast_xml(title: &str, body: &str) -> Result<String, NativeError> {
    Ok(format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escape(title)?,
        escape(body)?
    ))
}

fn register_app_id() -> bool {
    let mut key = HKEY::default();
    let path = HSTRING::from(format!(r"Software\Classes\AppUserModelId\{APP_ID}"));
    // SAFETY: all pointers are valid for the calls; the key is closed below.
    unsafe {
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &path,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let name: Vec<u16> = "Tekto".encode_utf16().chain(Some(0)).collect();
        let bytes = std::slice::from_raw_parts(name.as_ptr().cast::<u8>(), name.len() * 2);
        let ok = RegSetValueExW(key, w!("DisplayName"), None, REG_SZ, Some(bytes)) == ERROR_SUCCESS;
        let _ = RegCloseKey(key);
        ok
    }
}

/// Create `Start Menu\Programs\Tekto.lnk` carrying the app id, if missing.
/// Windows lists an unpackaged app in Notification settings (and so allows a
/// priority rule) only when a shortcut with its AppUserModelID exists.
fn ensure_shortcut() -> windows::core::Result<()> {
    use windows::core::Interface;
    use windows::core::GUID;
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
        pid: 5,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    let appdata = std::env::var_os("APPDATA").ok_or_else(windows::core::Error::empty)?;
    let lnk =
        std::path::Path::new(&appdata).join(r"Microsoft\Windows\Start Menu\Programs\Tekto.lnk");
    if lnk.exists() {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|_| windows::core::Error::empty())?;
    // SAFETY: COM calls with valid arguments; interfaces drop before return.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(exe.as_os_str()))?;
        let store: IPropertyStore = link.cast()?;
        store.SetValue(&PKEY_APP_USER_MODEL_ID, &PROPVARIANT::from(APP_ID))?;
        store.Commit()?;
        let file: IPersistFile = link.cast()?;
        file.Save(&HSTRING::from(lnk.as_os_str()), true)?;
    }
    Ok(())
}

pub(crate) fn show(title: &str, body: &str) -> Result<(), NativeError> {
    let xml = toast_xml(title, body)?;
    if !register_app_id() {
        return Err(unavailable("could not register the app id"));
    }
    ensure_shortcut().map_err(|_| unavailable("could not create the Start Menu shortcut"))?;
    // SAFETY: plain call with a valid string.
    unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(APP_ID)) }
        .map_err(|_| unavailable("could not set the app id"))?;
    let doc = XmlDocument::new().map_err(|_| unavailable("XML unavailable"))?;
    doc.LoadXml(&HSTRING::from(xml))
        .map_err(|_| unavailable("invalid toast XML"))?;
    let toast = ToastNotification::CreateToastNotification(&doc)
        .map_err(|_| unavailable("toast creation failed"))?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))
        .map_err(|_| unavailable("toast notifier unavailable"))?;
    notifier
        .Show(&toast)
        .map_err(|_| unavailable("Windows refused the toast"))
}

#[cfg(test)]
mod tests {
    use super::{escape, toast_xml};

    #[test]
    fn toast_text_is_escaped_and_rejects_nul() {
        assert_eq!(escape("a<b&\"'").unwrap(), "a&lt;b&amp;&quot;&apos;");
        assert!(escape("a\0b").is_err());
        assert!(toast_xml("t", "<x>").unwrap().contains("&lt;x&gt;"));
    }
}
