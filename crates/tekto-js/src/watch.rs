//! Poll file mtimes so we can reload without a `notify` crate dependency.

use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use crate::bundle::find_packages_root;

pub fn is_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("ts" | "tsx" | "js" | "jsx" | "mjs")
    )
}

/// Files whose changes should trigger a reload: the entry, siblings, and
/// the local `@tektogui/*` packages when we can see them.
pub fn collect_watch_files(entry: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if entry.is_file() {
        files.push(entry.to_path_buf());
    }
    if let Some(dir) = entry.parent() {
        walk(dir, &mut files);
    }
    if let Some(root) = find_packages_root(entry) {
        walk(&root.join("packages/core/src"), &mut files);
        walk(&root.join("packages/jsx-runtime/src"), &mut files);
    }
    files.sort();
    files.dedup();
    files
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "node_modules" || name == "target" || name == ".git" {
            continue;
        }
        if path.is_dir() {
            walk(&path, files);
        } else if is_source_file(&path) {
            files.push(path);
        }
    }
}

pub fn snapshot_mtimes(files: &[PathBuf]) -> Vec<(PathBuf, Option<SystemTime>)> {
    files
        .iter()
        .map(|p| {
            (
                p.clone(),
                std::fs::metadata(p).and_then(|m| m.modified()).ok(),
            )
        })
        .collect()
}

/// Spawn a thread that polls mtimes and calls `on_change` after a short debounce.
pub fn spawn_watcher(entry: PathBuf, on_change: impl Fn() + Send + 'static) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut files = collect_watch_files(&entry);
        let mut snap = snapshot_mtimes(&files);
        loop {
            thread::sleep(Duration::from_millis(200));
            files = collect_watch_files(&entry);
            let next = snapshot_mtimes(&files);
            if next != snap {
                thread::sleep(Duration::from_millis(50));
                files = collect_watch_files(&entry);
                snap = snapshot_mtimes(&files);
                on_change();
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_notices_a_write() {
        let dir = std::env::temp_dir().join(format!("tekto-watch-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("app.tsx");
        std::fs::write(&file, "a").unwrap();
        let files = collect_watch_files(&file);
        assert!(files.iter().any(|p| p == &file), "{files:?}");
        let first = snapshot_mtimes(&files);
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&file, "b").unwrap();
        let second = snapshot_mtimes(&files);
        assert_ne!(first, second);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
