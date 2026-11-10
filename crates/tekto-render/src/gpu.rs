//! wgpu + Vello renderer. Compiled only with the `gpu` feature.
//!
//! The scene is recorded on the CPU and rasterized on the GPU into an
//! offscreen texture. The window still presents those pixels through
//! softbuffer (`rgba8`), same as the CPU backend. Headless snapshots
//! stay on [`crate::CpuRenderer`].

use crate::{RenderError, Renderer};
use parley::PositionedLayoutItem;
use std::cell::RefCell;
use std::num::NonZeroUsize;
use std::time::Duration;
use tekto_text::TextEngine;
use tekto_tree::{Color as TektoColor, Rect};
use vello::kurbo::{Affine, Rect as VelloRect};
use vello::peniko::{Fill, FontData};
use vello::{wgpu, Renderer as VelloRenderer};
use vello::{AaConfig, AaSupport, Glyph, RenderParams, RendererOptions, Scene};

struct Inner {
    width: u32,
    height: u32,
    scale: f32,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: VelloRenderer,
    scene: Scene,
    clear: vello::peniko::Color,
    text: TextEngine,
    adapter_name: String,
    /// Logical clips. A layer is pushed for each one.
    clip_depth: usize,
    pixels: Option<Vec<u8>>,
}

pub struct GpuRenderer {
    inner: RefCell<Inner>,
}

impl GpuRenderer {
    pub fn new(width: u32, height: u32) -> Result<Self, RenderError> {
        let width = width.max(1);
        let height = height.max(1);
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|err| RenderError::msg(format!("no GPU adapter: {err}")))?;
        let adapter_name = adapter.get_info().name;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tekto-gpu"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|err| RenderError::msg(format!("GPU device: {err}")))?;
        let renderer = VelloRenderer::new(
            &device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
                pipeline_cache: None,
            },
        )
        .map_err(|err| RenderError::msg(format!("vello: {err}")))?;
        Ok(Self {
            inner: RefCell::new(Inner {
                width,
                height,
                scale: 1.0,
                device,
                queue,
                renderer,
                scene: Scene::new(),
                clear: to_color(TektoColor::PAGE),
                text: TextEngine::new(),
                adapter_name,
                clip_depth: 0,
                pixels: None,
            }),
        })
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.inner.borrow_mut().scale = if scale > 0.0 { scale } else { 1.0 };
    }

    pub fn scale(&self) -> f32 {
        self.inner.borrow().scale
    }

    pub fn adapter_name(&self) -> String {
        self.inner.borrow().adapter_name.clone()
    }
}

impl Renderer for GpuRenderer {
    fn width(&self) -> u32 {
        self.inner.borrow().width
    }

    fn height(&self) -> u32 {
        self.inner.borrow().height
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        let mut inner = self.inner.borrow_mut();
        let width = width.max(1);
        let height = height.max(1);
        if inner.width != width || inner.height != height {
            inner.width = width;
            inner.height = height;
            inner.pixels = None;
        }
        Ok(())
    }

    fn begin_frame(&mut self, clear: TektoColor) {
        let mut inner = self.inner.borrow_mut();
        inner.scene.reset();
        inner.clip_depth = 0;
        inner.clear = to_color(clear);
        inner.pixels = None;
    }

    fn fill_rect(&mut self, rect: Rect, color: TektoColor) {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        let shape = phys_rect(rect, inner.scale);
        inner.scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            to_color(color),
            None,
            &shape,
        );
        inner.pixels = None;
    }

    fn draw_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: TektoColor,
        max_width: Option<f32>,
    ) {
        if text.is_empty() {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        let s = inner.scale;
        let brush = [color.r, color.g, color.b, color.a];
        let layout = inner
            .text
            .layout(text, size * s, brush, max_width.map(|w| w * s));
        let vello_color = to_color(color);
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                draw_glyph_run(&mut inner.scene, &glyph_run, x * s, y * s, vello_color);
            }
        }
        inner.pixels = None;
    }

    fn push_clip(&mut self, rect: Rect) {
        let mut inner = self.inner.borrow_mut();
        let shape = phys_rect(rect, inner.scale);
        inner
            .scene
            .push_clip_layer(Fill::NonZero, Affine::IDENTITY, &shape);
        inner.clip_depth += 1;
        inner.pixels = None;
    }

    fn pop_clip(&mut self) {
        let mut inner = self.inner.borrow_mut();
        if inner.clip_depth > 0 {
            inner.scene.pop_layer();
            inner.clip_depth -= 1;
            inner.pixels = None;
        }
    }

    fn to_png(&self) -> Result<Vec<u8>, RenderError> {
        let rgba = self.rgba8()?;
        let inner = self.inner.borrow();
        encode_png(inner.width, inner.height, &rgba)
    }

    fn rgba8(&self) -> Result<Vec<u8>, RenderError> {
        let mut inner = self.inner.borrow_mut();
        if let Some(pixels) = &inner.pixels {
            return Ok(pixels.clone());
        }
        while inner.clip_depth > 0 {
            inner.scene.pop_layer();
            inner.clip_depth -= 1;
        }
        let pixels = readback(&mut inner)?;
        inner.pixels = Some(pixels.clone());
        Ok(pixels)
    }
}

