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

/// Canvas type for the wasm WebGPU present path.
#[cfg(target_arch = "wasm32")]
pub type WebCanvas = wgpu::web_sys::HtmlCanvasElement;

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
    /// Last uploaded image keyed by [`tekto_image::Image::id`]. The pixel
    /// copy and format conversion happen once per image, not once per frame.
    image: Option<(u64, vello::peniko::ImageData)>,
}

pub struct GpuRenderer {
    inner: RefCell<Inner>,
}

impl GpuRenderer {
    /// Desktop (and any host with a blocking executor). The browser path is
    /// [`paint_tree_webgpu`]: `pollster` cannot wait on a WebGPU adapter.
    ///
    /// On Android the window calls this only when `TEKTO_ANDROID_GPU=1`.
    /// There is no on-screen wgpu surface here: pixels are read back and
    /// the caller blits them. A lost `ANativeWindow` is the presenter's
    /// problem; this function returns an error instead of panicking when
    /// no adapter or device exists.
    #[cfg(not(target_arch = "wasm32"))]
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
        let desc = device_desc(&adapter);
        let (device, queue) = pollster::block_on(adapter.request_device(&desc))
            .map_err(|err| RenderError::msg(format!("GPU device: {err}")))?;
        Self::from_device(width, height, device, queue, adapter_name)
    }

    /// Sync setup is the desktop path. The wasm host calls [`paint_tree_webgpu`].
    #[cfg(target_arch = "wasm32")]
    pub fn new(_width: u32, _height: u32) -> Result<Self, RenderError> {
        Err(RenderError::msg(
            "WebGPU paint is async; call paint_tree_webgpu from the host",
        ))
    }

    fn from_device(
        width: u32,
        height: u32,
        device: wgpu::Device,
        queue: wgpu::Queue,
        adapter_name: String,
    ) -> Result<Self, RenderError> {
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
                image: None,
            }),
        })
    }

    /// Rasterize the recorded scene and copy it onto `target` (a surface texture).
    pub fn present_to(
        &self,
        target: &wgpu::Texture,
        target_format: wgpu::TextureFormat,
    ) -> Result<(), RenderError> {
        let mut inner = self.inner.borrow_mut();
        while inner.clip_depth > 0 {
            inner.scene.pop_layer();
            inner.clip_depth -= 1;
        }
        let src = color_target(&mut inner)?;
        blit_texture(&inner, &src, target, target_format)
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
    fn draw_image(
        &mut self,
        image: &tekto_image::Image,
        bounds: Rect,
        fit: tekto_image::Fit,
        opacity: f32,
    ) {
        let Some((x, y, width, height)) = tekto_image::fit(image, bounds.width, bounds.height, fit)
        else {
            return;
        };
        if !bounds.x.is_finite() || !bounds.y.is_finite() || !opacity.is_finite() {
            return;
        }
        self.push_clip(bounds);
        {
            let mut inner = self.inner.borrow_mut();
            let cached = inner
                .image
                .as_ref()
                .filter(|(id, _)| *id == image.id())
                .map(|(_, data)| data.clone());
            let data = match cached {
                Some(data) => data,
                None => {
                    let data = vello::peniko::ImageData {
                        data: image.pixels().to_vec().into(),
                        format: vello::peniko::ImageFormat::Rgba8,
                        alpha_type: vello::peniko::ImageAlphaType::Alpha,
                        width: image.width(),
                        height: image.height(),
                    };
                    inner.image = Some((image.id(), data.clone()));
                    data
                }
            };
            let s = inner.scale;
            let transform = Affine::new([
                f64::from(width / image.width() as f32 * s),
                0.0,
                0.0,
                f64::from(height / image.height() as f32 * s),
                f64::from((bounds.x + x) * s),
                f64::from((bounds.y + y) * s),
            ]);
            let brush = vello::peniko::ImageBrush::new(data)
                .with_quality(vello::peniko::ImageQuality::Low)
                .multiply_alpha(opacity.clamp(0.0, 1.0));
            inner.scene.draw_image(brush.as_ref(), transform);
            inner.pixels = None;
        }
        self.pop_clip();
    }
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

    fn fill_rrect(&mut self, rect: Rect, radius: f32, color: TektoColor) {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        let s = inner.scale;
        let r = (radius.max(0.0).min(rect.width.min(rect.height) / 2.0) * s) as f64;
        let shape = vello::kurbo::RoundedRect::new(
            (rect.x * s) as f64,
            (rect.y * s) as f64,
            ((rect.x + rect.width) * s) as f64,
            ((rect.y + rect.height) * s) as f64,
            r,
        );
        inner.scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            to_color(color),
            None,
            &shape,
        );
        inner.pixels = None;
    }

    fn stroke_rrect(&mut self, rect: Rect, radius: f32, width: f32, color: TektoColor) {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        let s = inner.scale;
        let stroke_width = width.max(0.0);
        if stroke_width <= 0.0 {
            return;
        }
        // Logical coordinates, scaled by the transform like `draw_line`.
        let r = f64::from(radius.max(0.0).min(rect.width.min(rect.height) / 2.0));
        let inset = f64::from(stroke_width / 2.0);
        let shape = vello::kurbo::RoundedRect::new(
            f64::from(rect.x + stroke_width / 2.0),
            f64::from(rect.y + stroke_width / 2.0),
            f64::from(rect.x + rect.width - stroke_width / 2.0),
            f64::from(rect.y + rect.height - stroke_width / 2.0),
            (r - inset).max(0.0),
        );
        let stroke = vello::kurbo::Stroke::new(f64::from(stroke_width));
        inner.scene.stroke(
            &stroke,
            Affine::scale(f64::from(s)),
            to_color(color),
            None,
            &shape,
        );
        inner.pixels = None;
    }

    fn draw_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, thickness: f32, color: TektoColor) {
        let mut inner = self.inner.borrow_mut();
        let s = inner.scale;
        let stroke = vello::kurbo::Stroke::new(thickness.max(0.0) as f64);
        let shape = vello::kurbo::Line::new(
            vello::kurbo::Point::new(x0 as f64, y0 as f64),
            vello::kurbo::Point::new(x1 as f64, y1 as f64),
        );
        inner.scene.stroke(
            &stroke,
            vello::kurbo::Affine::scale(s as f64),
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

fn device_desc(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'_> {
    wgpu::DeviceDescriptor {
        label: Some("tekto-gpu"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }
}

fn color_target(inner: &mut Inner) -> Result<wgpu::Texture, RenderError> {
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
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
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
    Ok(texture)
}

const BLIT_WGSL: &str = r#"
struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
  // One oversized triangle. NDC y is up; uv y 0 is the top of the texture.
  var clip = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
  );
  var out: VsOut;
  out.pos = vec4<f32>(clip[i], 0.0, 1.0);
  out.uv = vec2<f32>((clip[i].x + 1.0) * 0.5, (1.0 - clip[i].y) * 0.5);
  return out;
}

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
  return textureSample(src_tex, src_samp, in.uv);
}
"#;

