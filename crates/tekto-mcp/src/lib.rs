//! Dev server shared by the MCP tools and the `tekto` CLI.
//!
//! The running app listens on a local HTTP port and writes the port to a
//! small JSON file. `tekto tree`, `tekto screenshot`, and the stdio MCP
//! adapter all talk to that same server.
//!
//! Live edits (`POST /patch`) change node props in the running tree and, when
//! the old literal is unique in the app project, write that change back to
//! source. Ambiguous edits stay in the runtime tree and are reported.
//! `POST /reload` accepts a JS bundle and queues it for the JS runtime.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tekto_tree::{route_named_key, NamedActionKey, NodeId, NodePatch, Store, Tree};

mod http_policy;
mod stdio;
mod writeback;
pub use writeback::{
    app_root, apply, apply_in, find_app_root, prefer_from_env, Prefer, Snapshot, WritebackReport,
};

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl McpError {
    pub fn msg(m: impl Into<String>) -> Self {
        McpError::Message(m.into())
    }
}

/// Check whether a key name is supported by agent input.
pub fn validate_key_name(name: &str) -> Result<(), String> {
    name.parse::<NamedActionKey>().map(|_| ())
}

/// What the running app can do for agents.
pub trait AppHandle: Send + Sync {
    fn tree(&self) -> Tree;
    fn screenshot_png(&self) -> Result<Vec<u8>, McpError>;
    fn patch(&self, id: NodeId, patch: NodePatch) -> Result<(), McpError>;
    fn click(&self, id: NodeId) -> Result<(), McpError> {
        let _ = id;
        Err(McpError::msg("click is not wired up yet"))
    }
    fn key(&self, _key: NamedActionKey) -> Result<(), McpError> {
        Err(McpError::msg("key is not wired up yet"))
    }
    fn scroll(&self, _id: NodeId, _delta: f32) -> Result<f32, McpError> {
        Err(McpError::msg("scroll is not wired up yet"))
    }
    fn type_text(&self, id: NodeId, text: &str) -> Result<(), McpError> {
        let _ = (id, text);
        Err(McpError::msg(
            "type is not wired up yet — text fields accept plain characters on the desktop",
        ))
    }
    fn state_json(&self) -> Value {
        json!({})
    }
    /// Recorded store frames. Default is history off and an empty list.
    fn states_json(&self) -> Value {
        json!({ "enabled": false, "cursor": 0, "states": [] })
    }
    /// Jump the opted-in store to `index`. Default reports that history is off.
    fn restore_state(&self, index: u64) -> Result<Value, McpError> {
        let _ = index;
        Err(McpError::msg(
            "time travel is off — opt in with Store::recording and attach it to the dev handle",
        ))
    }
    fn logs(&self) -> Vec<String> {
        Vec::new()
    }
    /// Queue a JS bundle to be evaluated on the app thread.
    ///
    /// Used by `POST /reload` so a host can push a new bundle over
    /// `adb forward` without rebuilding the Rust core. Default: error
    /// (no JS runtime attached).
    fn queue_js(&self, source: String) -> Result<(), McpError> {
        let _ = source;
        Err(McpError::msg(
            "no JS runtime attached — start with `tekto dev --js <entry.tsx>`",
        ))
    }
    /// Source write-back from the last `patch`, if any.
    fn last_writeback(&self) -> Option<Value> {
        None
    }
}

/// PNG renderer used by [`SharedTreeHandle`].
pub type RenderFn = Arc<dyn Fn(&Tree) -> Result<Vec<u8>, McpError> + Send + Sync>;

/// A simple handle over a mutex-protected tree plus a render callback.
pub type OnChangeFn = Arc<dyn Fn() + Send + Sync>;

/// Evaluates a JS bundle and swaps the tree. Installed by `tekto dev --js`.
pub type ApplyJsFn = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

pub type ClickJsFn = Arc<dyn Fn(NodeId) -> Result<(), String> + Send + Sync>;

pub struct SharedTreeHandle {
    /// Serializes supported handle actions with retained session reloads.
    pub action_gate: Arc<Mutex<()>>,
    pub click_js: Mutex<Option<ClickJsFn>>,
    /// Direct concurrent mutation is unsupported while a JS session is active.
    /// Use handle operations so they are ordered with clicks and reloads.
    pub tree: Arc<Mutex<Tree>>,
    pub render: RenderFn,
    pub logs: Arc<Mutex<Vec<String>>>,
    /// Called after a live patch or click so a window can redraw
    /// and refresh AccessKit. Also used to wake the loop for a JS reload.
    pub on_change: Mutex<Option<OnChangeFn>>,
    /// A bundled JS IIFE waiting to be evaluated on the app thread
    /// (QuickJS is not run from the HTTP or watcher threads).
    pub pending_js: Mutex<Option<String>>,
    /// Hook that actually evaluates the bundle (set only when `--js` is on).
    pub apply_js: Mutex<Option<ApplyJsFn>>,
    /// Result of the last source write-back attempted after a patch.
    pub last_writeback: Mutex<Option<writeback::WritebackReport>>,
    /// Opt-in JSON store. `None` means the app is not recording history.
    pub history: Mutex<Option<Arc<Mutex<Store<Value>>>>>,
}

impl SharedTreeHandle {
    /// Run a routed action under the same gate as reload and patch. The router
    /// may update non-click state and returns the node to activate, if any.
    pub fn route_action(
        &self,
        route: impl FnOnce(&mut Tree) -> (bool, Option<NodeId>),
    ) -> Result<bool, McpError> {
        self.route_action_with_caret(|tree| {
            let (changed, id) = route(tree);
            (changed, id, None)
        })
    }

    /// Route an action and apply a host-computed field caret after activation.
    pub fn route_action_with_caret(
        &self,
        route: impl FnOnce(&mut Tree) -> (bool, Option<NodeId>, Option<usize>),
    ) -> Result<bool, McpError> {
        self.route_activated_action(|tree| {
            let (changed, id, caret) = route(tree);
            if let Some(id) = id {
                tree.click(id).map_err(|e| McpError::msg(e.to_string()))?;
                if let (Some(caret), Some(node)) = (caret, tree.get_mut(id)) {
                    node.caret = caret.min(node.text.as_deref().unwrap_or("").chars().count());
                }
            }
            Ok((changed, id))
        })
    }

    /// Run a route that performs its own activation under the action gate.
    pub fn route_activated_action(
        &self,
        route: impl FnOnce(&mut Tree) -> Result<(bool, Option<NodeId>), McpError>,
    ) -> Result<bool, McpError> {
        let _gate = self
            .action_gate
            .lock()
            .map_err(|e| McpError::msg(e.to_string()))?;
        let (changed, id) = {
            let mut tree = self.tree.lock().map_err(|e| McpError::msg(e.to_string()))?;
            let (changed, id) = route(&mut tree)?;
            (changed, id)
        };
        if let Some(id) = id {
            self.log(format!("clicked #{id}"));
        }
        if changed || id.is_some() {
            self.notify_change();
        }
        if let Some(id) = id {
            let hook = self
                .click_js
                .lock()
                .map_err(|e| McpError::msg(e.to_string()))?
                .clone();
            if let Some(hook) = hook {
                hook(id).map_err(McpError::msg)?;
            }
        }
        Ok(changed || id.is_some())
    }

