use crate::NativeError;
use std::process::{Command, Output};

fn unavailable(capability: &'static str) -> NativeError {
    NativeError::Unavailable {
        capability,
        platform: "linux",
        reason: "no usable display or zenity file dialog",
    }
}

pub(crate) fn show(save: bool) -> Result<Option<String>, NativeError> {
    let capability = if save {
        "files.save_dialog"
    } else {
        "files.open_dialog"
    };
    if std::env::var_os("DISPLAY")
        .filter(|v| !v.is_empty())
        .is_none()
        && std::env::var_os("WAYLAND_DISPLAY")
            .filter(|v| !v.is_empty())
            .is_none()
    {
        return Err(unavailable(capability));
    }
    let mut command = Command::new("zenity");
    command.arg("--file-selection");
    if save {
        command.args(["--save", "--confirm-overwrite"]);
    }
    let output = command.output().map_err(|_| unavailable(capability))?;
    parse_output(output, capability)
}

fn parse_output(output: Output, capability: &'static str) -> Result<Option<String>, NativeError> {
    if output.status.success() {
        let mut path = String::from_utf8(output.stdout)
            .map_err(|_| NativeError::Message("file dialog returned a non-UTF-8 path".into()))?;
        if path.ends_with('\n') {
            path.pop();
        }
        if path.is_empty() || !std::path::Path::new(&path).is_absolute() {
            return Err(unavailable(capability));
        }
        Ok(Some(path))
    } else if output.status.code() == Some(1) && output.stderr.is_empty() {
        Ok(None)
    } else {
        Err(unavailable(capability))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn output(code: i32, stdout: &[u8], stderr: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn parses_selection_cancel_and_failure() {
        let cap = "files.open_dialog";
        assert_eq!(
            parse_output(output(0, b"/tmp/a\n", b""), cap).unwrap(),
            Some("/tmp/a".into())
        );
        assert_eq!(parse_output(output(1, b"", b""), cap).unwrap(), None);
        assert!(matches!(
            parse_output(output(1, b"", b"display failed"), cap),
            Err(NativeError::Unavailable { .. })
        ));
        assert!(matches!(
            parse_output(output(0, b"relative\n", b""), cap),
            Err(NativeError::Unavailable { .. })
        ));
    }

    #[test]
    fn ops_have_expected_json_shape() {
        use crate::NativeOp;
        assert_eq!(
            serde_json::from_str::<NativeOp>(r#"{"op":"filesOpenDialog"}"#).unwrap(),
            NativeOp::FilesOpenDialog
        );
        assert_eq!(
            serde_json::from_str::<NativeOp>(r#"{"op":"filesSaveDialog"}"#).unwrap(),
            NativeOp::FilesSaveDialog
        );
    }
}
