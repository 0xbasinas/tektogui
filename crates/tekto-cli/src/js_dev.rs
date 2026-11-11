//! `tekto dev --js`: watch a TSX entry, rebuild the JS bundle, swap it into
//! the running app. The Rust core (window, dev server, renderer) stays up.

use std::path::PathBuf;

#[cfg(feature = "quickjs")]
use std::io;
#[cfg(feature = "quickjs")]
use std::path::Path;
#[cfg(feature = "quickjs")]
use std::sync::{mpsc, Arc};
#[cfg(feature = "quickjs")]
use std::time::{Duration, Instant};

#[cfg(feature = "quickjs")]
use tekto::{run_windowed_with_session, start_handle, AppHandle, AppRuntime, WindowOptions};
#[cfg(feature = "quickjs")]
use tekto_js::{bundle, spawn_watcher};
#[cfg(feature = "quickjs")]
use tekto_mcp::SharedTreeHandle;

#[cfg(feature = "quickjs")]
const WIDTH: u32 = 640;
#[cfg(feature = "quickjs")]
const HEIGHT: u32 = 400;

#[cfg(not(feature = "quickjs"))]
pub fn missing_feature_hint() -> String {
    "tekto dev --js needs a build with --features quickjs. Example:\n  \
     cargo run -p tekto-cli --features quickjs -- dev --js examples/notes/src/app.tsx --headless --out /tmp/notes.png"
        .into()
}

pub fn export_writeback_env(entry: &std::path::Path) {
    if let Some(root) = tekto_mcp::find_app_root(entry) {
        std::env::set_var("TEKTO_APP_ROOT", &root);
    }
    std::env::set_var("TEKTO_JS_ENTRY", entry);
    std::env::set_var("TEKTO_WRITEBACK_PREFER", "ts");
}

pub fn run_js_dev(
    entry: PathBuf,
    headless: bool,
    out: PathBuf,
    watch: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    export_writeback_env(&entry);
    #[cfg(not(feature = "quickjs"))]
    {
        let _ = (headless, out, watch);
        Err(missing_feature_hint().into())
    }
    #[cfg(feature = "quickjs")]
    {
        run_js_dev_inner(entry, headless, out, watch)
    }
}

#[cfg(feature = "quickjs")]
struct ShutdownOwner(tekto::JsAppOwner);

#[cfg(feature = "quickjs")]
impl Drop for ShutdownOwner {
    fn drop(&mut self) {
        let _ = self.0.shutdown();
    }
}

#[cfg(feature = "quickjs")]
fn bundle_logged(entry: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let t0 = Instant::now();
    let js = bundle(entry)?;
    eprintln!(
        "js bundle {}ms ({} bytes)",
        t0.elapsed().as_millis(),
        js.len()
    );
    Ok(js)
}

#[cfg(feature = "quickjs")]
fn run_js_dev_inner(
    entry: PathBuf,
    headless: bool,
    out: PathBuf,
    watch: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let js = bundle_logged(&entry)?;
    let runtime = AppRuntime::new(tekto::Tree::new(), "Notes");
    let (handle, ep, viewport) = start_handle(&runtime, WIDTH, HEIGHT)?;
    let owner = tekto::install_js_runtime(&handle, &viewport);
    let _shutdown = ShutdownOwner(owner.clone());
    owner.mount(&js)?;
    println!("dev server {}", ep.base_url());
    println!("JS entry {}", entry.display());
    println!("saving a .tsx / .ts / .js file reloads the JS side; the Rust core stays up.");
    println!("JS remounts from source, so in-memory signal state resets.");
    println!("Live ui.patch edits write back to source when the old literal is unique in the app project.");

    if watch {
        let handle = Arc::clone(&handle);
        let watched = entry.clone();
        spawn_watcher(entry.clone(), move || {
            let t0 = Instant::now();
            match bundle(&watched) {
                Ok(js) => {
                    eprintln!(
                        "js bundle {}ms ({} bytes)",
                        t0.elapsed().as_millis(),
                        js.len()
                    );
                    if let Err(err) = handle.enqueue_js(js) {
                        eprintln!("{err}");
                        handle.log(err.to_string());
                    }
                }
                Err(err) => {
                    let msg = format!("js reload failed: {err}");
                    eprintln!("{msg}");
                    handle.log(msg);
                }
            }
        });
    }

    if headless {
        write_shot(&handle, &out)?;
        println!("wrote {}", out.display());
        println!("server stays up until you press Enter");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = io::stdin().read_line(&mut line);
            let _ = tx.send(());
        });
        loop {
            if rx.try_recv().is_ok() {
                break;
            }
            if handle.apply_pending_js() {
                if let Err(err) = write_shot(&handle, &out) {
                    eprintln!("could not rewrite {}: {err}", out.display());
                } else {
                    eprintln!("rewrote {}", out.display());
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        tekto_mcp::clear_endpoint();
    } else {
        let tree = handle.tree();
        run_windowed_with_session(
            AppRuntime::new(tree, "Notes"),
            WindowOptions {
                title: "Notes".into(),
                width: WIDTH,
                height: HEIGHT,
                dev_server: true,
            },
            Some((handle, ep, viewport)),
        )?;
    }
    owner.shutdown().map_err(io::Error::other)?;
    Ok(())
}

#[cfg(feature = "quickjs")]
fn write_shot(handle: &SharedTreeHandle, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let png = handle.screenshot_png()?;
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    std::fs::write(out, png)?;
    Ok(())
}