    fn notify_change(&self) {
        if let Ok(cb) = self.on_change.lock() {
            if let Some(f) = cb.as_ref() {
                f();
            }
        }
    }

    pub fn log(&self, line: impl Into<String>) {
        if let Ok(mut logs) = self.logs.lock() {
            logs.push(line.into());
        }
    }

    /// Queue a JS bundle to be applied on the app thread, then wake the loop.
    pub fn enqueue_js(&self, source: String) -> Result<(), McpError> {
        {
            let hook = self
                .apply_js
                .lock()
                .map_err(|e| McpError::msg(e.to_string()))?;
            if hook.is_none() {
                return Err(McpError::msg(
                    "no JS runtime attached — start with `tekto dev --js <entry.tsx>`",
                ));
            }
        }
        if let Ok(mut slot) = self.pending_js.lock() {
            *slot = Some(source);
        }
        self.notify_change();
        Ok(())
    }

    pub fn take_pending_js(&self) -> Option<String> {
        self.pending_js.lock().ok().and_then(|mut slot| slot.take())
    }

    /// Run on the app / event-loop thread (QuickJS runtimes stay off HTTP threads).
    /// Returns true when a bundle was evaluated successfully.
    pub fn apply_pending_js(&self) -> bool {
        let Some(js) = self.take_pending_js() else {
            return false;
        };
        let hook = self.apply_js.lock().ok().and_then(|slot| slot.clone());
        match hook {
            Some(apply) => {
                if let Err(e) = apply(&js) {
                    let msg = format!("js reload failed: {e}");
                    self.log(msg.clone());
                    eprintln!("{msg}");
                    false
                } else {
                    true
                }
            }
            None => {
                let msg = "js reload ignored: no JS runtime attached".to_string();
                self.log(msg.clone());
                eprintln!("{msg}");
                false
            }
        }
    }
}

impl AppHandle for SharedTreeHandle {
    fn tree(&self) -> Tree {
        self.tree.lock().expect("tree lock").clone()
    }

    fn screenshot_png(&self) -> Result<Vec<u8>, McpError> {
        let tree = self.tree();
        (self.render)(&tree)
    }

    fn patch(&self, id: NodeId, patch: NodePatch) -> Result<(), McpError> {
        let _gate = self
            .action_gate
            .lock()
            .map_err(|e| McpError::msg(e.to_string()))?;
        let mut tree = self.tree.lock().expect("tree lock");
        let snapshot = tree
            .get(id)
            .map(writeback::Snapshot::from_node)
            .ok_or_else(|| McpError::msg(format!("unknown node {id}")))?;
        tree.patch(id, patch.clone())
            .map_err(|e| McpError::msg(e.to_string()))?;
        drop(tree);
        let report = writeback::apply(&snapshot, &patch);
        self.log(report.log_line());
        if let Ok(mut slot) = self.last_writeback.lock() {
            *slot = Some(report);
        }
        self.notify_change();
        Ok(())
    }

    fn click(&self, id: NodeId) -> Result<(), McpError> {
        self.route_action(|_| (false, Some(id))).map(|_| ())
    }

    fn key(&self, key: NamedActionKey) -> Result<(), McpError> {
        self.route_action(|tree| {
            let result = route_named_key(tree, key, false);
            if key == NamedActionKey::Space && result.1.is_none() {
                (tree.type_keyboard_text(" ").unwrap_or(false), None)
            } else {
                result
            }
        })
        .map(|_| ())
    }

    fn scroll(&self, id: NodeId, delta: f32) -> Result<f32, McpError> {
        if !delta.is_finite() {
            return Err(McpError::msg("delta must be finite"));
        }
        let _gate = self
            .action_gate
            .lock()
            .map_err(|e| McpError::msg(e.to_string()))?;
        let offset = self
            .tree
            .lock()
            .map_err(|e| McpError::msg(e.to_string()))?
            .scroll_by(id, delta)
            .map_err(|e| McpError::msg(e.to_string()))?;
        self.notify_change();
        Ok(offset)
    }

    fn type_text(&self, id: NodeId, text: &str) -> Result<(), McpError> {
        let _gate = self
            .action_gate
            .lock()
            .map_err(|e| McpError::msg(e.to_string()))?;
        let mut tree = self.tree.lock().expect("tree lock");
        tree.insert_text(id, text)
            .map_err(|e| McpError::msg(e.to_string()))?;
        drop(tree);
        if let Ok(mut logs) = self.logs.lock() {
            logs.push(format!("typed into #{id}"));
        }
        self.notify_change();
        Ok(())
    }

    fn state_json(&self) -> serde_json::Value {
        let tree = self.tree.lock().expect("tree lock");
        serde_json::json!({
            "focus": tree.focus.map(|id| id.raw()),
            "last_click": tree.last_click.map(|id| id.raw()),
        })
    }

    fn states_json(&self) -> Value {
        states_value(&self.history)
    }

    fn restore_state(&self, index: u64) -> Result<Value, McpError> {
        let slot = self
            .history
            .lock()
            .map_err(|e| McpError::msg(e.to_string()))?;
        let Some(store) = slot.as_ref() else {
            return Err(McpError::msg(
                "time travel is off — opt in with Store::recording and attach it to the dev handle",
            ));
        };
        let mut store = store.lock().map_err(|e| McpError::msg(e.to_string()))?;
        let value = store
            .jump(index)
            .map_err(|e| McpError::msg(e.to_string()))?
            .clone();
        drop(store);
        drop(slot);
        self.notify_change();
        Ok(value)
    }

    fn logs(&self) -> Vec<String> {
        self.logs.lock().expect("logs lock").clone()
    }

    fn queue_js(&self, source: String) -> Result<(), McpError> {
        self.enqueue_js(source)
    }

    fn last_writeback(&self) -> Option<Value> {
        self.last_writeback
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|r| r.to_json()))
    }
}

/// Default port for the Android dev server, so the host can `adb forward`
/// without reading a file from the device.
pub const ANDROID_DEV_PORT: u16 = 8474;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DevEndpoint {
    pub port: u16,
    pub host: String,
}

impl DevEndpoint {
    pub fn base_url(&self) -> String {
        let host = if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!("http://{}:{}", host, self.port)
    }
}

pub fn endpoint_path() -> PathBuf {
    if let Ok(p) = std::env::var("TEKTO_DEV_FILE") {
        return PathBuf::from(p);
    }
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return Path::new(&dir).join("tekto-dev.json");
    }
    std::env::temp_dir().join("tekto-dev.json")
}

