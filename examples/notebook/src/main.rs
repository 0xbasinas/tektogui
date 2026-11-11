//! A small persistent notebook built with the Rust API.
use std::{
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex},
};
use tekto::{
    button, layout_tree, render_headless_to_file, run_windowed_with_session, scroll, start_handle,
    text, text_field, window, AppRuntime, Color, NodeId, Tree, TreeEvent, WindowOptions,
};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 520;
const LINES: usize = 8;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Default, serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
#[serde(default)]
struct Document {
    title: String,
    lines: Vec<String>,
}
struct View {
    tree: Tree,
    title: NodeId,
    lines: Vec<NodeId>,
    save: NodeId,
    status: NodeId,
}

fn build(doc: &Document) -> Result<View, Box<dyn std::error::Error>> {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Notebook", WIDTH as f32, HEIGHT as f32)?;
    text(&mut tree, root, "Notebook", 24.0, Color::INK)?;
    let title = text_field(&mut tree, root, "Title", &doc.title)?;
    let area = scroll(&mut tree, root, f32::NAN, 260.0)?;
    tree.get_mut(area).unwrap().layout.gap = 8.0;
    let mut lines = Vec::new();
    for i in 0..LINES {
        lines.push(text_field(
            &mut tree,
            area,
            &format!("Line {}", i + 1),
            doc.lines.get(i).map(String::as_str).unwrap_or(""),
        )?);
    }
    let save = button(&mut tree, root, "Save notebook")?;
    let status = text(&mut tree, root, "Edit a line, then save.", 16.0, Color::INK)?;
    layout_tree(&mut tree, WIDTH as f32, HEIGHT as f32);
    Ok(View {
        tree,
        title,
        lines,
        save,
        status,
    })
}
fn document(tree: &Tree, title: NodeId, lines: &[NodeId]) -> Document {
    let value = |id| {
        tree.get(id)
            .and_then(|n| n.text.clone())
            .unwrap_or_default()
    };
    Document {
        title: value(title),
        lines: lines.iter().map(|id| value(*id)).collect(),
    }
}
fn status(tree: &mut Tree, id: NodeId, message: &str) {
    if let Some(node) = tree.get_mut(id) {
        node.text = Some(message.into());
        node.name = Some(message.into());
    }
}
fn load(path: &Path) -> Result<Document, String> {
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Document::default()),
        Err(e) => return Err(e.to_string()),
    };
    let doc: Document = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    if doc.lines.len() > LINES {
        return Err(format!(
            "file has {} lines; this app supports {LINES}",
            doc.lines.len()
        ));
    }
    Ok(doc)
}
fn atomic_save(path: &Path, doc: &Document) -> Result<(), String> {
    let parent = path.parent().ok_or("missing parent directory")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let data = serde_json::to_vec_pretty(doc).map_err(|e| e.to_string())?;
    let name = format!(
        ".notebook-{}-{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    );
    let temp = parent.join(name);
    let mut created = false;
    let result = (|| -> io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        created = true;
        file.write_all(&data)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|e| e.to_string())
}
fn install(view: &mut View, path: PathBuf, load_error: Option<String>) {
    let title = view.title;
    let lines = view.lines.clone();
    let save = view.save;
    let status_id = view.status;
    let saved = Arc::new(Mutex::new(if path.exists() {
        Some(document(&view.tree, title, &lines))
    } else {
        None
    }));
    if let Some(error) = &load_error {
        status(&mut view.tree, status_id, error);
    }
    view.tree.set_event_handler(move |tree, event| {
        if let Some(error) = &load_error {
            status(tree, status_id, error);
            return;
        }
        match event {
            TreeEvent::Click(id) if id == save => {
                let doc = document(tree, title, &lines);
                match atomic_save(&path, &doc) {
                    Ok(()) => {
                        *saved.lock().unwrap() = Some(doc);
                        status(tree, status_id, "Saved.");
                    }
                    Err(e) => status(tree, status_id, &format!("Save failed: {e}")),
                }
            }
            TreeEvent::Changed(id) if id == title || lines.contains(&id) => {
                let dirty = Some(document(tree, title, &lines)) != *saved.lock().unwrap();
                status(
                    tree,
                    status_id,
                    if dirty { "Unsaved changes." } else { "Saved." },
                );
            }
            _ => {}
        }
    });
}
fn app_path(relative: &Path) -> Result<PathBuf, String> {
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err("file path must contain only relative normal components".into());
    }
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|v| PathBuf::from(v).join("AppData/Local"))
        });
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| PathBuf::from(v).is_absolute())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".local/share")));
    Ok(base
        .ok_or("app data directory is unavailable")?
        .join("tekto")
        .join(relative))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut path = PathBuf::from("notebook.json");
    let mut headless = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--file" => path = PathBuf::from(args.next().ok_or("--file needs a path")?),
            "--headless" => {
                headless = Some(PathBuf::from(
                    args.next().ok_or("--headless needs an output path")?,
                ))
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    let path = app_path(&path)?;
    let (doc, error) = match load(&path) {
        Ok(doc) => (doc, None),
        Err(e) => (
            Document::default(),
            Some(format!(
                "Cannot load {}: {e}. File preserved; fix it and restart.",
                path.display()
            )),
        ),
    };
    if let Some(error) = &error {
        eprintln!("{error}");
    }
    let mut view = build(&doc)?;
    install(&mut view, path, error);
    if let Some(out) = headless {
        render_headless_to_file(&view.tree, WIDTH, HEIGHT, Some(&out))?;
        println!("wrote {}", out.display());
        return Ok(());
    }
    let runtime = AppRuntime::new(view.tree, "Notebook");
    let session = start_handle(&runtime, WIDTH, HEIGHT)?;
    run_windowed_with_session(
        runtime,
        WindowOptions {
            title: "Notebook".into(),
            width: WIDTH,
            height: HEIGHT,
            dev_server: true,
            decorations: true,
        },
        Some(session),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "tekto-notebook-test-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn message(view: &View) -> &str {
        view.tree.get(view.status).unwrap().text.as_deref().unwrap()
    }
    #[test]
    fn rapid_clicks_save_and_edits_mark_dirty() {
        let dir = Temp::new();
        let path = dir.0.join("doc.json");
        let mut view = build(&Document::default()).unwrap();
        install(&mut view, path.clone(), None);
        view.tree.insert_text(view.title, "Shopping").unwrap();
        assert_eq!(message(&view), "Unsaved changes.");
        view.tree.click(view.save).unwrap();
        view.tree.click(view.lines[0]).unwrap();
        assert_eq!(load(&path).unwrap().title, "Shopping");
        assert_eq!(message(&view), "Saved.");
        view.tree.insert_text(view.lines[0], "Milk").unwrap();
        assert_eq!(message(&view), "Unsaved changes.");
    }
    #[test]
    fn invalid_and_extra_lines_are_preserved() {
        let dir = Temp::new();
        let path = dir.0.join("doc.json");
        for data in [
            "",
            "{bad",
            "{\"lines\":[\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\"]}",
        ] {
            fs::write(&path, data).unwrap();
            let error = load(&path).unwrap_err();
            let mut view = build(&Document::default()).unwrap();
            install(&mut view, path.clone(), Some(error));
            view.tree.click(view.save).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), data);
        }
        fs::write(&path, "{\"title\":\"t\"}").unwrap();
        assert_eq!(load(&path).unwrap().title, "t");
    }
    #[test]
    fn save_failure_does_not_replace_existing_file() {
        let dir = Temp::new();
        let path = dir.0.join("destination");
        fs::create_dir(&path).unwrap();
        assert!(atomic_save(&path, &Document::default()).is_err());
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }
}
