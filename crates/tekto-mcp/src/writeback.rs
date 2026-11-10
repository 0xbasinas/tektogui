//! Write a live `ui.patch` back into app source when the edit is unique
//! and the file is inside the app project.
//!
//! If the old literal appears more than once, or the source uses a named
//! constant instead of a literal, the runtime tree is left as patched and
//! this module reports why it did not write. Never writes outside the app
//! root, or into `target` / `node_modules` / `.git` / `crates` / `docs`.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tekto_tree::{Node, NodePatch};

/// Which sources to try first. `--js` prefers TypeScript; a Rust-only
/// `tekto dev` prefers `.rs`. The other set is a fallback when the first
/// has no literal at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Prefer {
    Ts,
    Rust,
    Any,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub text: Option<String>,
    pub name: Option<String>,
    pub fill: Option<String>,
    pub color: Option<String>,
    pub font_size: f32,
}

impl Snapshot {
    pub fn from_node(node: &Node) -> Self {
        Self {
            text: node.text.clone(),
            name: node.name.clone(),
            fill: node.fill.map(|c| c.to_hex()),
            color: node.color.map(|c| c.to_hex()),
            font_size: node.font_size,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WritebackChange {
    pub prop: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WritebackSkip {
    pub prop: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WritebackReport {
    pub changes: Vec<WritebackChange>,
    pub skipped: Vec<WritebackSkip>,
}

impl WritebackReport {
    pub fn status(&self) -> &'static str {
        match (self.changes.is_empty(), self.skipped.is_empty()) {
            (false, true) => "written",
            (false, false) => "partial",
            (true, false) => "skipped",
            (true, true) => "noop",
        }
    }

    pub fn to_json(&self) -> Value {
        let files: Vec<String> = {
            let mut v: Vec<String> = self
                .changes
                .iter()
                .map(|c| c.path.display().to_string())
                .collect();
            v.sort();
            v.dedup();
            v
        };
        json!({
            "status": self.status(),
            "files": files,
            "changes": self.changes.iter().map(|c| json!({
                "prop": c.prop,
                "path": c.path.display().to_string(),
            })).collect::<Vec<_>>(),
            "skipped": self.skipped.iter().map(|s| json!({
                "prop": s.prop,
                "reason": s.reason,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn log_line(&self) -> String {
        match self.status() {
            "written" | "partial" => {
                let paths: Vec<String> = self
                    .changes
                    .iter()
                    .map(|c| c.path.display().to_string())
                    .collect();
                let mut line = format!("write-back {}", paths.join(", "));
                for s in &self.skipped {
                    line.push_str(&format!("; skipped {}: {}", s.prop, s.reason));
                }
                line
            }
            "skipped" => {
                let reasons: Vec<String> = self
                    .skipped
                    .iter()
                    .map(|s| format!("{}: {}", s.prop, s.reason))
                    .collect();
                format!("write-back skipped: {}", reasons.join("; "))
            }
            _ => "write-back: nothing to write".into(),
        }
    }
}

pub fn prefer_from_env() -> Prefer {
    match std::env::var("TEKTO_WRITEBACK_PREFER")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "ts" | "tsx" | "js" => Prefer::Ts,
        "rs" | "rust" => Prefer::Rust,
        _ if std::env::var("TEKTO_JS_ENTRY").is_ok() => Prefer::Ts,
        _ => Prefer::Rust,
    }
}

/// App project root: `TEKTO_APP_ROOT`, or walk from `TEKTO_JS_ENTRY` / `cwd`.
pub fn app_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TEKTO_APP_ROOT") {
        let p = PathBuf::from(p);
        if p.is_dir() {
            return p.canonicalize().ok();
        }
    }
    if let Ok(entry) = std::env::var("TEKTO_JS_ENTRY") {
        if let Some(root) = find_app_root(Path::new(&entry)) {
            return Some(root);
        }
    }
    find_app_root(&std::env::current_dir().ok()?)
}

pub fn find_app_root(start: &Path) -> Option<PathBuf> {
    let start = if start.is_file() {
        start.parent().unwrap_or(start)
    } else {
        start
    };
    for dir in start.ancestors() {
        if is_app_dir(dir) {
            return dir.canonicalize().ok();
        }
    }
    None
}

fn is_app_dir(dir: &Path) -> bool {
    let has_src = dir.join("src/app.tsx").is_file()
        || dir.join("src/main.rs").is_file()
        || dir.join("src/lib.rs").is_file();
    let has_manifest = dir.join("Cargo.toml").is_file() || dir.join("package.json").is_file();
    has_src && has_manifest
}

/// Apply write-back using `TEKTO_APP_ROOT` / prefer env.
pub fn apply(old: &Snapshot, patch: &NodePatch) -> WritebackReport {
    let Some(root) = app_root() else {
        return WritebackReport {
            skipped: vec![WritebackSkip {
                prop: "project".into(),
                reason: "no app project (set TEKTO_APP_ROOT, or run `tekto dev --js` from the app)"
                    .into(),
            }],
            changes: Vec::new(),
        };
    };
    apply_in(&root, prefer_from_env(), old, patch)
}

pub fn apply_in(root: &Path, prefer: Prefer, old: &Snapshot, patch: &NodePatch) -> WritebackReport {
    let Ok(root) = root.canonicalize() else {
        return WritebackReport {
            skipped: vec![WritebackSkip {
                prop: "project".into(),
                reason: format!("cannot resolve app project {}", root.display()),
            }],
            changes: Vec::new(),
        };
    };
    let mut report = WritebackReport::default();
    if let Some(new_text) = patch.text.as_deref() {
        one(
            &root,
            prefer,
            "text",
            old.text.as_deref(),
            new_text,
            FindKind::String,
            &mut report,
        );
    }
    if let Some(new_name) = patch.name.as_deref() {
        one(
            &root,
            prefer,
            "name",
            old.name.as_deref(),
            new_name,
            FindKind::String,
            &mut report,
        );
    }
    if let Some(new_fill) = patch.fill.as_deref() {
        one(
            &root,
            prefer,
            "fill",
            old.fill.as_deref(),
            new_fill,
            FindKind::Color,
            &mut report,
        );
    }
    if let Some(new_color) = patch.color.as_deref() {
        one(
            &root,
            prefer,
            "color",
            old.color.as_deref(),
            new_color,
            FindKind::Color,
            &mut report,
        );
    }
    if let Some(new_size) = patch.font_size {
        let old_s = fmt_num(old.font_size);
        let new_s = fmt_num(new_size);
        one(
            &root,
            prefer,
            "fontSize",
            Some(old_s.as_str()),
            new_s.as_str(),
            FindKind::FontSize,
            &mut report,
        );
    }
    report
}

#[derive(Clone, Copy)]
enum FindKind {
    String,
    Color,
    FontSize,
}

fn one(
    root: &Path,
    prefer: Prefer,
    prop: &str,
    old: Option<&str>,
    new: &str,
    kind: FindKind,
    report: &mut WritebackReport,
) {
    let Some(old) = old else {
        report.skipped.push(WritebackSkip {
            prop: prop.into(),
            reason: format!("running node has no current {prop} literal to search for"),
        });
        return;
    };
    if old == new {
        return;
    }
    if old.is_empty() {
        report.skipped.push(WritebackSkip {
            prop: prop.into(),
            reason: format!("{prop} old value is empty; refusing a write"),
        });
        return;
    }
    let preferred = collect_sources(root, prefer);
    match replace_unique(root, &preferred, kind, old, new) {
        Ok(Some(path)) => report.changes.push(WritebackChange {
            prop: prop.into(),
            path: rel_path(root, &path),
        }),
        Ok(None) => report.skipped.push(WritebackSkip {
            prop: prop.into(),
            reason: format!(
                "no unique {prop} literal `{old}` in the app project (source may use a named constant, or the literal is in another language than this session prefers)"
            ),
        }),
        Err(reason) => report.skipped.push(WritebackSkip {
            prop: prop.into(),
            reason,
        }),
    }
}

fn rel_path(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| path.to_path_buf())
}

fn collect_sources(root: &Path, prefer: Prefer) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(root, root, &mut files, prefer);
    files.sort();
    files.dedup();
    files
}

fn walk(root: &Path, dir: &Path, files: &mut Vec<PathBuf>, prefer: Prefer) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if SKIP_DIRS.contains(&name) {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, files, prefer);
            continue;
        }
        if !matches_prefer(&path, prefer) {
            continue;
        }
        if !safe_to_write(root, &path) {
            continue;
        }
        files.push(path);
    }
}

const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    "crates",
    "docs",
    "dist",
    ".agents",
];

fn matches_prefer(path: &Path, prefer: Prefer) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match prefer {
        Prefer::Ts => matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs"),
        Prefer::Rust => ext == "rs",
        Prefer::Any => matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "rs"),
    }
}

