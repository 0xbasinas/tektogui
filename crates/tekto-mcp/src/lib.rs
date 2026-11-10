//! Dev server shared by the MCP tools and the `tekto` CLI.
//!
//! The running app listens on a local HTTP port and writes the port to a
//! small JSON file. `tekto tree`, `tekto screenshot`, and the stdio MCP
//! adapter all talk to that same server.
//!
//! Live edits (`POST /patch`) change node props in the running tree. Writing
//! those edits back to source files is a separate path (`ui.patch` write-back).
//! `POST /reload` accepts a JS bundle and queues it for the JS runtime.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tekto_tree::{NodeId, NodePatch, Tree};

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

/// What the running app can do for agents.
pub trait AppHandle: Send + Sync {
    fn tree(&self) -> Tree;
    fn screenshot_png(&self) -> Result<Vec<u8>, McpError>;
    fn patch(&self, id: NodeId, patch: NodePatch) -> Result<(), McpError>;
    fn click(&self, id: NodeId) -> Result<(), McpError> {
        let _ = id;
        Err(McpError::msg("click is not wired up yet"))
    }
    fn type_text(&self, id: NodeId, text: &str) -> Result<(), McpError> {
        let _ = (id, text);
        Err(McpError::msg("type is not wired up yet"))
    }
    fn state_json(&self) -> Value {
        json!({})
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
}

/// PNG renderer used by [`SharedTreeHandle`].
pub type RenderFn = Arc<dyn Fn(&Tree) -> Result<Vec<u8>, McpError> + Send + Sync>;

/// A simple handle over a mutex-protected tree plus a render callback.
pub type OnChangeFn = Arc<dyn Fn() + Send + Sync>;

/// Evaluates a JS bundle and swaps the tree. Installed by `tekto dev --js`.
pub type ApplyJsFn = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

pub struct SharedTreeHandle {
    pub tree: Arc<Mutex<Tree>>,
    pub render: RenderFn,
    pub logs: Arc<Mutex<Vec<String>>>,
    /// Called after a live patch (and later, clicks) so a window can redraw
    /// and refresh AccessKit. Also used to wake the loop for a JS reload.
    pub on_change: Mutex<Option<OnChangeFn>>,
    /// A bundled JS IIFE waiting to be evaluated on the app thread
    /// (QuickJS is not run from the HTTP or watcher threads).
    pub pending_js: Mutex<Option<String>>,
    /// Hook that actually evaluates the bundle (set only when `--js` is on).
    pub apply_js: Mutex<Option<ApplyJsFn>>,
}

impl SharedTreeHandle {
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
        let mut tree = self.tree.lock().expect("tree lock");
        tree.patch(id, patch)
            .map_err(|e| McpError::msg(e.to_string()))?;
        drop(tree);
        self.notify_change();
        Ok(())
    }

    fn click(&self, id: NodeId) -> Result<(), McpError> {
        let mut tree = self.tree.lock().expect("tree lock");
        tree.click(id).map_err(|e| McpError::msg(e.to_string()))?;
        drop(tree);
        if let Ok(mut logs) = self.logs.lock() {
            logs.push(format!("clicked #{id}"));
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

    fn logs(&self) -> Vec<String> {
        self.logs.lock().expect("logs lock").clone()
    }

    fn queue_js(&self, source: String) -> Result<(), McpError> {
        self.enqueue_js(source)
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
        format!("http://{}:{}", self.host, self.port)
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
            for stream in listener.incoming() {
                if !running.load(Ordering::Relaxed) {
                    break;
                }
                if let Ok(stream) = stream {
                    let handle = Arc::clone(&handle);
                    let _ = handle_client(stream, handle);
                }
            }
        })?;
    Ok(ep)
}

fn read_http_request(stream: &mut TcpStream) -> Result<(String, String, String), McpError> {
    let mut data = Vec::new();
    let mut tmp = [0u8; 2048];
    let header_end = loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(McpError::msg("empty HTTP request"));
        }
        data.extend_from_slice(&tmp[..n]);
        if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        if data.len() > 64 * 1024 {
            return Err(McpError::msg("request headers too large"));
        }
    };
    let headers = String::from_utf8_lossy(&data[..header_end]).into_owned();
    let mut content_len = 0usize;
    for line in headers.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_len = v.trim().parse().unwrap_or(0);
        }
    }
    let body_start = header_end + 4;
    while data.len() < body_start + content_len {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
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
    Ok((method, path, body))
}

