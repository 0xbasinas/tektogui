//! Notes example (Rust API). A TypeScript version lives in `src/app.tsx`.

use std::path::PathBuf;
use tekto::{notes_tree, render_headless_to_file, run, WindowOptions};

fn main() {
    if let Err(err) = real_main() {
        eprintln!("notes: {err}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let headless = args.iter().any(|a| a == "--headless");
    let out = args
        .windows(2)
        .find(|w| w[0] == "--out")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| PathBuf::from("notes-headless.png"));
    let tree = notes_tree(640.0, 400.0)?;
    if headless {
        render_headless_to_file(&tree, 640, 400, Some(&out))?;
        println!("wrote {}", out.display());
        println!("{}", tree.to_pretty_text());
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
    Ok(())
}