fn safe_to_write(root: &Path, path: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(path) = path.canonicalize().or_else(|_| {
        // New files are not created; canonicalize fails only if missing.
        path.parent()
            .and_then(|p| p.canonicalize().ok())
            .map(|p| p.join(path.file_name().unwrap_or_default()))
            .ok_or_else(|| std::io::Error::other("no parent"))
    }) else {
        return false;
    };
    if !path.starts_with(&root) {
        return false;
    }
    for comp in path.strip_prefix(&root).unwrap_or(&path).components() {
        if let std::path::Component::Normal(name) = comp {
            if SKIP_DIRS.contains(&name.to_string_lossy().as_ref()) {
                return false;
            }
        }
    }
    true
}

/// Ok(Some(path)) written, Ok(None) no matches, Err(reason) ambiguous or IO.
fn replace_unique(
    root: &Path,
    files: &[PathBuf],
    kind: FindKind,
    old: &str,
    new: &str,
) -> Result<Option<PathBuf>, String> {
    let mut hits: Vec<(PathBuf, usize, String)> = Vec::new();
    for path in files {
        if !safe_to_write(root, path) {
            continue;
        }
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.len() > 1_000_000 {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        for (at, token) in find_spans(&src, kind, old) {
            hits.push((path.clone(), at, token));
        }
    }
    if hits.is_empty() {
        return Ok(None);
    }
    if hits.len() > 1 {
        let places: Vec<String> = hits
            .iter()
            .map(|(p, _, _)| rel_path(root, p).display().to_string())
            .collect();
        return Err(format!(
            "`{old}` appears {} times ({}); leaving the runtime change",
            hits.len(),
            places.join(", ")
        ));
    }
    let (path, start, needle) = &hits[0];
    let start = *start;
    let src = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if start + needle.len() > src.len() || &src[start..start + needle.len()] != needle {
        return Err(format!("lost the match for `{old}` while writing"));
    }
    let replacement = rewrite_match(kind, needle, needle, new);
    let mut next = String::with_capacity(src.len() + replacement.len());
    next.push_str(&src[..start]);
    next.push_str(&replacement);
    next.push_str(&src[start + needle.len()..]);
    if !safe_to_write(root, path) {
        return Err(format!(
            "refusing to write {} (outside the app project)",
            path.display()
        ));
    }
    std::fs::write(path, next).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(Some(path.clone()))
}

fn find_spans(src: &str, kind: FindKind, old: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for token in search_tokens(kind, old) {
        let mut start = 0;
        while let Some(i) = src[start..].find(&token) {
            let at = start + i;
            let end = at + token.len();
            if number_boundary_ok(kind, src, end) {
                out.push((at, token.clone()));
            }
            start = at + token.len();
        }
    }
    // Overlapping tokens at the same offset (e.g. `font_size: 22` vs
    // `font_size: 22.0`) count as one hit; keep the longest.
    out.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.len().cmp(&a.1.len())));
    let mut deduped = Vec::new();
    let mut last_end = 0usize;
    for (at, token) in out {
        if at < last_end {
            continue;
        }
        last_end = at + token.len();
        deduped.push((at, token));
    }
    deduped
}