pub fn write_endpoint(ep: &DevEndpoint) -> Result<(), McpError> {
    let path = endpoint_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, serde_json::to_vec_pretty(ep)?)
        .map_err(|e| McpError::msg(format!("could not write {}: {e}", path.display())))?;
    Ok(())
}

pub fn read_endpoint() -> Result<DevEndpoint, McpError> {
    let path = endpoint_path();
    let data = std::fs::read_to_string(&path).map_err(|_| {
        McpError::msg(format!(
            "no dev server file at {} — start an app with `tekto dev` first",
            path.display()
        ))
    })?;
    Ok(serde_json::from_str(&data)?)
}

pub fn clear_endpoint() {
    let _ = std::fs::remove_file(endpoint_path());
}

fn listen_addr() -> (String, u16) {
    let host = std::env::var("TEKTO_DEV_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("TEKTO_DEV_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(if cfg!(target_os = "android") {
            ANDROID_DEV_PORT
        } else {
            0
        });
    (host, port)
}

/// Start a localhost HTTP server on an ephemeral port
/// (or `TEKTO_DEV_PORT` / [`ANDROID_DEV_PORT`] on Android).
pub fn start_dev_server(handle: Arc<dyn AppHandle>) -> Result<DevEndpoint, McpError> {
    let (host, port) = listen_addr();
    let listener = TcpListener::bind((host.as_str(), port))
        .map_err(|e| McpError::msg(format!("could not bind dev server on {host}:{port}: {e}")))?;
    listener.set_nonblocking(false)?;
    let port = listener.local_addr()?.port();
    let ep = DevEndpoint { port, host };
    // On Android the host talks over `adb forward`; a local endpoint file
    // may not be writable and is not required.
    if let Err(e) = write_endpoint(&ep) {
        eprintln!("Tekto: {e} (dev server is still listening)");
    }
    let running = Arc::new(AtomicBool::new(true));
    thread::Builder::new()
        .name("tekto-dev".into())
        .spawn(move || {
            serve(listener, handle, running);
        })?;
    Ok(ep)
}

// Absolute deadlines stop clients that send one byte just before each timeout.
// At most eight requests (and their buffers) may be in flight; excess sockets close.
const MAX_CLIENTS: usize = 8;
const IO_DEADLINE: Duration = Duration::from_secs(5);

fn serve(listener: TcpListener, handle: Arc<dyn AppHandle>, running: Arc<AtomicBool>) {
    let active = Arc::new(AtomicUsize::new(0));
    // Keep multi-step HTTP operations, including patch and writeback, in order.
    // Clicks use the handle action gate instead, so callbacks run outside this lock.
    let dispatch_lock = Arc::new(Mutex::new(()));
    for stream in listener.incoming() {
        if !running.load(Ordering::Relaxed) {
            break;
        }
        if let Ok(stream) = stream {
            // Only this accept loop increments; workers only decrement.
            if active.load(Ordering::Acquire) >= MAX_CLIENTS {
                continue;
            }
            active.fetch_add(1, Ordering::AcqRel);
            let handle = Arc::clone(&handle);
            let count = Arc::clone(&active);
            let lock = Arc::clone(&dispatch_lock);
            if thread::Builder::new()
                .name("tekto-dev-client".into())
                .spawn(move || {
                    struct Slot(Arc<AtomicUsize>);
                    impl Drop for Slot {
                        fn drop(&mut self) {
                            self.0.fetch_sub(1, Ordering::Release);
                        }
                    }
                    let _slot = Slot(count);
                    let _ = handle_client_locked(stream, handle, &lock);
                })
                .is_err()
            {
                active.fetch_sub(1, Ordering::Release);
            }
        }
    }
}

struct DeadlineReader<'a> {
    stream: &'a mut TcpStream,
    deadline: Instant,
}
impl Read for DeadlineReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "request deadline"))?;
        self.stream.set_read_timeout(Some(left))?;
        self.stream.read(buf)
    }
}

fn write_before(
    stream: &mut TcpStream,
    mut bytes: &[u8],
    deadline: Instant,
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "response deadline")
            })?;
        stream.set_write_timeout(Some(left))?;
        let n = stream.write(bytes)?;
        if n == 0 {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        bytes = &bytes[n..];
    }
    Ok(())
}

fn read_http_request(stream: &mut impl Read) -> Result<Option<(String, String, String)>, McpError> {
    let mut data = Vec::new();
    let mut tmp = [0u8; 2048];
    let header_end = loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(McpError::msg("empty HTTP request"));
        }
        data.extend_from_slice(&tmp[..n]);
        if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            if pos > 64 * 1024 {
                return Err(McpError::msg("request headers too large"));
            }
            break pos;
        }
        if data.len() > 64 * 1024 {
            return Err(McpError::msg("request headers too large"));
        }
    };
    let headers = String::from_utf8_lossy(&data[..header_end]).into_owned();
    if !http_policy::trusted(&headers) {
        return Ok(None);
    }
    let mut content_len = 0usize;
    for line in headers.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_len = v.trim().parse().unwrap_or(0);
        }
    }
    let body_start = header_end + 4;
    if content_len > 2 * 1024 * 1024 - body_start {
        return Err(McpError::msg("request body too large"));
    }
    while data.len() < body_start + content_len {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(McpError::msg("incomplete HTTP request body"));
        }
        data.extend_from_slice(&tmp[..n]);
        if data.len() > 2 * 1024 * 1024 {
            return Err(McpError::msg("request body too large"));
        }
    }
    let request_line = headers.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let end = (body_start + content_len).min(data.len());
    let body = String::from_utf8_lossy(&data[body_start..end]).into_owned();
    Ok(Some((method, path, body)))
}

#[cfg(test)]
fn handle_client(stream: TcpStream, handle: Arc<dyn AppHandle>) -> Result<(), McpError> {
    handle_client_locked(stream, handle, &Mutex::new(()))
}

fn handle_client_locked(
    mut stream: TcpStream,
    handle: Arc<dyn AppHandle>,
    lock: &Mutex<()>,
) -> Result<(), McpError> {
    let request = read_http_request(&mut DeadlineReader {
        stream: &mut stream,
        deadline: Instant::now() + IO_DEADLINE,
    })?;
    let (status, content_type, body_bytes) = if matches!(&request, Some((method, path, _)) if method == "POST" && (path == "/click" || path == "/key" || path == "/scroll"))
    {
        process_http_request(handle.as_ref(), request)
    } else {
        let _guard = lock.lock().map_err(|e| McpError::msg(e.to_string()))?;
        process_http_request(handle.as_ref(), request)
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body_bytes.len()
    );
    let deadline = Instant::now() + IO_DEADLINE;
    write_before(&mut stream, header.as_bytes(), deadline)?;
    write_before(&mut stream, &body_bytes, deadline)?;
    Ok(())
}

