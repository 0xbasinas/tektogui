//! A small Tekto app. The TypeScript sketch of the same UI is in `app.tsx`.

use std::path::PathBuf;
use tekto::{
    banner, render_headless_to_file, run, text, title_on_banner, window, Color, Tree, WindowOptions,
    BODY_SIZE,
};

fn app_tree() -> Result<Tree, Box<dyn std::error::Error>> {
    let mut tree = Tree::new();
    let width = 640.0;
    let height = 400.0;
    let root = window(&mut tree, "{{title}}", width, height)?;
    // Leave banner width unset so it stretches with the window.
    let b = banner(&mut tree, root, f32::NAN, 64.0)?;
    title_on_banner(&mut tree, b, "{{title}}", f32::NAN)?;
    text(
        &mut tree,
        root,
        "Hello from Tekto",
        BODY_SIZE,
        Color::INK,
    )?;
    Ok(tree)
}

fn main() {
    if let Err(err) = real_main() {
        eprintln!("{{package}}: {err}");
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
        .unwrap_or_else(|| PathBuf::from("{{package}}-headless.png"));
    let tree = app_tree()?;
    if headless {
        render_headless_to_file(&tree, 640, 400, Some(&out))?;
        println!("wrote {}", out.display());
        println!("{}", tree.to_pretty_text());
    } else {
        run(
            tree,
            WindowOptions {
                title: "{{title}}".into(),
                width: 640,
                height: 400,
                dev_server: true,
                decorations: true,
            },
        )?;
    }
    Ok(())
}
