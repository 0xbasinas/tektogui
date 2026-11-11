//! Optional regression with a locally supplied, unredistributed CJK font.
use tekto::{layout_tree, render_headless_to_file, set_fallback_font, text, window, Color, Tree};

fn image(value: &str) -> Vec<u8> {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Font test", 240.0, 100.0).unwrap();
    text(&mut tree, root, value, 32.0, Color::INK).unwrap();
    layout_tree(&mut tree, 240.0, 100.0);
    render_headless_to_file(&tree, 240, 100, None).unwrap()
}

#[test]
fn supplied_cjk_font_paints_distinct_glyphs() {
    let Some(path) = std::env::var_os("TEKTO_TEST_CJK_FONT") else {
        eprintln!("TEKTO_TEST_CJK_FONT not set; skipping local CJK font check");
        return;
    };
    set_fallback_font(std::fs::read(path).expect("read local CJK font")).unwrap();
    let japanese = image("日本");
    let missing = image("\u{10ffff}\u{10ffff}");
    assert_ne!(
        japanese, missing,
        "CJK must paint differently from missing glyphs"
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tektogui-shots/fallback-cjk.png");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, japanese).unwrap();
}
