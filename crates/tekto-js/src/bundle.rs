//! Fast JS/TSX bundling via an external `esbuild` binary (MIT).
//!
//! esbuild is not a Rust crate dependency and is not required for default
//! `cargo test` or PR CI. Install it on the PATH (`npm install -g esbuild`)
//! or set `ESBUILD` to the binary. `TEKTO_ROOT` can point at the monorepo
//! root when the entry is outside the checkout.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::JsError;

pub fn which_esbuild() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ESBUILD") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    which("esbuild")
}

fn which(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            for ext in ["cmd", "exe", "bat"] {
                let candidate = dir.join(format!("{name}.{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Walk up from `start` (file or directory) looking for `packages/jsx-runtime`.
pub fn find_packages_root(start: &Path) -> Option<PathBuf> {
    if let Ok(root) = std::env::var("TEKTO_ROOT") {
        let root = PathBuf::from(root);
        if looks_like_root(&root) {
            return Some(root);
        }
    }
    let start = if start.is_file() {
        start.parent().unwrap_or(start)
    } else {
        start
    };
    for dir in start.ancestors() {
        if looks_like_root(dir) {
            return Some(dir.to_path_buf());
        }
    }
    None
}

fn looks_like_root(dir: &Path) -> bool {
    dir.join("packages/jsx-runtime/src/index.js").is_file()
        && dir.join("packages/core/src/index.js").is_file()
}

/// Virtual entry that mounts `App` from the user's file onto `globalThis.tekto`.
pub fn make_wrapper(entry: &Path) -> String {
    let path = entry.display().to_string().replace('\\', "/");
    format!(
        r#"import {{ mount }} from "@tektogui/jsx-runtime";
import * as entry from "{path}";
const App = entry.App || entry.default;
if (typeof App !== "function") {{
  throw new Error("TSX entry must export a function App (named or default)");
}}
mount(App(), globalThis.tekto);
"#
    )
}

/// Bundle `entry` to an IIFE that talks to the QuickJS `tekto` host.
pub fn bundle(entry: &Path) -> Result<String, JsError> {
    let entry = entry
        .canonicalize()
        .map_err(|e| JsError::Message(format!("cannot read {}: {e}", entry.display())))?;
    if !entry.is_file() {
        return Err(JsError::Message(format!(
            "{} is not a file",
            entry.display()
        )));
    }
    let esbuild = which_esbuild().ok_or_else(|| {
        JsError::Message(
            "esbuild not found on PATH (or $ESBUILD). Install the MIT binary with `npm install -g esbuild`."
                .into(),
        )
    })?;
    let packages = find_packages_root(&entry).ok_or_else(|| {
        JsError::Message(
            "cannot find the tektogui packages/ folder (looked for packages/jsx-runtime). Run from the repo, or set TEKTO_ROOT."
                .into(),
        )
    })?;
    let core = packages.join("packages/core/src/index.js");
    let jsx = packages.join("packages/jsx-runtime/src/index.js");

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("tekto-bundle-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir)
        .map_err(|e| JsError::Message(format!("cannot create {}: {e}", dir.display())))?;
    let wrapper_path = dir.join("entry.jsx");
    let out_path = dir.join("bundle.js");
    let write_err = std::fs::write(&wrapper_path, make_wrapper(&entry));
    if let Err(e) = write_err {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(JsError::Message(format!(
            "cannot write {}: {e}",
            wrapper_path.display()
        )));
    }

    let output = Command::new(&esbuild)
        .arg(&wrapper_path)
        .arg("--bundle")
        .arg("--format=iife")
        .arg("--platform=neutral")
        .arg("--target=es2020")
        .arg("--jsx=automatic")
        .arg("--jsx-import-source=@tektogui/jsx-runtime")
        .arg(format!("--alias:@tektogui/core={}", core.display()))
        .arg(format!("--alias:@tektogui/jsx-runtime={}", jsx.display()))
        .arg(format!(
            "--alias:@tektogui/jsx-runtime/jsx-runtime={}",
            jsx.display()
        ))
        .arg(format!(
            "--alias:@tektogui/jsx-runtime/jsx-dev-runtime={}",
            jsx.display()
        ))
        .arg(format!("--outfile={}", out_path.display()))
        .arg("--log-level=warning")
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(JsError::Message(format!(
                "failed to run {}: {e}",
                esbuild.display()
            )));
        }
    };
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_dir_all(&dir);
        return Err(JsError::Message(format!("esbuild failed:\n{err}")));
    }
    let js = std::fs::read_to_string(&out_path);
    let _ = std::fs::remove_dir_all(&dir);
    js.map_err(|e| JsError::Message(format!("cannot read bundle: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_mentions_the_entry_and_mount() {
        let w = make_wrapper(Path::new("/tmp/app.tsx"));
        assert!(w.contains("/tmp/app.tsx"), "{w}");
        assert!(w.contains("mount(App()"), "{w}");
        assert!(w.contains("@tektogui/jsx-runtime"), "{w}");
    }

    #[test]
    fn missing_esbuild_is_a_clear_error() {
        if which_esbuild().is_some() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("tekto-no-esbuild-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let entry = dir.join("app.tsx");
        std::fs::write(&entry, "export function App() { return null; }").unwrap();
        let err = bundle(&entry).unwrap_err().to_string();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(err.contains("esbuild"), "{err}");
    }

    #[test]
    fn bundles_notes_app_when_esbuild_is_present() {
        if which_esbuild().is_none() {
            return;
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let entry = root.join("examples/notes/src/app.tsx");
        if !entry.is_file() {
            return;
        }
        let js = bundle(&entry).expect("bundle");
        assert!(
            js.contains("Hello from Tekto") || js.len() > 200,
            "unexpected bundle ({} bytes)",
            js.len()
        );
        // automatic runtime must emit a jsx import, not a bare global factory
        assert!(
            js.contains("function jsx") || js.contains("jsx("),
            "bundle should include the jsx factory"
        );
        #[cfg(feature = "quickjs")]
        {
            let tree = crate::eval_to_tree(&js).expect("eval bundled Notes");
            let pretty = tree.to_pretty_text();
            assert!(pretty.contains("Hello from Tekto"), "{pretty}");
            assert!(pretty.contains("Notes"), "{pretty}");
        }
    }
}