fn handle_client(mut stream: TcpStream, handle: Arc<dyn AppHandle>) -> Result<(), McpError> {
    let (method, path, body) = read_http_request(&mut stream)?;
    let (status, content_type, body_bytes) = match dispatch(handle.as_ref(), &method, &path, &body)
    {
        Ok(v) => v,
        Err(e) => error_response(e),
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body_bytes.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(&body_bytes)?;
    Ok(())
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
        ("GET", "/logs") => ok_json(json!({ "logs": handle.logs() })),
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
            ok_json(json!({"ok": true, "node": node}))
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

fn http_request(
    method: &str,
    url: &str,
    body: Option<&Value>,
) -> Result<(String, Vec<u8>), McpError> {
    let url = url
        .strip_prefix("http://")
        .ok_or_else(|| McpError::msg("only http:// is supported"))?;
    let (host, path) = url.split_once('/').unwrap_or((url, ""));
    let path = format!("/{path}");
    let (host_only, port) = if let Some((h, p)) = host.split_once(':') {
        (h, p.parse().unwrap_or(80))
    } else {
        (host, 80u16)
    };
    let mut stream = TcpStream::connect((host_only, port))?;
    let payload = body
        .map(serde_json::to_vec)
        .transpose()?
        .unwrap_or_default();
    let extra = if body.is_some() {
        format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            payload.len()
        )
    } else {
        String::new()
    };
    let req =
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{extra}\r\n");
    // One write so the server cannot reply before the POST body arrives.
    let mut msg = req.into_bytes();
    msg.extend_from_slice(&payload);
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
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut in_lock = stdin.lock();
    while let Some(msg) = read_lsp_message(&mut in_lock)? {
        let v: Value = serde_json::from_slice(&msg)?;
        if v.get("method").and_then(|m| m.as_str()) == Some("notifications/initialized") {
            continue;
        }
        let id = v.get("id").cloned().unwrap_or(Value::Null);
        let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let result = match method {
            "initialize" => json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "tekto", "version": "0.1.0" }
            }),
            "tools/list" => json!({ "tools": tool_list() }),
            "tools/call" => {
                let params = v.get("params").cloned().unwrap_or(json!({}));
                match call_tool(&params) {
                    Ok(r) => r,
                    Err(e) => json!({
                        "content": [{ "type": "text", "text": e.to_string() }],
                        "isError": true
                    }),
                }
            }
            "ping" => json!({}),
            _ => {
                write_lsp_message(
                    &mut stdout,
                    &json!({"jsonrpc":"2.0","id": id, "error": {"code": -32601, "message": "method not found"}}),
                )?;
                continue;
            }
        };
        write_lsp_message(
            &mut stdout,
            &json!({"jsonrpc":"2.0","id": id, "result": result}),
        )?;
    }
    Ok(())
}

fn tool_list() -> Value {
    json!([
        {"name": "ui.tree", "description": "Dump the AccessKit / scene tree with stable node ids.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "ui.accesskit", "description": "Dump the AccessKit tree (roles, labels, bounds, focus) with the same stable ids.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "ui.screenshot", "description": "Take a PNG screenshot using the CPU renderer.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "ui.click", "description": "Click a node by id.", "inputSchema": {"type": "object", "properties": {"id": {"type": "number"}}, "required": ["id"]}},
        {"name": "ui.type", "description": "Type text into a node by id.", "inputSchema": {"type": "object", "properties": {"id": {"type": "number"}, "text": {"type": "string"}}, "required": ["id", "text"]}},
        {"name": "ui.patch", "description": "Live-edit a node by stable id: text, name, fill, color, or fontSize. Returns the updated node. Does not reload the app.", "inputSchema": {"type": "object", "properties": {"id": {"type": "number"}, "text": {"type": "string"}, "name": {"type": "string"}, "fill": {"type": "string"}, "color": {"type": "string"}, "fontSize": {"type": "number"}, "font_size": {"type": "number"}}, "required": ["id"]}},
        {"name": "app.state", "description": "Read opted-in app state as JSON.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "app.logs", "description": "Read structured logs.", "inputSchema": {"type": "object", "properties": {}}}
    ])
}

fn call_tool(params: &Value) -> Result<Value, McpError> {
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
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
        "ui.type" => {
            let (_, body) = http_post(&format!("{base}/type"), &args)?;
            Ok(json!({"content": [{"type": "text", "text": String::from_utf8_lossy(&body)}]}))
        }
        "app.state" => {
            let (_, body) = http_get(&format!("{base}/state"))?;
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

fn read_lsp_message(r: &mut impl Read) -> Result<Option<Vec<u8>>, McpError> {
    let mut header = Vec::new();
    loop {
        let mut b = [0u8; 1];
        let n = r.read(&mut b)?;
        if n == 0 {
            return if header.is_empty() {
                Ok(None)
            } else {
                Err(McpError::msg("eof in header"))
            };
        }
        header.push(b[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
        if header.len() > 4096 {
            return Err(McpError::msg("header too large"));
        }
    }
    let header = String::from_utf8_lossy(&header);
    let mut len = 0usize;
    for line in header.lines() {
        let line = line.trim();
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(Some(body))
}

fn write_lsp_message(w: &mut impl Write, v: &Value) -> Result<(), McpError> {
    let body = serde_json::to_vec(v)?;
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tekto_tree::{NodeKind, NodePatch};

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
        };
        let err = handle
            .enqueue_js("tekto.create('window')".into())
            .unwrap_err();
        assert!(err.to_string().contains("no JS runtime"), "{err}");
    }
}
