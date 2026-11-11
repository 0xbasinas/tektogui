//! End-to-end checks that `tekto tree`, `tekto screenshot`, and `tekto patch`
//! talk to a running dev server (issues #7 and #12).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn tekto_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tekto"))
}

fn unique_dir() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tekto-cli-e2e-{}-{now}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_tekto(dev_file: &Path, args: &[&str]) -> std::process::Output {
    Command::new(tekto_bin())
        .args(args)
        .env("TEKTO_DEV_FILE", dev_file)
        .output()
        .expect("run tekto")
}

fn wait_for_endpoint(path: &Path, timeout: Duration) {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(s) = fs::read_to_string(path) {
            if s.contains("port") {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for dev server file {}", path.display());
}

fn id_for(tree: &str, needle: &str) -> u64 {
    for line in tree.lines() {
        if !line.contains(needle) {
            continue;
        }
        if let Some(hash) = line.find('#') {
            let digits: String = line[hash + 1..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(id) = digits.parse() {
                return id;
            }
        }
    }
    panic!("no node id for {needle} in tree:\n{tree}");
}

struct DevServer {
    child: std::process::Child,
}

impl Drop for DevServer {
    fn drop(&mut self) {
        if let Some(mut stdin) = self.child.stdin.take() {
            let _ = stdin.write_all(b"\n");
            let _ = stdin.flush();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn ipv6_dev_server_cli_flow() {
    if std::net::TcpListener::bind("[::1]:0").is_err() {
        eprintln!("IPv6 loopback unavailable; skipping");
        return;
    }
    let dir = unique_dir();
    let dev_file = dir.join("endpoint.json");
    let child = Command::new(tekto_bin())
        .args(["dev", "--headless", "--out"])
        .arg(dir.join("headless.png"))
        .env("TEKTO_DEV_FILE", &dev_file)
        .env("TEKTO_DEV_HOST", "::1")
        .env("TEKTO_DEV_PORT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = DevServer { child };
    wait_for_endpoint(&dev_file, Duration::from_secs(30));
    let inspect = run_tekto(&dev_file, &["inspect"]);
    assert!(inspect.status.success());
    assert!(String::from_utf8_lossy(&inspect.stdout).starts_with("http://[::1]:"));
    let tree = run_tekto(&dev_file, &["tree"]);
    assert!(
        tree.status.success(),
        "{}",
        String::from_utf8_lossy(&tree.stderr)
    );
    assert!(String::from_utf8_lossy(&tree.stdout).contains("Hello from Tekto"));
}

#[test]
fn tree_screenshot_and_patch_against_dev_server() {
    let dir = unique_dir();
    let dev_file = dir.join("tekto-dev.json");
    let headless = dir.join("headless.png");
    let live = dir.join("live.png");

    let child = Command::new(tekto_bin())
        .args(["dev", "--headless", "--out"])
        .arg(&headless)
        .env("TEKTO_DEV_FILE", &dev_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start tekto dev");
    let _server = DevServer { child };

    wait_for_endpoint(&dev_file, Duration::from_secs(30));

    let out = run_tekto(&dev_file, &["inspect"]);
    assert!(
        out.status.success(),
        "inspect failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let inspector_url = String::from_utf8(out.stdout).unwrap();
    let inspector_url = inspector_url.trim();
    assert!(inspector_url.ends_with("/inspector"));
    let (headers, page) = tekto_mcp::http_get(inspector_url).unwrap();
    assert!(headers.contains("text/html; charset=utf-8"));
    let page = String::from_utf8(page).unwrap();
    assert!(page.contains("Tekto inspector") && page.contains("/tree") && page.contains("/patch"));
    let base = inspector_url.strip_suffix("/inspector").unwrap();
    let (_, health) = tekto_mcp::http_get(&format!("{base}/health")).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&health).unwrap()["ok"],
        true
    );
    let (_, tree_json) = tekto_mcp::http_get(&format!("{base}/tree")).unwrap();
    let tree_json: serde_json::Value = serde_json::from_slice(&tree_json).unwrap();
    let node = tree_json["dump"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["text"] == "Hello from Tekto")
        .unwrap();
    let (_, patched) = tekto_mcp::http_post(
        &format!("{base}/patch"),
        &serde_json::json!({"id": node["id"], "name": "Inspector test"}),
    )
    .unwrap();
    let patched: serde_json::Value = serde_json::from_slice(&patched).unwrap();
    assert_eq!(patched["node"]["name"], "Inspector test");
    let (_, updated) = tekto_mcp::http_get(&format!("{base}/tree")).unwrap();
    assert!(String::from_utf8(updated)
        .unwrap()
        .contains("Inspector test"));

    let out = run_tekto(&dev_file, &["tree"]);
    assert!(
        out.status.success(),
        "tree failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let tree = String::from_utf8_lossy(&out.stdout);
    assert!(tree.contains("window#1"), "missing window#1 in:\n{tree}");
    assert!(
        tree.contains("Hello from Tekto"),
        "missing greeting in:\n{tree}"
    );

    let out = run_tekto(&dev_file, &["screenshot", live.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "screenshot failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let png = fs::read(&live).expect("live png");
    assert!(
        png.starts_with(&[0x89, b'P', b'N', b'G']),
        "screenshot was not a PNG ({} bytes)",
        png.len()
    );

    let body_id = id_for(&tree, "Hello from Tekto");
    let banner_id = id_for(&tree, "banner");

    let id = body_id.to_string();
    let out = run_tekto(&dev_file, &["patch", &id, "--text", "Patched hello"]);
    assert!(
        out.status.success(),
        "patch text failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let patch_body = String::from_utf8_lossy(&out.stdout);
    assert!(
        patch_body.contains("Patched hello") && patch_body.contains("\"ok\": true"),
        "patch should return the updated node, got:
    {patch_body}"
    );

    let out = run_tekto(&dev_file, &["tree"]);
    let tree2 = String::from_utf8_lossy(&out.stdout);
    assert!(
        tree2.contains("Patched hello"),
        "tree did not show patched text:\n{tree2}"
    );
    assert!(
        !tree2.contains("Hello from Tekto"),
        "old text still present:\n{tree2}"
    );

    let bid = banner_id.to_string();
    let out = run_tekto(&dev_file, &["patch", &bid, "--fill", "#c44c3a"]);
    assert!(
        out.status.success(),
        "patch fill failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run_tekto(&dev_file, &["patch", &id, "--color", "#ffffff"]);
    assert!(
        out.status.success(),
        "patch color failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = run_tekto(&dev_file, &["patch", "99999", "--text", "nope"]);
    assert!(
        !out.status.success(),
        "missing id should be an error, stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        err.contains("unknown node") && err.contains("99999"),
        "unclear missing-id error: {err}"
    );
}

#[test]
fn tree_fails_clearly_without_dev_server() {
    let missing = unique_dir().join("no-server.json");
    let out = run_tekto(&missing, &["tree"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("no dev server") && err.contains("tekto dev"),
        "unclear no-server error: {err}"
    );
}
