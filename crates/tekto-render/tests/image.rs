use tekto_image::{Fit, Image};
use tekto_render::{CpuRenderer, Renderer};
use tekto_tree::{Color, Rect};

#[test]
fn rgba_images_fit_clip_and_blend() {
    let image = Image::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
    let mut renderer = CpuRenderer::new(8, 8).unwrap();
    renderer.begin_frame(Color::rgba(255, 255, 255, 255));
    renderer.draw_image(&image, Rect::new(2.0, 2.0, 4.0, 4.0), Fit::Contain, 1.0);
    let pixels = renderer.rgba8().unwrap();
    let pixel = |x: usize, y: usize| &pixels[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4];
    assert_eq!(pixel(3, 2), &[255, 255, 255, 255]);
    assert_eq!(pixel(2, 3), &[255, 0, 0, 255]);
    assert_eq!(pixel(5, 3), &[127, 127, 255, 255]);
    renderer.begin_frame(Color::rgba(255, 255, 255, 255));
    renderer.push_clip(Rect::new(2.0, 2.0, 2.0, 4.0));
    renderer.draw_image(&image, Rect::new(2.0, 2.0, 4.0, 4.0), Fit::Cover, 1.0);
    renderer.pop_clip();
    let pixels = renderer.rgba8().unwrap();
    assert_eq!(
        &pixels[(3 * 8 + 5) * 4..(3 * 8 + 6) * 4],
        &[255, 255, 255, 255]
    );
    assert_ne!(
        &pixels[(3 * 8 + 2) * 4..(3 * 8 + 3) * 4],
        &[255, 255, 255, 255]
    );
}

#[cfg(all(feature = "gpu", not(target_arch = "wasm32")))]
#[test]
#[ignore = "requires an available GPU adapter"]
fn gpu_renders_image_pixels() {
    let image = Image::new(1, 1, vec![255, 0, 0, 255]).unwrap();
    let mut renderer =
        tekto_render::GpuRenderer::new(8, 8).expect("GPU adapter required for this test");
    renderer.begin_frame(Color::rgba(255, 255, 255, 255));
    renderer.draw_image(&image, Rect::new(2.0, 2.0, 4.0, 4.0), Fit::Contain, 1.0);
    let pixels = renderer.rgba8().unwrap();
    let center = &pixels[(3 * 8 + 3) * 4..(3 * 8 + 4) * 4];
    assert!(
        center[0] > 240 && center[1] < 15 && center[2] < 15,
        "{center:?}"
    );
}
