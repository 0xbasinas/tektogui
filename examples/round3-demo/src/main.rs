//! Round 3 feature demo, to try by hand.
//!
//! Tokens are set before the widgets exist, so the whole screen uses the
//! tinted palette. The bar under the header runs a repeating colour tween.
//! The split pane collapses its right side when the window is narrower than
//! 760 logical pixels and moves focus to the split. The slider keeps tracking
//! after the pointer leaves its bounds because capture lives in the tree.
//!
//!   cargo run -p round3-demo

use tekto::{
    banner, progress, slider, split_pane, text, text_field, title_on_banner, window, Color, Easing,
    NodeKind, StyleTransition, Tree, WindowOptions, BANNER_HEIGHT,
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
    let header = banner(&mut tree, root, f32::NAN, BANNER_HEIGHT)?;
    title_on_banner(&mut tree, header, "Round 3: things to try", f32::NAN)?;
    text(
        &mut tree,
        root,
        "Resize below 760 px: the right pane collapses. Drag the slider outside its bounds: capture keeps tracking. The bar below pulses with a repeating colour tween.",
        15.0,
        Color::INK,
    )?;

    let pulse = tree.add(NodeKind::Rect, Some(root))?;
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

    let split = split_pane(&mut tree, root, 760.0, 16.0)?;
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
