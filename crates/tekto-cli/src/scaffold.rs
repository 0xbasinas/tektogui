//! `tekto new`: write a compiling Notes-style app next to the tektogui checkout.

use std::fs;
use std::path::{Path, PathBuf};

const CARGO_TOML: &str = include_str!("../templates/app/Cargo.toml");
const MAIN_RS: &str = include_str!("../templates/app/src/main.rs");
const APP_TSX: &str = include_str!("../templates/app/src/app.tsx");
const README: &str = include_str!("../templates/app/README.md");

#[derive(Debug)]
pub struct ScaffoldError(pub String);

impl std::fmt::Display for ScaffoldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ScaffoldError {}

/// A valid directory / Cargo package name: starts with a letter, then
/// letters, digits, `_` or `-`.
pub fn sanitize_name(name: &str) -> Result<&str, ScaffoldError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ScaffoldError("app name is empty".into()));
    }
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(ScaffoldError(
            "app name must be a single directory name, not a path".into(),
        ));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() {
        return Err(ScaffoldError(
            "app name must start with a letter (Cargo package rule)".into(),
        ));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(ScaffoldError(
            "app name may only contain letters, digits, '_' and '-'".into(),
        ));
    }
    Ok(name)
}

/// Walk from cwd, the executable, and this crate's source tree to find
/// the tektogui repo root (the folder that contains `crates/tekto`).
pub fn find_tekto_root() -> Result<PathBuf, ScaffoldError> {
    if let Ok(p) = std::env::var("TEKTO_ROOT") {
        let p = PathBuf::from(p);
        if is_repo_root(&p) {
            return Ok(p);
        }
        return Err(ScaffoldError(format!(
            "TEKTO_ROOT={} does not look like a tektogui checkout",
            p.display()
        )));
    }
    let mut starts = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            starts.push(dir.to_path_buf());
        }
    }
    starts.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    for start in starts {
        for anc in start.ancestors() {
            if is_repo_root(anc) {
                return Ok(anc.to_path_buf());
            }
        }
    }
    Err(ScaffoldError(
        "could not find a tektogui checkout. Run this from the repo, or set TEKTO_ROOT.".into(),
    ))
}

fn is_repo_root(path: &Path) -> bool {
    path.join("crates/tekto/Cargo.toml").is_file() && path.join("Cargo.toml").is_file()
}