fn process_http_request(
    handle: &dyn AppHandle,
    request: Option<(String, String, String)>,
) -> (&'static str, &'static str, Vec<u8>) {
    let Some((method, path, body)) = request else {
        return (
            "403 Forbidden",
            "application/json",
            br#"{"error":"untrusted host or origin"}"#.to_vec(),
        );
    };
    dispatch(handle, &method, &path, &body).unwrap_or_else(error_response)
}

fn states_value(history: &Mutex<Option<Arc<Mutex<Store<Value>>>>>) -> Value {
    let Ok(slot) = history.lock() else {
        return json!({ "enabled": false, "cursor": 0, "states": [] });
    };
    let Some(store) = slot.as_ref() else {
        return json!({ "enabled": false, "cursor": 0, "states": [] });
    };
    let Ok(store) = store.lock() else {
        return json!({ "enabled": false, "cursor": 0, "states": [] });
    };
    let states: Vec<Value> = store
        .list()
        .into_iter()
        .map(|info| {
            json!({
                "index": info.index,
                "label": info.label,
                "current": info.current,
            })
        })
        .collect();
    json!({
        "enabled": store.is_recording(),
        "cursor": store.cursor(),
        "states": states,
    })
}

fn error_response(e: McpError) -> (&'static str, &'static str, Vec<u8>) {
    let msg = e.to_string();
    let status = if msg.contains("unknown node") {
        "404 Not Found"
    } else {
        "400 Bad Request"
    };
    let body = serde_json::to_vec(&serde_json::json!({ "error": msg }))
        .unwrap_or_else(|_| br#"{"error":"internal"}"#.to_vec());
    (status, "application/json", body)
}

fn dispatch(
    handle: &dyn AppHandle,
    method: &str,
    path: &str,
    body: &str,
) -> Result<(&'static str, &'static str, Vec<u8>), McpError> {
    match (method, path) {
        ("GET", "/health") => ok_json(json!({"ok": true})),
        ("GET", "/inspector") => Ok((
            "200 OK",
            "text/html; charset=utf-8",
            include_bytes!("inspector.html").to_vec(),
        )),
        ("GET", "/tree") => {
            let tree = handle.tree();
            ok_json(json!({
                "pretty": tree.to_pretty_text(),
                "dump": tree.to_dump(),
            }))
        }
        ("GET", "/tree.txt") => {
            let text = handle.tree().to_pretty_text();
            Ok(("200 OK", "text/plain; charset=utf-8", text.into_bytes()))
        }
        ("GET", "/screenshot") => {
            let png = handle.screenshot_png()?;
            Ok(("200 OK", "image/png", png))
        }
        ("GET", "/accesskit") => {
            let tree = handle.tree();
            let dump = tekto_a11y::dump(&tree).ok_or_else(|| McpError::msg("empty tree"))?;
            ok_json(serde_json::to_value(dump)?)
        }
        ("GET", "/state") => ok_json(handle.state_json()),
        ("GET", "/states") => ok_json(handle.states_json()),
        ("POST", "/states/restore") => {
            let v: Value = serde_json::from_str(if body.is_empty() { "{}" } else { body })?;
            let index = v
                .get("index")
                .and_then(|x| x.as_u64())
                .ok_or_else(|| McpError::msg("missing index"))?;
            let value = handle.restore_state(index)?;
            ok_json(json!({
                "ok": true,
                "index": index,
                "value": value,
                "states": handle.states_json(),
            }))
        }
        ("GET", "/logs") => ok_json(json!({ "logs": handle.logs() })),
        ("GET", "/components") => ok_json(components_value()?),
        ("POST", "/patch") => {
            let v: Value = serde_json::from_str(if body.is_empty() { "{}" } else { body })?;
            let id = v
                .get("id")
                .and_then(|x| x.as_u64())
                .ok_or_else(|| McpError::msg("missing id"))?;
            let patch: NodePatch = serde_json::from_value(v)?;
            handle.patch(NodeId::from_raw(id), patch)?;
            let tree = handle.tree();
            let node = tree
                .node_dump(NodeId::from_raw(id))
                .ok_or_else(|| McpError::msg(format!("node {id} missing after patch")))?;
            let mut body = json!({"ok": true, "node": node});
            if let Some(wb) = handle.last_writeback() {
                body["writeback"] = wb;
            }
            ok_json(body)
        }
        ("POST", "/click") => {
            let v: Value = serde_json::from_str(if body.is_empty() { "{}" } else { body })?;
            let id = v
                .get("id")
                .and_then(|x| x.as_u64())
                .ok_or_else(|| McpError::msg("missing id"))?;
            handle.click(NodeId::from_raw(id))?;
            ok_json(json!({"ok": true}))
        }
        ("POST", "/key") => {
            let v: Value = serde_json::from_str(if body.is_empty() { "{}" } else { body })?;
            let name = v
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| McpError::msg("missing key name"))?;
            let key = name.parse().map_err(McpError::msg)?;
            handle.key(key)?;
            ok_json(json!({"ok": true}))
        }
        ("POST", "/scroll") => {
            let v: Value = serde_json::from_str(if body.is_empty() { "{}" } else { body })?;
            let id = v
                .get("id")
                .and_then(Value::as_u64)
                .ok_or_else(|| McpError::msg("missing or invalid id"))?;
            let delta = v
                .get("delta")
                .and_then(Value::as_f64)
                .ok_or_else(|| McpError::msg("missing or invalid delta"))?
                as f32;
            if !delta.is_finite() {
                return Err(McpError::msg("delta must be finite"));
            }
            let offset = handle.scroll(NodeId::from_raw(id), delta)?;
            ok_json(json!({"ok": true, "offset": offset}))
        }
        ("POST", "/type") => {
            let v: Value = serde_json::from_str(if body.is_empty() { "{}" } else { body })?;
            let id = v
                .get("id")
                .and_then(|x| x.as_u64())
                .ok_or_else(|| McpError::msg("missing id"))?;
            let text = v.get("text").and_then(|x| x.as_str()).unwrap_or("");
            handle.type_text(NodeId::from_raw(id), text)?;
            ok_json(json!({"ok": true}))
        }
        ("POST", "/reload") => {
            let source = parse_reload_body(body)?;
            handle.queue_js(source)?;
            ok_json(json!({"ok": true}))
        }
        _ => Ok((
            "404 Not Found",
            "application/json",
            serde_json::to_vec(&json!({"error": "not found"}))?,
        )),
    }
}

pub fn parse_reload_body(body: &str) -> Result<String, McpError> {
    let body = body.trim();
    if body.is_empty() {
        return Err(McpError::msg(
            "POST /reload needs a JS bundle body (or JSON {\"js\": \"...\"})",
        ));
    }
    if body.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<Value>(body) {
            if let Some(js) = v.get("js").and_then(|x| x.as_str()) {
                return Ok(js.to_string());
            }
        }
    }
    Ok(body.to_string())
}

fn ok_json(v: Value) -> Result<(&'static str, &'static str, Vec<u8>), McpError> {
    Ok(("200 OK", "application/json", serde_json::to_vec_pretty(&v)?))
}

