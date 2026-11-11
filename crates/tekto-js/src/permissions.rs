//! Capability checks for the retained JS native bridge.
//!
//! A [`NativePermissions`] set names the native call families a session may
//! use. The default is every documented family, so existing apps keep working;
//! an embedding that runs agent-written TSX should opt in to a narrower set
//! with [`TreeHost::set_native_permissions`](crate::TreeHost::set_native_permissions).
//!
//! This is a flat allow-list. It does not scope file paths, prompt the user,
//! or replace the platform's own permission prompts.

use std::collections::BTreeSet;
use tekto_native::NativeOp;

/// One family of native calls a retained JS session may make.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeCapability {
    Clipboard,
    Files,
    Notifications,
    Tray,
    Shortcuts,
}

impl NativeCapability {
    /// Every capability this layer knows about, in a stable order.
    pub const ALL: [Self; 5] = [
        Self::Clipboard,
        Self::Files,
        Self::Notifications,
        Self::Tray,
        Self::Shortcuts,
    ];

    /// The name used in errors and configuration.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clipboard => "clipboard",
            Self::Files => "files",
            Self::Notifications => "notifications",
            Self::Tray => "tray",
            Self::Shortcuts => "shortcuts",
        }
    }

    /// Parse one name. Case and surrounding spaces are ignored.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "clipboard" => Some(Self::Clipboard),
            "files" | "file" => Some(Self::Files),
            "notifications" | "notification" => Some(Self::Notifications),
            "tray" => Some(Self::Tray),
            "shortcuts" | "shortcut" => Some(Self::Shortcuts),
            _ => None,
        }
    }

    /// The family a concrete call belongs to.
    pub fn of(op: &NativeOp) -> Self {
        match op {
            NativeOp::ClipboardGetText | NativeOp::ClipboardSetText { .. } => Self::Clipboard,
            NativeOp::FilesOpenDialog
            | NativeOp::FilesSaveDialog
            | NativeOp::FilesReadText { .. }
            | NativeOp::FilesWriteText { .. }
            | NativeOp::FilesReadBytes { .. }
            | NativeOp::FilesWriteBytes { .. } => Self::Files,
            NativeOp::NotificationsShow { .. } => Self::Notifications,
            NativeOp::TrayShow { .. }
            | NativeOp::TrayHide
            | NativeOp::TraySetLabel { .. }
            | NativeOp::TrayTakeClick => Self::Tray,
            NativeOp::ShortcutsRegister { .. }
            | NativeOp::ShortcutsUnregister { .. }
            | NativeOp::ShortcutsTakeActivation => Self::Shortcuts,
        }
    }
}

/// The native call families a host allows. Checks are per family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativePermissions {
    allowed: BTreeSet<NativeCapability>,
}

impl Default for NativePermissions {
    fn default() -> Self {
        Self::allow_all()
    }
}

impl NativePermissions {
    /// Allow every documented family. This is the default so a host that does
    /// not configure permissions behaves as it did before this check existed.
    pub fn allow_all() -> Self {
        Self {
            allowed: NativeCapability::ALL.into_iter().collect(),
        }
    }

    /// Deny every family. A good starting point for agent-written apps.
    pub fn deny_all() -> Self {
        Self {
            allowed: BTreeSet::new(),
        }
    }

    /// Allow only the named families.
    pub fn only(capabilities: impl IntoIterator<Item = NativeCapability>) -> Self {
        Self {
            allowed: capabilities.into_iter().collect(),
        }
    }

    /// Parse a list such as `["clipboard", "files"]`. `all` expands to every
    /// family, and mixing `all` with names is allowed. An unknown name is an
    /// error that lists the accepted names.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<Self, String> {
        let mut permissions = Self::deny_all();
        for name in names {
            if name.trim().eq_ignore_ascii_case("all") {
                permissions = Self::allow_all();
                continue;
            }
            let capability = NativeCapability::from_name(name).ok_or_else(|| {
                format!(
                    "unknown native capability `{name}`; expected all, clipboard, files, notifications, tray, or shortcuts"
                )
            })?;
            permissions.allowed.insert(capability);
        }
        Ok(permissions)
    }

    pub fn allow(&mut self, capability: NativeCapability) {
        self.allowed.insert(capability);
    }

    pub fn deny(&mut self, capability: NativeCapability) {
        self.allowed.remove(&capability);
    }

    pub fn allows(&self, capability: NativeCapability) -> bool {
        self.allowed.contains(&capability)
    }

    pub fn allows_op(&self, op: &NativeOp) -> bool {
        self.allows(NativeCapability::of(op))
    }

    /// Allowed names, for errors, logs, and tests.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.allowed.iter().map(|capability| capability.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_allows_every_family_and_deny_allows_none() {
        let all = NativePermissions::default();
        assert_eq!(all, NativePermissions::allow_all());
        for capability in NativeCapability::ALL {
            assert!(all.allows(capability));
        }
        let none = NativePermissions::deny_all();
        for capability in NativeCapability::ALL {
            assert!(!none.allows(capability));
        }
    }

    #[test]
    fn names_round_trip_and_reject_unknowns() {
        let parsed = NativePermissions::from_names(["clipboard", " Files "]).unwrap();
        assert!(parsed.allows(NativeCapability::Clipboard));
        assert!(parsed.allows(NativeCapability::Files));
        assert!(!parsed.allows(NativeCapability::Notifications));
        assert_eq!(
            parsed.names().collect::<Vec<_>>(),
            vec!["clipboard", "files"]
        );
        assert_eq!(
            NativePermissions::from_names(["all"]).unwrap(),
            NativePermissions::allow_all()
        );
        let error = NativePermissions::from_names(["teleport"]).unwrap_err();
        assert!(
            error.contains("unknown native capability `teleport`"),
            "{error}"
        );
    }

    #[test]
    fn each_op_maps_to_one_family() {
        let cases = [
            (
                NativeOp::ClipboardGetText,
                NativeCapability::Clipboard,
                "clipboard",
            ),
            (
                NativeOp::FilesReadText {
                    path: "note.txt".into(),
                },
                NativeCapability::Files,
                "files",
            ),
            (
                NativeOp::NotificationsShow {
                    title: "t".into(),
                    body: "b".into(),
                },
                NativeCapability::Notifications,
                "notifications",
            ),
            (NativeOp::TrayTakeClick, NativeCapability::Tray, "tray"),
            (
                NativeOp::ShortcutsTakeActivation,
                NativeCapability::Shortcuts,
                "shortcuts",
            ),
        ];
        for (op, capability, name) in cases {
            assert_eq!(NativeCapability::of(&op), capability);
            assert_eq!(capability.as_str(), name);
            assert!(!NativePermissions::deny_all().allows_op(&op));
            assert!(NativePermissions::only([capability]).allows_op(&op));
        }
    }
}