fn number_boundary_ok(kind: FindKind, src: &str, end: usize) -> bool {
    if !matches!(kind, FindKind::FontSize) {
        return true;
    }
    !matches!(src[end..].chars().next(), Some('0'..='9') | Some('.'))
}

fn search_tokens(kind: FindKind, old: &str) -> Vec<String> {
    match kind {
        FindKind::String => {
            let mut toks = vec![format!("\"{}\"", escape_double(old))];
            if !old.contains('\'') {
                toks.push(format!("'{old}'"));
            }
            toks
        }
        FindKind::Color => {
            let hex = normalize_hex(old).unwrap_or_else(|| old.to_string());
            let mut toks = Vec::new();
            for form in hex_forms(&hex) {
                toks.push(format!("\"{form}\""));
                toks.push(format!("'{form}'"));
            }
            toks
        }
        FindKind::FontSize => {
            let mut toks = vec![
                format!("fontSize={{{old}}}"),
                format!("fontSize={{ {old} }}"),
                format!("font_size: {old}"),
                format!("font_size: {old}.0"),
            ];
            if !old.contains('.') {
                toks.push(format!("fontSize={{{old}.0}}"));
                toks.push(format!("font_size: {old}.0"));
            }
            toks
        }
    }
}

fn rewrite_match(kind: FindKind, _token_pat: &str, found: &str, new: &str) -> String {
    match kind {
        FindKind::String => {
            if found.starts_with('\'') {
                format!("'{}'", new.replace('\'', "\\'"))
            } else {
                format!("\"{}\"", escape_double(new))
            }
        }
        FindKind::Color => {
            let hex = normalize_hex(new).unwrap_or_else(|| new.to_string());
            if found.starts_with('\'') {
                format!("'{hex}'")
            } else {
                format!("\"{hex}\"")
            }
        }
        FindKind::FontSize => {
            if found.starts_with("fontSize=") {
                let spaced = found.contains(' ');
                if spaced {
                    format!("fontSize={{ {new} }}")
                } else if found.contains(".0") && !new.contains('.') {
                    format!("fontSize={{{new}.0}}")
                } else {
                    format!("fontSize={{{new}}}")
                }
            } else if found.contains(".0") && !new.contains('.') {
                format!("font_size: {new}.0")
            } else {
                format!("font_size: {new}")
            }
        }
    }
}

