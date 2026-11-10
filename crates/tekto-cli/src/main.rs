//! `tekto` command line.

use clap::{Parser, Subcommand};
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
    /// Create a new Tekto app (scaffold is still small).
    New { name: String },
    /// Run the Notes example. Default is a window; use --headless for a PNG.
    Dev {
        #[arg(long)]
        headless: bool,
        #[arg(long, default_value = "notes-headless.png")]
        out: PathBuf,
    },
    /// Build the workspace (debug profile).
    Build,
    /// Run tests, including the headless snapshot.
    Test,
    /// Dump the live scene tree from the dev server.
    Tree,
    /// Click a node by id (talks to the dev server).
    Click { id: u64 },
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
    },
    /// Speak the Model Context Protocol on stdio (proxies to the dev server).
    Mcp,
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
            std::fs::create_dir_all(&name)?;
            std::fs::write(
                PathBuf::from(&name).join("README.md"),
                format!(
                    "# {name}\n\nA Tekto app. Run `tekto dev` from the tektogui repo for now.\n"
                ),
            )?;
            println!("created ./{name}");
        }
        Command::Dev { headless, out } => {
            let tree = notes_tree(640.0, 400.0)?;
            if headless {
                let (handle, ep) =
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
        Command::Build => {
            let status = std::process::Command::new("cargo").arg("build").status()?;
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
        Command::Click { id } => {
            let ep = read_endpoint()?;
            let (_, body) = http_post(
                &format!("{}/click", ep.base_url()),
                &serde_json::json!({"id": id}),
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
            let (_, body) = http_post(&format!("{}/patch", ep.base_url()), &v)?;
            println!("{}", String::from_utf8_lossy(&body));
        }
        Command::Mcp => run_stdio_mcp()?,
    }
    Ok(())
}