/// HTTP client used by the CLI (stdlib only, keeps the crate graph lean).
pub fn http_get(url: &str) -> Result<(String, Vec<u8>), McpError> {
    http_request("GET", url, None)
}

pub fn http_post(url: &str, body: &Value) -> Result<(String, Vec<u8>), McpError> {
    http_request("POST", url, Some(body))
}

/// POST a raw body (used by `POST /reload` to push a JS bundle).
pub fn http_post_raw(
    url: &str,
    body: &[u8],
    content_type: &str,
) -> Result<(String, Vec<u8>), McpError> {
    http_request_bytes("POST", url, Some(body), content_type)
}

fn http_request(
    method: &str,
    url: &str,
    body: Option<&Value>,
) -> Result<(String, Vec<u8>), McpError> {
    let payload = body.map(serde_json::to_vec).transpose()?;
    let ct = if body.is_some() {
        "application/json"
    } else {
        "application/octet-stream"
    };
    http_request_bytes(method, url, payload.as_deref(), ct)
}

fn http_request_bytes(
    method: &str,
    url: &str,
    body: Option<&[u8]>,
    content_type: &str,
) -> Result<(String, Vec<u8>), McpError> {
    let url = url
        .strip_prefix("http://")
        .ok_or_else(|| McpError::msg("only http:// is supported"))?;
    let (host, path) = url.split_once('/').unwrap_or((url, ""));
    let path = format!("/{path}");
    let (host_only, port) = if let Some(rest) = host.strip_prefix('[') {
        let (ip, suffix) = rest
            .split_once(']')
            .ok_or_else(|| McpError::msg("invalid IPv6 URL"))?;
        let port = if suffix.is_empty() {
            80
        } else {
            suffix
                .strip_prefix(':')
                .and_then(|p| p.parse().ok())
                .ok_or_else(|| McpError::msg("invalid port"))?
        };
        (ip, port)
    } else if let Some((h, p)) = host.split_once(':') {
        (h, p.parse().unwrap_or(80))
    } else {
        (host, 80u16)
    };
    let mut stream = TcpStream::connect((host_only, port))?;
    let payload = body.unwrap_or(b"");
    let extra = if body.is_some() {
        format!(
            "Content-Type: {content_type}\r\nContent-Length: {}\r\n",
            payload.len()
        )
    } else {
        String::new()
    };
    let req =
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{extra}\r\n");
    // One write so the server cannot reply before the POST body arrives.
    let mut msg = req.into_bytes();
    msg.extend_from_slice(payload);
    stream.write_all(&msg)?;
    let mut resp = Vec::new();
    stream.read_to_end(&mut resp)?;
    let header_end = resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| McpError::msg("bad HTTP response"))?;
    let headers = String::from_utf8_lossy(&resp[..header_end]).to_string();
    let body = resp[header_end + 4..].to_vec();
    let status = headers.lines().next().unwrap_or("").to_string();
    if !status.contains("200") {
        return Err(McpError::msg(format!(
            "{status}: {}",
            String::from_utf8_lossy(&body)
        )));
    }
    Ok((headers, body))
}

/// Run a stdio MCP server that proxies to the dev HTTP server.
pub fn run_stdio_mcp() -> Result<(), McpError> {
    stdio::run(std::io::stdin().lock(), std::io::stdout().lock())
}