fn escape_double(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn normalize_hex(s: &str) -> Option<String> {
    tekto_tree::Color::from_hex(s).map(|c| c.to_hex())
}

fn hex_forms(hex: &str) -> Vec<String> {
    let mut forms = vec![hex.to_string(), hex.to_ascii_uppercase()];
    if let Some(stripped) = hex.strip_prefix('#') {
        forms.push(format!("#{stripped}"));
        forms.push(format!("#{}", stripped.to_ascii_uppercase()));
    }
    forms.sort();
    forms.dedup();
    forms
}

fn fmt_num(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i32)
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_app() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("tekto-writeback-{}-{stamp}", std::process::id()));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        dir
    }

    fn text_patch(old: &str, new: &str) -> (Snapshot, NodePatch) {
        let old_s = Snapshot {
            text: Some(old.into()),
            ..Snapshot::default()
        };
        let patch = NodePatch {
            text: Some(new.into()),
            ..NodePatch::default()
        };
        (old_s, patch)
    }

    #[test]
    fn writes_a_unique_tsx_string_literal() {
        let dir = temp_app();
        let app = dir.join("src/app.tsx");
        fs::write(
            &app,
            r#"export function App() {
  const [greeting] = createSignal("Hello from Tekto");
  return <text fontSize={20}>{greeting()}</text>;
}
"#,
        )
        .unwrap();
        let (old, patch) = text_patch("Hello from Tekto", "Hello after write-back");
        let report = apply_in(&dir, Prefer::Ts, &old, &patch);
        assert_eq!(report.status(), "written", "{report:?}");
        let src = fs::read_to_string(&app).unwrap();
        assert!(src.contains("Hello after write-back"), "{src}");
        assert!(!src.contains("Hello from Tekto"), "{src}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_when_the_literal_appears_twice() {
        let dir = temp_app();
        fs::write(
            dir.join("src/app.tsx"),
            r#"const a = "Hello from Tekto";
const b = "Hello from Tekto";
"#,
        )
        .unwrap();
        let (old, patch) = text_patch("Hello from Tekto", "Nope");
        let report = apply_in(&dir, Prefer::Ts, &old, &patch);
        assert_eq!(report.status(), "skipped", "{report:?}");
        assert!(
            report.skipped.iter().any(|s| s.reason.contains("2 times")),
            "{report:?}"
        );
        let src = fs::read_to_string(dir.join("src/app.tsx")).unwrap();
        assert_eq!(src.matches("Hello from Tekto").count(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_named_colors_without_a_hex_literal() {
        let dir = temp_app();
        fs::write(
            dir.join("src/main.rs"),
            r#"n.fill = Some(Color::BANNER);
"#,
        )
        .unwrap();
        let old = Snapshot {
            fill: Some("#3a424a".into()),
            ..Snapshot::default()
        };
        let patch = NodePatch {
            fill: Some("#ff0000".into()),
            ..NodePatch::default()
        };
        let report = apply_in(&dir, Prefer::Rust, &old, &patch);
        assert_eq!(report.status(), "skipped", "{report:?}");
        let src = fs::read_to_string(dir.join("src/main.rs")).unwrap();
        assert!(src.contains("Color::BANNER"), "{src}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn writes_a_unique_fontsize() {
        let dir = temp_app();
        let app = dir.join("src/app.tsx");
        fs::write(&app, r#"<text fontSize={22}>Notes</text>"#).unwrap();
        let old = Snapshot {
            font_size: 22.0,
            ..Snapshot::default()
        };
        let patch = NodePatch {
            font_size: Some(28.0),
            ..NodePatch::default()
        };
        let report = apply_in(&dir, Prefer::Ts, &old, &patch);
        assert_eq!(report.status(), "written", "{report:?}");
        let src = fs::read_to_string(&app).unwrap();
        assert!(src.contains("fontSize={28}"), "{src}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefers_tsx_and_does_not_touch_a_second_language_copy() {
        let dir = temp_app();
        fs::write(dir.join("src/app.tsx"), r#"const g = "Hello from Tekto";"#).unwrap();
        fs::write(dir.join("src/main.rs"), r#"let g = "Hello from Tekto";"#).unwrap();
        let (old, patch) = text_patch("Hello from Tekto", "Only TS");
        let report = apply_in(&dir, Prefer::Ts, &old, &patch);
        assert_eq!(report.status(), "written", "{report:?}");
        let ts = fs::read_to_string(dir.join("src/app.tsx")).unwrap();
        let rs = fs::read_to_string(dir.join("src/main.rs")).unwrap();
        assert!(ts.contains("Only TS"), "{ts}");
        assert!(rs.contains("Hello from Tekto"), "{rs}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_to_write_outside_the_app_project() {
        let dir = temp_app();
        // A crates/ folder must never be touched even if it sits under the root.
        fs::create_dir_all(dir.join("crates/tekto-widgets/src")).unwrap();
        fs::write(
            dir.join("crates/tekto-widgets/src/lib.rs"),
            r#"text("Hello from Tekto");"#,
        )
        .unwrap();
        fs::write(
            dir.join("src/app.tsx"),
            "export function App() { return null; }\n",
        )
        .unwrap();
        let (old, patch) = text_patch("Hello from Tekto", "Hacked");
        let report = apply_in(&dir, Prefer::Any, &old, &patch);
        assert_eq!(report.status(), "skipped", "{report:?}");
        let src = fs::read_to_string(dir.join("crates/tekto-widgets/src/lib.rs")).unwrap();
        assert!(src.contains("Hello from Tekto"), "{src}");
        assert!(!src.contains("Hacked"), "{src}");
        let _ = fs::remove_dir_all(&dir);
    }
}
