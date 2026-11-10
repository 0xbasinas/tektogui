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

/// rquickjs bindgen needs host libclang. The NDK ships one under
/// `toolchains/llvm/prebuilt/<host>/musl/lib`; use it when LIBCLANG_PATH is unset.
fn maybe_set_ndk_libclang() {
    if std::env::var_os("LIBCLANG_PATH").is_some() {
        return;
    }
    let ndk = std::env::var_os("ANDROID_NDK_HOME")
        .or_else(|| std::env::var_os("ANDROID_NDK_ROOT"))
        .or_else(|| {
            std::env::var_os("ANDROID_HOME")
                .map(|h| PathBuf::from(h).join("ndk/27.1.12297006").into_os_string())
        });
    let Some(ndk) = ndk else { return };
    let prebuilt = PathBuf::from(ndk).join("toolchains/llvm/prebuilt");
    let Ok(hosts) = std::fs::read_dir(&prebuilt) else {
        return;
    };
    for host in hosts.flatten() {
        let lib = host.path().join("musl/lib");
        if lib.join("libclang.so").exists() || lib.join("libclang.dll").exists() {
            println!("tekto: LIBCLANG_PATH {}", lib.display());
            std::env::set_var("LIBCLANG_PATH", lib);
            return;
        }
    }
}

/// rquickjs bindgen (Android only) needs the NDK sysroot and clang builtin
/// headers; the NDK libclang reports an empty InstalledDir otherwise.
fn maybe_set_ndk_bindgen_args() {
    let ndk = std::env::var_os("ANDROID_NDK_HOME")
        .or_else(|| std::env::var_os("ANDROID_NDK_ROOT"))
        .or_else(|| {
            std::env::var_os("ANDROID_HOME")
                .map(|h| PathBuf::from(h).join("ndk/27.1.12297006").into_os_string())
        });
    let Some(ndk) = ndk else { return };
    let prebuilt = PathBuf::from(ndk).join("toolchains/llvm/prebuilt");
    let Ok(hosts) = std::fs::read_dir(&prebuilt) else {
        return;
    };
    for host in hosts.flatten() {
        let root = host.path();
        let sysroot = root.join("sysroot");
        let clang_inc = {
            let mut found = None;
            if let Ok(vers) = std::fs::read_dir(root.join("lib/clang")) {
                for ver in vers.flatten() {
                    let inc = ver.path().join("include");
                    if inc.join("stdbool.h").exists() {
                        found = Some(inc);
                        break;
                    }
                }
            }
            found
        };
        if !sysroot.exists() || clang_inc.is_none() {
            continue;
        }
        let clang_inc = clang_inc.unwrap();
        let pairs = [
            ("x86_64-linux-android", "x86_64-linux-android24"),
            ("aarch64-linux-android", "aarch64-linux-android24"),
        ];
        for (triple, clang_target) in pairs {
            let args = format!(
                "--sysroot={} --target={clang_target} -I{}",
                sysroot.display(),
                clang_inc.display()
            );
            for key in [
                format!("BINDGEN_EXTRA_CLANG_ARGS_{triple}"),
                format!("BINDGEN_EXTRA_CLANG_ARGS_{}", triple.replace('-', "_")),
            ] {
                if std::env::var_os(&key).is_none() {
                    std::env::set_var(&key, &args);
                }
            }
        }
        println!(
            "tekto: bindgen NDK sysroot {} include {}",
            sysroot.display(),
            clang_inc.display()
        );
        return;
    }
}

/// Build the Notes APK with cargo-apk2. Returns the path to the APK.
pub fn build_apk(release: bool) -> Result<PathBuf, String> {
    build_apk_with(release, &[], None)
}

/// Like [`build_apk`], with optional cargo features and a single Android target
/// triple (e.g. `x86_64-linux-android`). `None` builds every triple listed in
/// the package metadata (aarch64 + x86_64).
pub fn build_apk_with(
    release: bool,
    features: &[&str],
    target: Option<&str>,
) -> Result<PathBuf, String> {
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
    let mut args: Vec<String> = vec![
        "apk2".into(),
        "build".into(),
        "-p".into(),
        "notes-android".into(),
    ];
    if release {
        args.push("--release".into());
    }
    if !features.is_empty() {
        args.push("-F".into());
        args.push(features.join(","));
    }
    if let Some(t) = target {
        args.push("--target".into());
        args.push(t.to_string());
    }
    maybe_set_ndk_libclang();
    maybe_set_ndk_bindgen_args();
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

/// ABI of the connected device, mapped to a Rust Android target triple.
/// Override with `TEKTO_ANDROID_TARGET` (e.g. `x86_64-linux-android`).
pub fn detect_android_target() -> Option<String> {
    if let Ok(t) = std::env::var("TEKTO_ANDROID_TARGET") {
        let t = t.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    let out = adb()
        .args(["shell", "getprop", "ro.product.cpu.abi"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    match String::from_utf8_lossy(&out.stdout).trim() {
        "x86_64" => Some("x86_64-linux-android".into()),
        "arm64-v8a" | "aarch64" => Some("aarch64-linux-android".into()),
        _ => None,
    }
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

/// Build the Notes APK with QuickJS, install it, `adb forward` the dev
/// server, then bundle the TSX entry and `POST /reload`. The host CLI does
/// not need `--features quickjs` (only esbuild + HTTP).
pub fn dev_android_js(entry: &Path, watch: bool) -> Result<(), String> {
    crate::js_dev::export_writeback_env(entry);
    let target = detect_android_target();
    if let Some(t) = target.as_deref() {
        println!("tekto: android target {t}");
    } else {
        println!("tekto: building both Android ABIs (set TEKTO_ANDROID_TARGET to build one)");
    }
    let apk = build_apk_with(false, &["quickjs"], target.as_deref())?;
    println!("tekto: apk {}", apk.display());
    install_and_launch(&apk)?;
    let port = forward_dev_port()?;
    println!("tekto: adb forward tcp:{port} tcp:{port}");
    wait_for_dev_server(Duration::from_secs(90))?;
    println!("dev server http://127.0.0.1:{port}");
    post_js_bundle(entry)?;
    if watch {
        println!("saving a .tsx / .ts / .js file reloads the JS side over adb forward; the APK stays up.");
        let entry = entry.to_path_buf();
        let _watcher = tekto_js::spawn_watcher(entry.clone(), move || {
            if let Err(err) = post_js_bundle(&entry) {
                eprintln!("js reload failed: {err}");
            }
        });
        println!("server stays up until you press Enter");
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
    } else {
        println!("Try: tekto tree    or    tekto screenshot    or    tekto click <id>");
    }
    Ok(())
}

fn post_js_bundle(entry: &Path) -> Result<(), String> {
    let t0 = std::time::Instant::now();
    let js = tekto_js::bundle(entry).map_err(|e| e.to_string())?;
    let bundle_ms = t0.elapsed().as_millis();
    println!("js bundle {bundle_ms}ms ({} bytes)", js.len());
    let t1 = std::time::Instant::now();
    let url = format!("http://127.0.0.1:{ANDROID_DEV_PORT}/reload");
    let (_headers, body) = tekto_mcp::http_post_raw(&url, js.as_bytes(), "application/javascript")
        .map_err(|e| e.to_string())?;
    let post_ms = t1.elapsed().as_millis();
    let reply = String::from_utf8_lossy(&body);
    println!("POST /reload {post_ms}ms: {}", reply.trim());
    Ok(())
}
