//! `tekto` command line.

use clap::{Parser, Subcommand};

mod android;
mod js_dev;
mod scaffold;
use std::path::PathBuf;
use tekto::{notes_tree, render_headless_to_file, run, WindowOptions};
use tekto_mcp::{http_get, http_post, read_endpoint, run_stdio_mcp};

#[derive(Parser)]
#[command(name = "tekto", about = "Tekto GUI framework tools", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new Notes-style Tekto app (Rust + a TSX sketch).
    New { name: String },
    /// Run the Notes example. Default is a window; use --headless for a PNG.
    Dev {
        #[arg(long)]
        headless: bool,
        #[arg(long, default_value = "notes-headless.png")]
        out: PathBuf,
        /// Build, install, and launch Notes on a connected Android device/AVD,
        /// then `adb forward` the dev server.
        #[arg(long)]
        android: bool,
        /// TypeScript/JSX entry (`.tsx` / `.ts` / `.js`). Reloads the JS
        /// bundle into the running app without rebuilding Rust.
        /// Desktop needs a CLI build with `--features quickjs`. Combined
        /// with `--android`, the APK embeds QuickJS and the host only
        /// needs esbuild (no host `quickjs` feature).
        #[arg(long, value_name = "ENTRY")]
        js: Option<PathBuf>,
        /// Load the JS entry once; do not watch for changes.
        #[arg(long)]
        no_watch: bool,
    },
    /// Build the workspace (debug profile).
    Build {
        /// `android` packages the Notes APK with cargo-apk2.
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        release: bool,
    },
    /// Run tests, including the headless snapshot.
    Test,
    /// Dump the live scene tree from the dev server.
    Tree,
    /// Print the URL of the live inspector.
    Inspect,
    /// Click a node by id (talks to the dev server).
    Click { id: u64 },
    /// Scroll a container by logical pixels.
    Scroll {
        id: u64,
        #[arg(long, allow_hyphen_values = true, value_parser = parse_delta)]
        by: f32,
    },
    /// Press Tab, Shift+Tab, Enter, Space, or Backspace.
    Key {
        #[arg(value_parser = parse_key_name)]
        name: String,
    },
    /// Take a PNG screenshot from the dev server, or render headless if none is running.
    Screenshot {
        #[arg(default_value = "screenshot.png")]
        out: PathBuf,
    },
    /// Live-edit a node in the running app.
    Patch {
        id: u64,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        fill: Option<String>,
        #[arg(long)]
        color: Option<String>,
        #[arg(long, alias = "fontSize")]
        font_size: Option<f32>,
        #[arg(long)]
        padding: Option<f32>,
        #[arg(long)]
        gap: Option<f32>,
    },
    /// List time-travel frames from the dev server (`enabled` is false if the app did not opt in).
    States,
    /// Jump an opted-in store to a recorded index.
    Restore { index: u64 },
    /// Speak the Model Context Protocol on stdio (proxies to the dev server).
    Mcp,
}

fn parse_delta(value: &str) -> Result<f32, String> {
    let delta: f32 = value.parse().map_err(|_| "invalid delta".to_string())?;
    if delta.is_finite() {
        Ok(delta)
    } else {
        Err("delta must be finite".into())
    }
}

fn parse_key_name(name: &str) -> Result<String, String> {
    tekto_mcp::validate_key_name(name)?;
    Ok(name.to_owned())
}

fn export_notes_writeback_env() {
    if std::env::var_os("TEKTO_APP_ROOT").is_some() {
        return;
    }
    if let Ok(cwd) = std::env::current_dir() {
        let notes = cwd.join("examples/notes");
        if notes.join("src").is_dir() {
            std::env::set_var("TEKTO_APP_ROOT", notes);
            std::env::set_var("TEKTO_WRITEBACK_PREFER", "rs");
        }
    }
}

fn main() {
    if let Err(err) = run_cli() {
        eprintln!("tekto: {err}");
        std::process::exit(1);
    }
}

fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::New { name } => {
            let dest = scaffold::scaffold_new(&name)?;
            println!("created {}", dest.display());
            println!("  cargo run -- --headless --out app.png   # PNG + tree");
            println!("  cargo run                               # window + dev server");
        }
        Command::Dev {
            headless,
            out,
            android,
            js,
            no_watch,
        } => {
            if android {
                if let Some(entry) = js {
                    android::dev_android_js(&entry, !no_watch)?;
                    return Ok(());
                }
                android::dev_android(false)?;
                return Ok(());
            }
            if let Some(entry) = js {
                return js_dev::run_js_dev(entry, headless, out, !no_watch);
            }
            export_notes_writeback_env();
            let tree = notes_tree(640.0, 400.0)?;
            if headless {
                let (handle, ep, _viewport) =
                    tekto::start_handle(&tekto::AppRuntime::new(tree.clone(), "Notes"), 640, 400)?;
                let _ = handle;
                println!("dev server {}", ep.base_url());
                render_headless_to_file(&tree, 640, 400, Some(&out))?;
                println!("wrote {}", out.display());
                println!("server stays up until you press Enter");
                let mut line = String::new();
                let _ = std::io::stdin().read_line(&mut line);
                tekto_mcp::clear_endpoint();
            } else {
                run(
                    tree,
                    WindowOptions {
                        title: "Notes".into(),
                        width: 640,
                        height: 400,
                        dev_server: true,
                    },
                )?;
            }
        }
        Command::Build { target, release } => {
            if target.as_deref() == Some("android") {
                let apk = android::build_apk(release)?;
                println!("wrote {}", apk.display());
                return Ok(());
            }
            if let Some(other) = target {
                return Err(format!("unknown build target `{other}` (try `android`)").into());
            }
            let mut cmd = std::process::Command::new("cargo");
            cmd.arg("build");
            if release {
                cmd.arg("--release");
            }
            let status = cmd.status()?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        Command::Test => {
            let status = std::process::Command::new("cargo").arg("test").status()?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        Command::Tree => {
            let ep = read_endpoint()?;
            let (_, body) = http_get(&format!("{}/tree.txt", ep.base_url()))?;
            print!("{}", String::from_utf8_lossy(&body));
        }
        Command::Inspect => {
            let ep = read_endpoint()?;
            println!("{}/inspector", ep.base_url());
        }
        Command::States => {
            let ep = read_endpoint()?;
            let (_, body) = http_get(&format!("{}/states", ep.base_url()))?;
            println!("{}", String::from_utf8_lossy(&body));
        }
        Command::Restore { index } => {
            let ep = read_endpoint()?;
            let (_, body) = http_post(
                &format!("{}/states/restore", ep.base_url()),
                &serde_json::json!({"index": index}),
            )?;
            println!("{}", String::from_utf8_lossy(&body));
        }
        Command::Click { id } => {
            let ep = read_endpoint()?;
            let (_, body) = http_post(
                &format!("{}/click", ep.base_url()),
                &serde_json::json!({"id": id}),
            )?;
            println!("{}", String::from_utf8_lossy(&body));
        }
        Command::Scroll { id, by } => {
            let ep = read_endpoint()?;
            let (_, body) = http_post(
                &format!("{}/scroll", ep.base_url()),
                &serde_json::json!({"id": id, "delta": by}),
            )?;
            println!("{}", String::from_utf8_lossy(&body));
        }
        Command::Key { name } => {
            let ep = read_endpoint()?;
            let (_, body) = http_post(
                &format!("{}/key", ep.base_url()),
                &serde_json::json!({"name": name}),
            )?;
            println!("{}", String::from_utf8_lossy(&body));
        }
        Command::Screenshot { out } => {
            if let Ok(ep) = read_endpoint() {
                let (_, body) = http_get(&format!("{}/screenshot", ep.base_url()))?;
                std::fs::write(&out, body)?;
                println!("wrote {}", out.display());
            } else {
                let tree = notes_tree(640.0, 400.0)?;
                render_headless_to_file(&tree, 640, 400, Some(&out))?;
                println!("wrote {} (no dev server, used headless)", out.display());
            }
        }
        Command::Patch {
            id,
            text,
            fill,
            color,
            font_size,
            padding,
            gap,
        } => {
            let ep = read_endpoint()?;
            let mut v = serde_json::json!({"id": id});
            if let Some(t) = text {
                v["text"] = serde_json::Value::String(t);
            }
            if let Some(f) = fill {
                v["fill"] = serde_json::Value::String(f);
            }
            if let Some(c) = color {
                v["color"] = serde_json::Value::String(c);
            }
            if let Some(size) = font_size {
                v["font_size"] = serde_json::json!(size);
            }
            if let Some(padding) = padding {
                v["padding"] = serde_json::json!(padding);
            }
            if let Some(gap) = gap {
                v["gap"] = serde_json::json!(gap);
            }
            let (_, body) = http_post(&format!("{}/patch", ep.base_url()), &v)?;
            println!("{}", String::from_utf8_lossy(&body));
        }
        Command::Mcp => run_stdio_mcp()?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scroll_command() {
        assert!(
            matches!(Cli::try_parse_from(["tekto", "scroll", "2", "--by", "-12.5"]).unwrap().command, Command::Scroll { id: 2, by } if by == -12.5)
        );
        for args in [
            vec!["tekto", "scroll", "2"],
            vec!["tekto", "scroll", "2", "--by", "NaN"],
            vec!["tekto", "scroll", "2", "--by", "inf"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn parses_key_command() {
        for name in ["Tab", "Shift+Tab", "Enter", "Space", "Backspace"] {
            let cli = Cli::try_parse_from(["tekto", "key", name]).unwrap();
            assert!(matches!(cli.command, Command::Key { name: parsed } if parsed == name));
        }
        assert!(Cli::try_parse_from(["tekto", "key"]).is_err());
        assert!(Cli::try_parse_from(["tekto", "key", "Escape"]).is_err());
    }
}