fn tool_list() -> Value {
    json!([
        {"name": "ui.tree", "description": "Dump the AccessKit / scene tree with stable node ids.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "ui.accesskit", "description": "Dump the AccessKit tree (roles, labels, bounds, focus) with the same stable ids.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "ui.screenshot", "description": "Take a PNG screenshot using the CPU renderer.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "ui.click", "description": "Click a node by id.", "inputSchema": {"type": "object", "properties": {"id": {"type": "number"}}, "required": ["id"]}},
        {"name": "ui.scroll", "description": "Scroll a container by logical pixels and return the clamped offset.", "inputSchema": {"type": "object", "properties": {"id": {"type": "integer"}, "delta": {"type": "number"}}, "required": ["id", "delta"]}},
        {"name": "ui.key", "description": "Press a named key on the focused node.", "inputSchema": {"type": "object", "properties": {"name": {"type": "string", "enum": ["Tab", "Shift+Tab", "Enter", "Space", "Backspace"]}}, "required": ["name"]}},
        {"name": "ui.type", "description": "Insert plain characters at the caret of a text field (by stable id) and focus it. Not an IME: no composition or candidate window. Use ui.key for Backspace.", "inputSchema": {"type": "object", "properties": {"id": {"type": "number"}, "text": {"type": "string"}}, "required": ["id", "text"]}},
        {"name": "ui.patch", "description": "Live-edit a node by stable id: text, name, fill, color, fontSize, padding, gap, or disabled (buttons and text fields). Returns the updated node and a writeback report. Applies immediately (no reload). Unique literals in the app project are written back to source; ambiguous edits stay in the runtime tree.", "inputSchema": {"type": "object", "properties": {"id": {"type": "number"}, "disabled": {"type": "boolean"}, "text": {"type": "string"}, "name": {"type": "string"}, "fill": {"type": "string"}, "color": {"type": "string"}, "fontSize": {"type": "number"}, "font_size": {"type": "number"}, "padding": {"type": "number"}, "gap": {"type": "number"}}, "required": ["id"]}},
        {"name": "app.state", "description": "Read opted-in app state as JSON.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "app.states", "description": "List time-travel frames for an opted-in store. enabled is false when the app did not record history.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "app.restore", "description": "Jump an opted-in store to a recorded index from app.states. Returns the restored JSON value.", "inputSchema": {"type": "object", "properties": {"index": {"type": "number"}}, "required": ["index"]}},
        {"name": "app.logs", "description": "Read structured logs.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "app.components", "description": "JSON schema of JSX components: kinds, props, and events. Same file the JSX runtime mounts. Does not need a running app.", "inputSchema": {"type": "object", "properties": {}}}
    ])
}

/// Shared component catalog (`packages/jsx-runtime/src/components.json`).
/// The JSX runtime imports this file. `app.components` returns it.
pub const COMPONENTS_SCHEMA: &str =
    include_str!("../../../packages/jsx-runtime/src/components.json");

fn components_value() -> Result<Value, McpError> {
    Ok(serde_json::from_str(COMPONENTS_SCHEMA)?)
}

fn call_tool(params: &Value) -> Result<Value, McpError> {
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if name == "app.components" {
        return Ok(json!({"content": [{"type": "text", "text": COMPONENTS_SCHEMA}]}));
    }
    let ep = read_endpoint()?;
    let base = ep.base_url();
    match name {
        "ui.tree" => {
            let (_, body) = http_get(&format!("{base}/tree"))?;
            let text = String::from_utf8_lossy(&body).into_owned();
            Ok(json!({"content": [{"type": "text", "text": text}]}))
        }
        "ui.accesskit" => {
            let (_, body) = http_get(&format!("{base}/accesskit"))?;
            let text = String::from_utf8_lossy(&body).into_owned();
            Ok(json!({"content": [{"type": "text", "text": text}]}))
        }
        "ui.screenshot" => {
            let (_, body) = http_get(&format!("{base}/screenshot"))?;
            let b64 = b64(&body);
            Ok(json!({"content": [{"type": "image", "data": b64, "mimeType": "image/png"}]}))
        }
        "ui.patch" => {
            let (_, body) = http_post(&format!("{base}/patch"), &args)?;
            Ok(json!({"content": [{"type": "text", "text": String::from_utf8_lossy(&body)}]}))
        }
        "ui.click" => {
            let (_, body) = http_post(&format!("{base}/click"), &args)?;
            Ok(json!({"content": [{"type": "text", "text": String::from_utf8_lossy(&body)}]}))
        }
        "ui.scroll" => {
            let (_, body) = http_post(&format!("{base}/scroll"), &args)?;
            Ok(json!({"content": [{"type": "text", "text": String::from_utf8_lossy(&body)}]}))
        }
        "ui.key" => {
            let (_, body) = http_post(&format!("{base}/key"), &args)?;
            Ok(json!({"content": [{"type": "text", "text": String::from_utf8_lossy(&body)}]}))
        }
        "ui.type" => {
            let (_, body) = http_post(&format!("{base}/type"), &args)?;
            Ok(json!({"content": [{"type": "text", "text": String::from_utf8_lossy(&body)}]}))
        }
        "app.state" => {
            let (_, body) = http_get(&format!("{base}/state"))?;
            Ok(json!({"content": [{"type": "text", "text": String::from_utf8_lossy(&body)}]}))
        }
        "app.states" => {
            let (_, body) = http_get(&format!("{base}/states"))?;
            Ok(json!({"content": [{"type": "text", "text": String::from_utf8_lossy(&body)}]}))
        }
        "app.restore" => {
            let (_, body) = http_post(&format!("{base}/states/restore"), &args)?;
            Ok(json!({"content": [{"type": "text", "text": String::from_utf8_lossy(&body)}]}))
        }
        "app.logs" => {
            let (_, body) = http_get(&format!("{base}/logs"))?;
            Ok(json!({"content": [{"type": "text", "text": String::from_utf8_lossy(&body)}]}))
        }
        _ => Err(McpError::msg(format!("unknown tool {name}"))),
    }
}

fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (a << 16) | (b << 8) | c;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(T[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tekto_tree::{NodeKind, NodePatch};

    #[test]
    fn endpoint_urls_bracket_ipv6() {
        for (host, expected) in [
            ("127.0.0.1", "http://127.0.0.1:42"),
            ("localhost", "http://localhost:42"),
            ("::1", "http://[::1]:42"),
            ("[::1]", "http://[::1]:42"),
        ] {
            assert_eq!(
                DevEndpoint {
                    host: host.into(),
                    port: 42
                }
                .base_url(),
                expected
            );
        }
    }

    #[test]
    fn request_limits_and_incomplete_body_are_enforced() {
        let large_header = format!(
            "GET / HTTP/1.1\r\nHost: localhost\r\nX: {}\r\n\r\n",
            "a".repeat(64 * 1024)
        );
        assert!(read_http_request(&mut large_header.as_bytes()).is_err());
        let large_body =
            "POST /reload HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2097152\r\n\r\n";
        assert!(read_http_request(&mut large_body.as_bytes()).is_err());
        let incomplete = "POST /reload HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\na";
        assert!(read_http_request(&mut incomplete.as_bytes()).is_err());
    }

    #[test]
    fn stalled_clients_timeout_and_capacity_recovers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            serve(
                listener,
                Arc::new(http_test_handle()),
                Arc::new(AtomicBool::new(true)),
            )
        });
        let mut idle = TcpStream::connect(addr).unwrap();
        let mut partial = TcpStream::connect(addr).unwrap();
        write!(
            partial,
            "POST /reload HTTP/1.1\r\nHost: localhost\r\nContent-Length: 10\r\n\r\na"
        )
        .unwrap();
        let start = Instant::now();
        assert!(http_get(&format!("http://{addr}/health")).is_ok());
        assert!(start.elapsed() < Duration::from_secs(2));
        // Fill the remaining slots, then verify overload closes without dispatch.
        let mut stalls = Vec::new();
        for _ in 0..MAX_CLIENTS - 2 {
            stalls.push(TcpStream::connect(addr).unwrap());
        }
        thread::sleep(Duration::from_millis(200));
        let mut excess = TcpStream::connect(addr).unwrap();
        excess
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let _ = excess.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let mut b = [0; 1];
        assert_closed(excess.read(&mut b));
        idle.set_read_timeout(Some(IO_DEADLINE + Duration::from_secs(2)))
            .unwrap();
        assert_closed(idle.read(&mut b));
        partial
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        assert_closed(partial.read(&mut b));
        thread::sleep(Duration::from_millis(200));
        assert!(http_get(&format!("http://{addr}/health")).is_ok());
    }

    fn assert_closed(result: std::io::Result<usize>) {
        match result {
            Ok(0) => (),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                ) => {}
            other => panic!("expected closed connection, got {other:?}"),
        }
    }

    #[test]
    fn trickling_headers_hit_absolute_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            serve(
                listener,
                Arc::new(http_test_handle()),
                Arc::new(AtomicBool::new(true)),
            )
        });
        let mut slow = TcpStream::connect(addr).unwrap();
        for _ in 0..6 {
            if slow.write_all(b"G").is_err() {
                break;
            }
            thread::sleep(Duration::from_secs(1));
        }
        slow.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        assert_closed(slow.read(&mut [0; 1]));
        assert!(http_get(&format!("http://{addr}/health")).is_ok());
    }

    #[test]
    fn slow_reader_does_not_hold_a_slot_forever() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut handle = http_test_handle();
        handle.render = Arc::new(|_| Ok(vec![0; 32 * 1024 * 1024]));
        thread::spawn(move || serve(listener, Arc::new(handle), Arc::new(AtomicBool::new(true))));
        let mut slow = TcpStream::connect(addr).unwrap();
        slow.write_all(b"GET /screenshot HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        assert!(http_get(&format!("http://{addr}/health")).is_ok());
        thread::sleep(IO_DEADLINE + Duration::from_secs(1));
        let mut data = Vec::new();
        slow.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        assert!(slow.read_to_end(&mut data).is_ok());
        assert!(data.len() < 32 * 1024 * 1024);
        assert!(http_get(&format!("http://{addr}/health")).is_ok());
    }

    #[test]
    fn named_key_route_and_callback() {
        let handle = http_test_handle();
        let button = {
            let mut tree = handle.tree.lock().unwrap();
            let root = NodeId::from_raw(1);
            tree.add(NodeKind::Button, Some(root)).unwrap()
        };
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        *handle.click_js.lock().unwrap() = Some(Arc::new(move |id| {
            seen.lock().unwrap().push(id);
            Ok(())
        }));
        for name in ["Tab", "Enter"] {
            let (status, _, _) = process_http_request(
                &handle,
                Some((
                    "POST".into(),
                    "/key".into(),
                    json!({"name": name}).to_string(),
                )),
            );
            assert_eq!(status, "200 OK");
        }
        assert_eq!(*calls.lock().unwrap(), vec![button]);
        assert_eq!(handle.tree().last_click, Some(button));
        for body in ["{}", r#"{"name":"Escape"}"#, "not json"] {
            let (status, _, body) =
                process_http_request(&handle, Some(("POST".into(), "/key".into(), body.into())));
            assert_eq!(status, "400 Bad Request");
            assert!(String::from_utf8(body).unwrap().contains("error"));
        }
        let tools = tool_list();
        let key = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "ui.key")
            .unwrap();
        assert_eq!(key["inputSchema"]["required"], json!(["name"]));
        assert_eq!(
            key["inputSchema"]["properties"]["name"]["enum"],
            json!(["Tab", "Shift+Tab", "Enter", "Space", "Backspace"])
        );
    }

    #[test]
    fn scroll_route_clamps_and_rejects_invalid_input() {
        let handle = http_test_handle();
        let id = {
            let mut tree = handle.tree.lock().unwrap();
            let id = tree
                .add(NodeKind::Scroll, Some(NodeId::from_raw(1)))
                .unwrap();
            let node = tree.get_mut(id).unwrap();
            node.bounds.height = 100.0;
            node.scroll_extent = 350.0;
            id
        };
        let changes = Arc::new(AtomicUsize::new(0));
        let seen = changes.clone();
        *handle.on_change.lock().unwrap() = Some(Arc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
        }));
        let request = |body: String| {
            process_http_request(&handle, Some(("POST".into(), "/scroll".into(), body)))
        };
        for (delta, expected) in [(1000.0, 250.0), (-1000.0, 0.0)] {
            let (status, _, body) = request(json!({"id": id.raw(), "delta": delta}).to_string());
            assert_eq!(status, "200 OK");
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap()["offset"],
                expected
            );
        }
        assert_eq!(changes.load(Ordering::SeqCst), 2);
        for body in [
            "{}".to_string(),
            r#"{"id":1,"delta":3}"#.into(),
            format!(r#"{{"id":{},"delta":1e100}}"#, id.raw()),
            format!(r#"{{"id":{},"delta":"NaN"}}"#, id.raw()),
        ] {
            let (status, _, body) = request(body);
            assert_eq!(status, "400 Bad Request");
            assert!(String::from_utf8(body).unwrap().contains("error"));
        }
        let (status, _, body) = request(r#"{"id":999,"delta":3}"#.into());
        assert_eq!(status, "404 Not Found");
        assert!(String::from_utf8(body).unwrap().contains("unknown node"));
        let tools = tool_list();
        let scroll = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "ui.scroll")
            .unwrap();
        assert_eq!(scroll["inputSchema"]["required"], json!(["id", "delta"]));
    }

    fn http_test_handle() -> SharedTreeHandle {
        let mut tree = Tree::new();
        tree.add(NodeKind::Window, None).unwrap();
        SharedTreeHandle {
            tree: Arc::new(Mutex::new(tree)),
            render: Arc::new(|_| Ok(Vec::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
            on_change: Mutex::new(None),
            pending_js: Mutex::new(None),
            apply_js: Mutex::new(Some(Arc::new(|_| Ok(())))),
            last_writeback: Mutex::new(None),
            action_gate: Arc::new(Mutex::new(())),
            click_js: Mutex::new(None),
            history: Mutex::new(None),
        }
    }

    #[test]
    fn patch_route_accepts_disabled_and_reports_it() {
        let handle = http_test_handle();
        let button = handle
            .tree
            .lock()
            .unwrap()
            .add(NodeKind::Button, None)
            .unwrap();
        let body = format!(r#"{{"id":{},"disabled":true}}"#, button.raw());
        let (status, _, body) =
            process_http_request(&handle, Some(("POST".into(), "/patch".into(), body)));
        assert_eq!(status, "200 OK");
        let response: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(response["node"]["disabled"], true);
        let field = handle
            .tree
            .lock()
            .unwrap()
            .add(NodeKind::TextField, None)
            .unwrap();
        handle
            .patch(
                field,
                NodePatch {
                    disabled: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
        let type_body = format!(r#"{{"id":{},"text":"no"}}"#, field.raw());
        let (status, _, _) =
            process_http_request(&handle, Some(("POST".into(), "/type".into(), type_body)));
        assert_eq!(status, "400 Bad Request");
        let tools = tool_list();
        let patch = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "ui.patch")
            .unwrap();
        assert_eq!(
            patch["inputSchema"]["properties"]["disabled"]["type"],
            "boolean"
        );
    }

    #[test]
    fn http_policy_covers_all_routes_and_trusted_mutations() {
        let handle = http_test_handle();
        for (method, path) in [
            ("GET", "/health"),
            ("GET", "/inspector"),
            ("GET", "/tree"),
            ("GET", "/tree.txt"),
            ("GET", "/screenshot"),
            ("GET", "/accesskit"),
            ("GET", "/state"),
            ("GET", "/states"),
            ("GET", "/logs"),
            ("GET", "/components"),
            ("POST", "/states/restore"),
            ("POST", "/patch"),
            ("POST", "/click"),
            ("POST", "/type"),
            ("POST", "/reload"),
        ] {
            for headers in [
                "Host: evil.test",
                "Host: localhost:3000\r\nOrigin: http://evil.test",
            ] {
                let raw =
                    format!("{method} {path} HTTP/1.1\r\n{headers}\r\nContent-Length: 100\r\n\r\n");
                let request = read_http_request(&mut raw.as_bytes()).unwrap();
                assert_eq!(
                    process_http_request(&handle, request).0,
                    "403 Forbidden",
                    "{path}"
                );
            }
        }
        for (path, body, origin) in [
            (
                "/patch",
                r#"{"id":1,"text":"changed"}"#,
                "Origin: http://localhost:3000\r\n",
            ),
            ("/reload", "console.log(1)", ""),
        ] {
            let raw = format!("POST {path} HTTP/1.1\r\nHost: localhost:3000\r\n{origin}Content-Length: {}\r\n\r\n{body}", body.len());
            let response =
                process_http_request(&handle, read_http_request(&mut raw.as_bytes()).unwrap());
            assert_eq!(
                response.0,
                "200 OK",
                "{path}: {:?}",
                String::from_utf8_lossy(&response.2)
            );
        }
        assert_eq!(
            handle.pending_js.lock().unwrap().as_deref(),
            Some("console.log(1)")
        );
    }

    #[test]
    fn http_socket_integration_response_and_cli() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                handle_client(stream, Arc::new(http_test_handle())).unwrap();
            }
        });
        let (status, _) = http_get(&format!("http://127.0.0.1:{port}/tree")).unwrap();
        assert!(status.contains("200"));
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            stream,
            "GET /state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://evil.test\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 403"));
        assert!(!response
            .to_ascii_lowercase()
            .contains("access-control-allow-origin"));
        worker.join().unwrap();
    }

    #[test]
    fn click_sets_focus_and_is_logged() {
        let mut tree = Tree::new();
        let root = tree.add(NodeKind::Window, None).unwrap();
        let t = tree.add(NodeKind::Text, Some(root)).unwrap();
        let handle = SharedTreeHandle {
            tree: Arc::new(Mutex::new(tree)),
            render: Arc::new(|_| Ok(Vec::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
            on_change: Mutex::new(None),
            pending_js: Mutex::new(None),
            apply_js: Mutex::new(None),
            last_writeback: Mutex::new(None),
            action_gate: Arc::new(Mutex::new(())),
            click_js: Mutex::new(None),
            history: Mutex::new(None),
        };
        handle.click(t).unwrap();
        let dump = handle.tree();
        assert_eq!(dump.focus, Some(t));
        assert_eq!(dump.last_click, Some(t));
        let logs = handle.logs();
        assert!(
            logs.iter().any(|l| l.contains(&format!("clicked #{t}"))),
            "{logs:?}"
        );
        let state = handle.state_json();
        assert_eq!(state["last_click"], t.raw());
        assert_eq!(state["focus"], t.raw());
        let _ = NodePatch::default();
    }

    #[test]
    fn parse_reload_body_accepts_raw_js_and_json() {
        assert_eq!(
            parse_reload_body("tekto.create('window')").unwrap(),
            "tekto.create('window')"
        );
        assert_eq!(
            parse_reload_body(r#"{ "js": "tekto.create('window')" }"#).unwrap(),
            "tekto.create('window')"
        );
        assert!(parse_reload_body("").is_err());
    }

    #[test]
    fn queue_js_errors_without_a_runtime_hook() {
        let mut tree = Tree::new();
        let _ = tree.add(NodeKind::Window, None).unwrap();
        let handle = SharedTreeHandle {
            tree: Arc::new(Mutex::new(tree)),
            render: Arc::new(|_| Ok(Vec::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
            on_change: Mutex::new(None),
            pending_js: Mutex::new(None),
            apply_js: Mutex::new(None),
            last_writeback: Mutex::new(None),
            action_gate: Arc::new(Mutex::new(())),
            click_js: Mutex::new(None),
            history: Mutex::new(None),
        };
        let err = handle
            .enqueue_js("tekto.create('window')".into())
            .unwrap_err();
        assert!(err.to_string().contains("no JS runtime"), "{err}");
    }

    #[test]
    fn states_restore_jumps_an_opted_in_store() {
        let store = Arc::new(Mutex::new(Store::recording(json!({"n": 0}), 8)));
        {
            let mut g = store.lock().unwrap();
            g.set(json!({"n": 1}), Some("one".into()));
            g.set(json!({"n": 2}), None);
            g.set(json!({"n": 3}), Some("three".into()));
        }
        let handle = SharedTreeHandle {
            tree: Arc::new(Mutex::new(Tree::new())),
            render: Arc::new(|_| Ok(Vec::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
            on_change: Mutex::new(None),
            pending_js: Mutex::new(None),
            apply_js: Mutex::new(None),
            last_writeback: Mutex::new(None),
            action_gate: Arc::new(Mutex::new(())),
            click_js: Mutex::new(None),
            history: Mutex::new(Some(store)),
        };
        let (_s, _c, body) = dispatch(&handle, "GET", "/states", "").unwrap();
        let listed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(listed["enabled"], true);
        assert_eq!(listed["cursor"], 3);
        assert_eq!(listed["states"].as_array().unwrap().len(), 4);

        let (_s, _c, body) =
            dispatch(&handle, "POST", "/states/restore", r#"{"index":1}"#).unwrap();
        let restored: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(restored["value"]["n"], 1);
        assert_eq!(restored["states"]["cursor"], 1);

        let (_s, _c, body) =
            dispatch(&handle, "POST", "/states/restore", r#"{"index":3}"#).unwrap();
        let restored: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(restored["value"]["n"], 3);

        let off = SharedTreeHandle {
            tree: Arc::new(Mutex::new(Tree::new())),
            render: Arc::new(|_| Ok(Vec::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
            on_change: Mutex::new(None),
            pending_js: Mutex::new(None),
            apply_js: Mutex::new(None),
            last_writeback: Mutex::new(None),
            action_gate: Arc::new(Mutex::new(())),
            click_js: Mutex::new(None),
            history: Mutex::new(None),
        };
        let listed = off.states_json();
        assert_eq!(listed["enabled"], false);
        assert!(off.restore_state(0).is_err());
    }

    #[test]
    fn app_components_lists_core_widgets() {
        let listed = tool_list();
        assert!(listed
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "app.components"));

        let result = call_tool(&json!({"name": "app.components", "arguments": {}})).unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        assert_eq!(text, COMPONENTS_SCHEMA);
        let schema: Value = serde_json::from_str(text).unwrap();
        let names: Vec<&str> = schema["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();

        let kind_names: Vec<&str> = tekto_tree::NodeKind::all()
            .iter()
            .map(|k| k.as_str())
            .collect();
        assert_eq!(
            names, kind_names,
            "components.json must list every NodeKind and nothing else"
        );

        // Events come from NodeKind::agent_events (what the host actually
        // does), not a shorter hand-written list. Prop completeness is
        // tekto_js::component_schema_lists_every_host_prop: every setProp key
        // must appear on every kind, because JSX forwards that set globally.
        for kind in tekto_tree::NodeKind::all() {
            let widget = kind.as_str();
            let spec = schema["components"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == widget)
                .unwrap_or_else(|| panic!("schema omits core widget {widget}"));
            let got_events: Vec<&str> = spec["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["name"].as_str().unwrap())
                .collect();
            for event in kind.agent_events() {
                assert!(
                    got_events.contains(event),
                    "{widget} schema omits event {event}"
                );
            }
            for event in &got_events {
                assert!(
                    kind.agent_events().contains(event),
                    "{widget} schema lists event {event} the host does not apply to this kind"
                );
            }
        }

        let (_status, _ct, body) = dispatch(
            &SharedTreeHandle {
                tree: Arc::new(Mutex::new(Tree::new())),
                render: Arc::new(|_| Ok(Vec::new())),
                logs: Arc::new(Mutex::new(Vec::new())),
                on_change: Mutex::new(None),
                pending_js: Mutex::new(None),
                apply_js: Mutex::new(None),
                last_writeback: Mutex::new(None),
                action_gate: Arc::new(Mutex::new(())),
                click_js: Mutex::new(None),
                history: Mutex::new(None),
            },
            "GET",
            "/components",
            "",
        )
        .unwrap();
        let http: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(http["components"], schema["components"]);
    }
}