fn draw_glyph_run(
    scene: &mut Scene,
    glyph_run: &parley::GlyphRun<'_, [u8; 4]>,
    ox: f32,
    oy: f32,
    color: vello::peniko::Color,
) {
    let mut run_x = glyph_run.offset();
    let run_y = glyph_run.baseline();
    let run = glyph_run.run();
    let font = run.font();
    let font_size = run.font_size();
    let coords: Vec<i16> = run
        .normalized_coords()
        .iter()
        .map(|coord| coord.to_bits())
        .collect();
    let mut glyphs = Vec::new();
    for glyph in glyph_run.glyphs() {
        let gx = ox + run_x + glyph.x;
        let gy = oy + run_y + glyph.y;
        run_x += glyph.advance;
        glyphs.push(Glyph {
            id: glyph.id,
            x: gx,
            y: gy,
        });
    }
    if glyphs.is_empty() {
        return;
    }
    let font: &FontData = font;
    scene
        .draw_glyphs(font)
        .font_size(font_size)
        .hint(false)
        .normalized_coords(&coords)
        .brush(color)
        .draw(Fill::NonZero, glyphs.into_iter());
}

fn readback(inner: &mut Inner) -> Result<Vec<u8>, RenderError> {
    let width = inner.width;
    let height = inner.height;
    let texture = inner.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tekto-gpu-target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    inner
        .renderer
        .render_to_texture(
            &inner.device,
            &inner.queue,
            &inner.scene,
            &view,
            &RenderParams {
                base_color: inner.clear,
                width,
                height,
                antialiasing_method: AaConfig::Area,
            },
        )
        .map_err(|err| RenderError::msg(format!("vello render: {err}")))?;

    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let unpadded = width.saturating_mul(4);
    let padded = unpadded.div_ceil(align) * align;
    let buffer = inner.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("tekto-gpu-readback"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = inner
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("tekto-gpu-copy"),
        });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let submission = inner.queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    inner
        .device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(Duration::from_secs(10)),
        })
        .map_err(|err| RenderError::msg(format!("GPU poll: {err}")))?;
    rx.recv()
        .map_err(|err| RenderError::msg(format!("GPU map: {err}")))?
        .map_err(|err| RenderError::msg(format!("GPU map: {err}")))?;
    let mapped = slice.get_mapped_range();
    let mut out = vec![0u8; (width * height * 4) as usize];
    for row in 0..height as usize {
        let src = row * padded as usize;
        let dst = row * unpadded as usize;
        out[dst..dst + unpadded as usize].copy_from_slice(&mapped[src..src + unpadded as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(out)
}

fn phys_rect(rect: Rect, scale: f32) -> VelloRect {
    VelloRect::new(
        f64::from(rect.x * scale),
        f64::from(rect.y * scale),
        f64::from((rect.x + rect.width) * scale),
        f64::from((rect.y + rect.height) * scale),
    )
}

fn to_color(color: TektoColor) -> vello::peniko::Color {
    vello::peniko::Color::from_rgba8(color.r, color.g, color.b, color.a)
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, RenderError> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|err| RenderError::msg(err.to_string()))?;
    writer
        .write_image_data(rgba)
        .map_err(|err| RenderError::msg(err.to_string()))?;
    drop(writer);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tekto_tree::{Color, Rect};

    #[test]
    fn gpu_paints_rect_text_and_clip_or_reports_no_adapter() {
        let mut renderer = match GpuRenderer::new(64, 48) {
            Ok(renderer) => renderer,
            Err(err) => {
                let message = err.to_string();
                assert!(
                    !message.contains("not enabled"),
                    "gpu feature is on, but the renderer is still the stub: {message}"
                );
                eprintln!("gpu adapter unavailable: {message}");
                return;
            }
        };
        renderer.begin_frame(Color::PAGE);
        renderer.fill_rect(Rect::new(0.0, 0.0, 40.0, 30.0), Color::BANNER);
        renderer.push_clip(Rect::new(0.0, 0.0, 10.0, 48.0));
        renderer.fill_rect(Rect::new(0.0, 0.0, 40.0, 30.0), Color::INK);
        renderer.pop_clip();
        renderer.draw_text("Hi", 12.0, 4.0, 14.0, Color::INK, Some(40.0));
        let png = renderer.to_png().expect("png");
        assert!(png.starts_with(b"\x89PNG"), "not a png");
        let rgba = renderer.rgba8().expect("rgba");
        // Clipped ink covers the left of the banner. A pixel just inside
        // that clip should not still be the banner color.
        let ink = ((8 * 64 + 4) * 4) as usize;
        assert_ne!(
            &rgba[ink..ink + 3],
            &[Color::BANNER.r, Color::BANNER.g, Color::BANNER.b]
        );
        let shot = std::path::PathBuf::from("/workspace/tektogui-shots/i32-gpu-rect.png");
        if let Some(parent) = shot.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&shot, &png);
    }
}
