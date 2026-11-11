//! Round 3 feature demo, to try by hand.
//!
//! The window is undecorated, so the in-window app bar is the title bar: drag
//! it to move the window and use the three buttons on the right to minimise,
//! maximise and close. Tokens are set before the widgets exist, so the whole
//! screen uses the tinted palette. The bar under the text runs a repeating
//! colour tween. The split pane collapses its right side when the window is
//! narrower than 760 logical pixels and moves focus to the split. The slider
//! keeps tracking after the pointer leaves its bounds because capture lives in
//! the tree.
//!
//!   cargo run -p round3-demo

use tekto::{
    app_bar, app_bar_leading_button, app_bar_window_controls, progress, slider, split_pane, text,
    text_field, window, Color, Easing, NodeKind, StyleTransition, Tree, WindowOptions,
};

fn main() {
    if let Err(error) = real_main() {
        eprintln!("round3-demo: {error}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<(), Box<dyn std::error::Error>> {
    let tree = demo()?;
    tekto::run(
        tree,
        WindowOptions {
            title: "Tekto round 3".into(),
            width: 900,
            height: 640,
            dev_server: true,
            decorations: false,
        },
    )?;
    Ok(())
}

fn demo() -> Result<Tree, Box<dyn std::error::Error>> {
    let mut tree = Tree::new();
    // Tokens apply to every node created afterwards, which is all of them.
    let mut tokens = tree.tokens();
    tokens.page = Color::rgb(0xf2, 0xf5, 0xf7);
    tokens.banner = Color::rgb(0x2f, 0x4a, 0x5f);
    tokens.focus = Color::rgb(0xd9, 0x77, 0x06);
    tokens.gap = 16.0;
    tokens.padding = 20.0;
    tree.set_tokens(tokens);

    let root = window(&mut tree, "Tekto round 3", 900.0, 640.0)?;
    if let Some(node) = tree.get_mut(root) {
        // Flush chrome: the app bar spans the window edge to edge.
        node.layout.padding = 0.0;
    }
    let bar = app_bar(&mut tree, root, "Round 3: things to try")?;
    app_bar_leading_button(&mut tree, &bar, "Menu")?;
    app_bar_window_controls(&mut tree, &bar)?;

    let content = tree.add(NodeKind::Group, Some(root))?;
    if let Some(node) = tree.get_mut(content) {
        node.layout.padding = 20.0;
        node.layout.gap = 16.0;
        node.layout.column = true;
    }

    text(
        &mut tree,
        content,
        "Drag the bar to move the window; the buttons on its right minimise, maximise and close. Resize below 760 px: the right pane collapses. Drag the slider outside its bounds: capture keeps tracking.",
        15.0,
        Color::INK,
    )?;

    let pulse = tree.add(NodeKind::Rect, Some(content))?;
    if let Some(node) = tree.get_mut(pulse) {
        node.name = Some("pulse".into());
        node.fill = Some(Color::rgb(0x2f, 0x4a, 0x5f));
        node.layout.height = 10.0;
        node.radius = 5.0;
    }
    tree.transition(
        pulse,
        StyleTransition {
            fill: Some(Color::rgb(0xd9, 0x77, 0x06)),
            easing: Some(Easing::Smoothstep),
            repeat: true,
            duration_ms: Some(900.0),
            ..StyleTransition::default()
        },
    )?;

    let split = split_pane(&mut tree, content, 760.0, 16.0)?;
    text(
        &mut tree,
        split.primary,
        "Primary pane: always visible",
        16.0,
        Color::INK,
    )?;
    text_field(&mut tree, split.primary, "Name", "Ada")?;
    text(
        &mut tree,
        split.secondary,
        "Secondary pane: folds away on a narrow window",
        15.0,
        Color::INK,
    )?;
    slider(&mut tree, split.secondary, "Volume", 0.4)?;
    progress(&mut tree, split.secondary, "Progress", 0.6)?;

    Ok(tree)
}