/// Path from `from_dir` to `to` using `..` segments. Both should exist
/// (or `from_dir` may be created first).
pub fn relative_from(from_dir: &Path, to: &Path) -> PathBuf {
    let from = from_dir
        .canonicalize()
        .unwrap_or_else(|_| from_dir.to_path_buf());
    let to = to.canonicalize().unwrap_or_else(|_| to.to_path_buf());
    let from_c: Vec<_> = from.components().collect();
    let to_c: Vec<_> = to.components().collect();
    let common = from_c
        .iter()
        .zip(to_c.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let ups = from_c.len().saturating_sub(common);
    let mut out = PathBuf::new();
    for _ in 0..ups {
        out.push("..");
    }
    for c in to_c.iter().skip(common) {
        out.push(c.as_os_str());
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

fn fill(template: &str, package: &str, title: &str, tekto_path: &str) -> String {
    template
        .replace("{{package}}", package)
        .replace("{{title}}", title)
        .replace("{{tekto_path}}", tekto_path)
}

fn title_case(package: &str) -> String {
    package
        .split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Write a new app into `dest` (created if needed). `dest` must be empty
/// or not yet exist.
pub fn write_app(dest: &Path, package: &str, repo_root: &Path) -> Result<(), ScaffoldError> {
    if dest.exists() {
        let empty = fs::read_dir(dest)
            .map(|mut it| it.next().is_none())
            .unwrap_or(false);
        if !empty {
            return Err(ScaffoldError(format!(
                "{} already exists and is not empty",
                dest.display()
            )));
        }
    }
    fs::create_dir_all(dest.join("src")).map_err(|e| ScaffoldError(e.to_string()))?;
    let tekto = repo_root.join("crates/tekto");
    if !tekto.join("Cargo.toml").is_file() {
        return Err(ScaffoldError(format!(
            "missing {} — is this a tektogui checkout?",
            tekto.display()
        )));
    }
    let tekto_rel = relative_from(dest, &tekto);
    let tekto_path = tekto_rel.to_string_lossy().replace('\\', "/");
    let package_path = |name: &str| {
        let path = relative_from(dest, &repo_root.join("packages").join(name));
        format!("file:{}", path.to_string_lossy().replace('\\', "/"))
    };
    let package_json = serde_json::json!({
        "name": package,
        "version": "0.1.0",
        "private": true,
        "type": "module",
        "scripts": { "typecheck": "tsc --noEmit", "build:web": "tekto-build web src/app.tsx dist/app.mjs", "build:desktop": "tekto-build desktop src/app.tsx dist/app.js" },
        "dependencies": {
            "@tektogui/core": package_path("core"),
            "@tektogui/jsx-runtime": package_path("jsx-runtime")
        },
        "devDependencies": { "typescript": "^6.0.3", "@tektogui/build": package_path("build") }
    });
    let tsconfig = serde_json::json!({
        "compilerOptions": { "target": "ES2020", "module": "ESNext", "moduleResolution": "Bundler",
            "jsx": "react-jsx", "jsxImportSource": "@tektogui/jsx-runtime", "strict": true, "noEmit": true },
        "include": ["src/**/*.ts", "src/**/*.tsx"]
    });
    let title = title_case(package);
    let files = [
        ("Cargo.toml", fill(CARGO_TOML, package, &title, &tekto_path)),
        ("src/main.rs", fill(MAIN_RS, package, &title, &tekto_path)),
        ("src/app.tsx", fill(APP_TSX, package, &title, &tekto_path)),
        ("README.md", fill(README, package, &title, &tekto_path)),
        (
            "package.json",
            format!(
                "{}\n",
                serde_json::to_string_pretty(&package_json)
                    .map_err(|e| ScaffoldError(e.to_string()))?
            ),
        ),
        (
            "tsconfig.json",
            format!(
                "{}\n",
                serde_json::to_string_pretty(&tsconfig)
                    .map_err(|e| ScaffoldError(e.to_string()))?
            ),
        ),
    ];
    for (rel, contents) in files {
        fs::write(dest.join(rel), contents).map_err(|e| ScaffoldError(e.to_string()))?;
    }
    Ok(())
}

/// `tekto new <name>`: create `./<name>` relative to the current directory.
pub fn scaffold_new(name: &str) -> Result<PathBuf, ScaffoldError> {
    let package = sanitize_name(name)?;
    let dest = PathBuf::from(package);
    let root = find_tekto_root()?;
    write_app(&dest, package, &root)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn rejects_bad_names() {
        assert!(sanitize_name("").is_err());
        assert!(sanitize_name("../x").is_err());
        assert!(sanitize_name("1app").is_err());
        assert!(sanitize_name("has space").is_err());
        assert_eq!(sanitize_name("hello").unwrap(), "hello");
        assert_eq!(sanitize_name("my-app").unwrap(), "my-app");
    }

    #[test]
    fn writes_rust_and_tsx_and_the_app_checks() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let repo = repo.canonicalize().unwrap();
        let dest = std::env::temp_dir().join("tekto-new-test-hello");
        let _ = fs::remove_dir_all(&dest);
        write_app(&dest, "hello", &repo).unwrap();
        assert!(dest.join("Cargo.toml").is_file());
        assert!(dest.join("src/main.rs").is_file());
        assert!(dest.join("src/app.tsx").is_file());
        assert!(dest.join("package.json").is_file());
        assert!(dest.join("tsconfig.json").is_file());
        assert!(dest.join("README.md").is_file());
        let cargo = fs::read_to_string(dest.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"hello\""));
        assert!(cargo.contains("crates/tekto"));
        let readme = fs::read_to_string(dest.join("README.md")).unwrap();
        assert!(readme.contains("cargo run -- --headless"));
        assert!(readme.contains("tree"));
        assert!(readme.contains("screenshot"));
        let tsx = fs::read_to_string(dest.join("src/app.tsx")).unwrap();
        assert!(tsx.contains("Hello from Tekto"));
        let package_json: serde_json::Value =
            serde_json::from_slice(&fs::read(dest.join("package.json")).unwrap()).unwrap();
        assert_eq!(package_json["scripts"]["typecheck"], "tsc --noEmit");
        assert!(package_json["scripts"]["build:web"]
            .as_str()
            .unwrap()
            .contains("tekto-build web"));
        assert!(package_json["scripts"]["build:desktop"]
            .as_str()
            .unwrap()
            .contains("tekto-build desktop"));
        assert!(package_json["dependencies"]["@tektogui/core"]
            .as_str()
            .unwrap()
            .starts_with("file:"));
        let tsconfig: serde_json::Value =
            serde_json::from_slice(&fs::read(dest.join("tsconfig.json")).unwrap()).unwrap();
        assert_eq!(
            tsconfig["compilerOptions"]["jsxImportSource"],
            "@tektogui/jsx-runtime"
        );
        // Typecheck the generated project when the workspace TypeScript tool is installed.
        // Copy only the public declarations, so the test needs neither network nor symlinks.
        let tsc =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../node_modules/typescript/bin/tsc");
        if tsc.is_file() {
            for package in ["core", "jsx-runtime"] {
                let source = repo.join("packages").join(package);
                let target = dest.join("node_modules/@tektogui").join(package);
                fs::create_dir_all(target.join("src")).unwrap();
                fs::copy(source.join("package.json"), target.join("package.json")).unwrap();
                fs::copy(source.join("src/index.d.ts"), target.join("src/index.d.ts")).unwrap();
                if package == "jsx-runtime" {
                    fs::copy(
                        source.join("src/jsx-types.d.ts"),
                        target.join("src/jsx-types.d.ts"),
                    )
                    .unwrap();
                }
            }
            let status = Command::new("node")
                .arg(tsc)
                .args(["--noEmit", "--project"])
                .arg(dest.join("tsconfig.json"))
                .status()
                .expect("node for generated project typecheck");
            assert!(status.success(), "generated app failed to typecheck");
        }
        let status = Command::new("cargo")
            .args(["check", "--manifest-path"])
            .arg(dest.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", repo.join("target"))
            .status()
            .expect("cargo");
        assert!(status.success(), "generated app failed to cargo check");
    }
}
