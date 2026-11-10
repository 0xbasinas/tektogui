//! Build, install, and talk to the Notes APK (`tekto build --target android`
//! and `tekto dev --android`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tekto::ANDROID_DEV_PORT;

pub const ANDROID_PACKAGE: &str = "com.tektogui.notes";
pub const ANDROID_ACTIVITY: &str = "android.app.NativeActivity";

fn repo_root() -> Result<PathBuf, String> {
    crate::scaffold::find_tekto_root().map_err(|e| e.to_string())
}

fn adb() -> Command {
    let mut c = Command::new("adb");
    if let Ok(serial) = std::env::var("ANDROID_SERIAL") {
        c.arg("-s").arg(serial);
    } else if let Ok(serial) = std::env::var("TEKTO_ANDROID_SERIAL") {
        c.arg("-s").arg(serial);
    }
    c
}

fn require_cmd(name: &str) -> Result<(), String> {
    if Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Ok(());
    }
    // cargo-apk2 --version works; `which` fallback
    if Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Ok(());
    }
    Err(format!(
        "`{name}` is not on PATH. For the APK: `cargo install cargo-apk2`. Also need the Android NDK/SDK (source ~/.android-env.sh if you have it)."
    ))
}

/// Build the Notes APK with cargo-apk2. Returns the path to the APK.
pub fn build_apk(release: bool) -> Result<PathBuf, String> {
    require_cmd("cargo")?;
    // cargo apk2 is a cargo subcommand provided by cargo-apk2
    if Command::new("cargo")
        .args(["apk2", "--help"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        .eq(&false)
    {
        return Err(
            "`cargo apk2` is not installed. Run: cargo install cargo-apk2 (MIT OR Apache-2.0)."
                .into(),
        );
    }
    let root = repo_root()?;
    let mut args = vec!["apk2", "build", "-p", "notes-android"];
    if release {
        args.push("--release");
    }
    println!("tekto: cargo {}", args.join(" "));
    let status = Command::new("cargo")
        .args(&args)
        .current_dir(&root)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("cargo apk2 build failed".into());
    }
    find_apk(&root, release)
        .ok_or_else(|| "built the APK, but could not find notes.apk under target/".into())
}

fn find_apk(root: &Path, release: bool) -> Option<PathBuf> {
    let profile = if release { "release" } else { "debug" };
    let mut candidates: Vec<PathBuf> = Vec::new();
    let walker = walk_limited(root.join("target"), 8);
    for path in walker {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == "notes.apk" || name == "notes-android.apk" {
            // Prefer the requested profile if the path contains it.
            candidates.push(path);
        }
    }
    candidates.sort_by_key(|p| {
        let s = p.to_string_lossy();
        let score = if s.contains(profile) { 0 } else { 1 };
        (score, std::cmp::Reverse(s.len()))
    });
    candidates.into_iter().next()
}

fn walk_limited(dir: PathBuf, depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn rec(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth == 0 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for ent in rd.flatten() {
            let path = ent.path();
            if path.is_dir() {
                rec(&path, depth - 1, out);
            } else {
                out.push(path);
            }
        }
    }
    rec(&dir, depth, &mut out);
    out
}

fn adb_ok() -> Result<(), String> {
    require_cmd("adb")?;
    let out = adb().arg("devices").output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let ready = text.lines().any(|l| {
        let l = l.trim();
        !l.is_empty() && !l.starts_with("List") && l.split_whitespace().nth(1) == Some("device")
    });
    if !ready {
        return Err(
            "no Android device/emulator is online. Start AVD beddcode30 first, e.g.\n  source ~/.android-env.sh\n  emulator -avd beddcode30 -no-window -memory 2048 -accel off"
                .into(),
        );
    }
    Ok(())
}

pub fn install_and_launch(apk: &Path) -> Result<(), String> {
    adb_ok()?;
    println!("tekto: adb install -r {}", apk.display());
    let status = adb()
        .args(["install", "-r", "--no-incremental"])
        .arg(apk)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        // retry without --no-incremental
        let status = adb()
            .args(["install", "-r"])
            .arg(apk)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("adb install failed".into());
        }
    }
    let _ = adb()
        .args(["shell", "am", "force-stop", ANDROID_PACKAGE])
        .status();
    let component = format!("{ANDROID_PACKAGE}/{ANDROID_ACTIVITY}");
    println!("tekto: adb shell am start -n {component}");
    let out = adb()
        .args(["shell", "am", "start", "-n", &component])
        .output()
        .map_err(|e| e.to_string())?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() || combined.contains("Error type") {
        return Err(format!("am start failed: {}", combined.trim()));
    }
    Ok(())
}

pub fn forward_dev_port() -> Result<u16, String> {
    adb_ok()?;
    let port = ANDROID_DEV_PORT.to_string();
    let status = adb()
        .args(["forward", &format!("tcp:{port}"), &format!("tcp:{port}")])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("adb forward failed".into());
    }
    let ep = tekto_mcp::DevEndpoint {
        port: ANDROID_DEV_PORT,
        host: "127.0.0.1".into(),
    };
    tekto_mcp::write_endpoint(&ep).map_err(|e| e.to_string())?;
    Ok(ANDROID_DEV_PORT)
}

/// Wait until the forwarded dev server answers /health (or /tree).
pub fn wait_for_dev_server(timeout: Duration) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{ANDROID_DEV_PORT}/tree.txt");
    let start = Instant::now();
    let mut last = String::new();
    while start.elapsed() < timeout {
        match tekto_mcp::http_get(&url) {
            Ok((_, body)) => {
                let text = String::from_utf8_lossy(&body);
                if text.contains("window#") || !text.is_empty() {
                    return Ok(());
                }
                last = text.into_owned();
            }
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    Err(format!(
        "dev server at 127.0.0.1:{ANDROID_DEV_PORT} did not answer within {}s (last: {last})",
        timeout.as_secs()
    ))
}

pub fn dev_android(release: bool) -> Result<(), String> {
    let apk = build_apk(release)?;
    println!("tekto: apk {}", apk.display());
    install_and_launch(&apk)?;
    let port = forward_dev_port()?;
    println!("tekto: adb forward tcp:{port} tcp:{port}");
    wait_for_dev_server(Duration::from_secs(90))?;
    println!("dev server http://127.0.0.1:{port}");
    println!("Try: tekto tree    or    tekto screenshot    or    tekto click <id>");
    Ok(())
}
