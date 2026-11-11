//! Notes example (Rust API). A TypeScript version lives in `src/app.tsx`.

use std::path::PathBuf;
use tekto::{
    menu, menu_item, notes_tree, render_headless_to_file, run, tab, tab_list, tab_panel, text,
    text_area, text_field, Color, Tree, WindowOptions, BODY_SIZE,
};

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
    let tree = demo_tree()?;
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

/// The Notes screen plus the newest widgets, so they can be tried by hand:
/// tab panels, a multiline field (select across lines, IME, accessibility
/// text runs), and a right-click context menu on the title field.
fn demo_tree() -> Result<Tree, Box<dyn std::error::Error>> {
    let mut tree = notes_tree(640.0, 400.0)?;
    let root = tree.root().ok_or("notes tree has no root")?;
    let tabs = tab_list(&mut tree, root, "Sections")?;
    let edit = tab(&mut tree, tabs, "Edit", true)?;
    let about = tab(&mut tree, tabs, "About", false)?;
    let edit_panel = tab_panel(&mut tree, root, edit, "Edit panel")?;
    let title = text_field(&mut tree, edit_panel, "Title", "Groceries")?;
    text_area(
        &mut tree,
        edit_panel,
        "Body",
        "Milk
Eggs
Bread",
        f32::NAN,
        120.0,
    )?;
    let about_panel = tab_panel(&mut tree, root, about, "About panel")?;
    text(
        &mut tree,
        about_panel,
        "Switch tabs: the Edit text keeps its state.",
        BODY_SIZE,
        Color::INK,
    )?;
    let ctx = menu(&mut tree, "Title actions")?;
    menu_item(&mut tree, ctx, "Copy")?;
    menu_item(&mut tree, ctx, "Paste")?;
    tree.bind_context_menu(title, ctx)?;
    Ok(tree)
}
