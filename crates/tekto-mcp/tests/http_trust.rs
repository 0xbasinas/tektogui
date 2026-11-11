use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use tekto_mcp::{start_dev_server, SharedTreeHandle};
use tekto_tree::{NodeKind, Tree};

#[test]
fn inspector_patch_writes_source_and_reload_without_origin() {
    let root = std::env::temp_dir().join(format!("tekto-http-trust-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    let source = root.join("src/app.tsx");
    std::fs::write(&source, "const label = \"before trust test\";\n").unwrap();
    std::env::set_var("TEKTO_APP_ROOT", &root);
    std::env::set_var("TEKTO_WRITEBACK_PREFER", "ts");
    std::env::set_var("TEKTO_DEV_FILE", root.join("endpoint.json"));
    std::env::set_var("TEKTO_DEV_HOST", "127.0.0.1");
    std::env::set_var("TEKTO_DEV_PORT", "0");
    let mut tree = Tree::new();
    let id = tree.add(NodeKind::Window, None).unwrap();
    tree.get_mut(id).unwrap().text = Some("before trust test".into());
    let handle = Arc::new(SharedTreeHandle {
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
    });
    let endpoint = start_dev_server(handle.clone()).unwrap();
    let send = |path: &str, body: &str, origin: &str| {
        let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port)).unwrap();
        write!(stream, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{origin}Content-Length: {}\r\n\r\n{body}", endpoint.port, body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    };
    let body = format!(r#"{{"id":{},"text":"after trust test"}}"#, id.raw());
    let origin = format!("Origin: http://127.0.0.1:{}\r\n", endpoint.port);
    let response = send("/patch", &body, &origin);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("\"written\""), "{response}");
    assert!(std::fs::read_to_string(source)
        .unwrap()
        .contains("after trust test"));
    let response = send("/reload", "console.log('reload')", "");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(
        handle.pending_js.lock().unwrap().as_deref(),
        Some("console.log('reload')")
    );
    std::fs::remove_dir_all(root).unwrap();
}