fn blit_texture(
    inner: &Inner,
    src: &wgpu::Texture,
    target: &wgpu::Texture,
    target_format: wgpu::TextureFormat,
) -> Result<(), RenderError> {
    let device = &inner.device;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("tekto-blit"),
        source: wgpu::ShaderSource::Wgsl(BLIT_WGSL.into()),
    });
    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("tekto-blit"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("tekto-blit"),
        bind_group_layouts: &[Some(&bind_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("tekto-blit"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let view = src.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("tekto-blit"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..wgpu::SamplerDescriptor::default()
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tekto-blit"),
        layout: &bind_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let dest = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("tekto-gpu-present-blit"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tekto-blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &dest,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
    inner.queue.submit([encoder.finish()]);
    Ok(())
}

fn readback(inner: &mut Inner) -> Result<Vec<u8>, RenderError> {
    let width = inner.width;
    let height = inner.height;
    let texture = color_target(inner)?;

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

/// Paint `tree` with the same Vello path as desktop, onto a browser canvas.
///
/// Returns an error when WebGPU has no adapter so the host can blit the CPU
/// PNG instead. WebGL2 is not used here: Vello needs compute shaders.
#[cfg(target_arch = "wasm32")]
pub async fn paint_tree_webgpu(
    tree: &tekto_tree::Tree,
    canvas: WebCanvas,
    width: u32,
    height: u32,
    clear: TektoColor,
) -> Result<(), RenderError> {
    if width == 0 || height == 0 {
        return Err(RenderError::msg("width and height must be non-zero"));
    }
    if !wgpu::util::is_browser_webgpu_supported().await {
        return Err(RenderError::msg("no WebGPU adapter"));
    }
    canvas.set_width(width);
    canvas.set_height(height);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
        .map_err(|err| RenderError::msg(format!("webgpu surface: {err}")))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|err| RenderError::msg(format!("no WebGPU adapter: {err}")))?;
    let adapter_name = adapter.get_info().name;
    let desc = device_desc(&adapter);
    let (device, queue) = adapter
        .request_device(&desc)
        .await
        .map_err(|err| RenderError::msg(format!("GPU device: {err}")))?;
    let config = surface
        .get_default_config(&adapter, width, height)
        .ok_or_else(|| RenderError::msg("canvas surface is not supported"))?;
    surface.configure(&device, &config);
    let mut renderer = GpuRenderer::from_device(width, height, device, queue, adapter_name)?;
    crate::paint_tree(&mut renderer, tree, clear);
    let frame = match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
        wgpu::CurrentSurfaceTexture::Timeout => {
            return Err(RenderError::msg("surface timeout"));
        }
        wgpu::CurrentSurfaceTexture::Occluded => {
            return Err(RenderError::msg("surface occluded"));
        }
        wgpu::CurrentSurfaceTexture::Outdated => {
            return Err(RenderError::msg("surface outdated"));
        }
        wgpu::CurrentSurfaceTexture::Lost => return Err(RenderError::msg("surface lost")),
        wgpu::CurrentSurfaceTexture::Validation => {
            return Err(RenderError::msg("surface validation"));
        }
    };
    renderer.present_to(&frame.texture, config.format)?;
    frame.present();
    Ok(())
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

    #[cfg(feature = "cpu")]
    #[test]
    fn gpu_matches_cpu_per_primitive() {
        use crate::CpuRenderer;
        use tekto_image::{Fit, Image};
        let mut gpu = match GpuRenderer::new(96, 64) {
            Ok(renderer) => renderer,
            Err(err) => {
                eprintln!("gpu adapter unavailable: {err}");
                return;
            }
        };
        let mut cpu = CpuRenderer::new(96, 64).unwrap();
        let mut px = Vec::new();
        for i in 0..16u32 {
            let on = (i / 4 + i % 4) % 2 == 0;
            px.extend_from_slice(if on {
                &[200, 40, 40, 255]
            } else {
                &[30, 60, 200, 255]
            });
        }
        let image = Image::new(4, 4, px).unwrap();
        type Case = (&'static str, fn(&mut dyn Renderer, &Image));
        let cases: [Case; 8] = [
            ("fill_rect", |r, _| {
                r.fill_rect(Rect::new(8.0, 8.0, 40.0, 24.0), Color::BANNER)
            }),
            ("fill_rrect", |r, _| {
                r.fill_rrect(Rect::new(8.0, 8.0, 60.0, 40.0), 8.0, Color::BANNER)
            }),
            ("stroke_rrect", |r, _| {
                r.stroke_rrect(Rect::new(8.0, 8.0, 60.0, 40.0), 6.0, 2.0, Color::INK)
            }),
            ("draw_line", |r, _| {
                r.draw_line(6.0, 6.0, 80.0, 50.0, 3.0, Color::INK)
            }),
            ("nested_clip", |r, _| {
                r.push_clip(Rect::new(4.0, 4.0, 60.0, 40.0));
                r.push_clip(Rect::new(20.0, 10.0, 70.0, 40.0));
                r.fill_rect(Rect::new(0.0, 0.0, 96.0, 64.0), Color::BANNER);
                r.pop_clip();
                r.pop_clip();
            }),
            ("text_max_width", |r, _| {
                r.draw_text("Milk and eggs", 6.0, 10.0, 14.0, Color::INK, Some(50.0))
            }),
            ("image_contain", |r, img| {
                r.draw_image(img, Rect::new(4.0, 4.0, 60.0, 40.0), Fit::Contain, 1.0)
            }),
            ("image_cover_alpha", |r, img| {
                r.draw_image(img, Rect::new(4.0, 4.0, 60.0, 40.0), Fit::Cover, 0.5)
            }),
        ];
        for (name, draw) in cases
            .iter()
            .flat_map(|c| [(c, 1.0_f32), (c, 1.25)])
            .map(|(c, scale)| ((c.0, c.1, scale), ()))
            .map(|(t, _)| (t.0, (t.1, t.2)))
        {
            let (draw, scale) = draw;
            cpu.set_scale(scale);
            gpu.set_scale(scale);
            cpu.begin_frame(Color::PAGE);
            gpu.begin_frame(Color::PAGE);
            draw(&mut cpu, &image);
            draw(&mut gpu, &image);
            let (a, b) = (cpu.rgba8().unwrap(), gpu.rgba8().unwrap());
            let differing = a
                .as_chunks::<4>()
                .0
                .iter()
                .zip(b.as_chunks::<4>().0.iter())
                .filter(|(x, y)| x.iter().zip(*y).any(|(p, q)| p.abs_diff(*q) > 48))
                .count();
            let total = a.len() / 4;
            assert!(
                differing * 100 < total * 3,
                "{name} at scale {scale}: cpu and gpu differ on {differing} of {total} pixels"
            );
        }
    }
}
