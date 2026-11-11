//! Validated, app-supplied RGBA pixels. No decoder or network access.
use std::{fmt, sync::Arc};

/// Maximum dimension and allocation of a single image.
pub const MAX_DIMENSION: u32 = 8192;
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Image {
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageError(pub &'static str);
impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ImageError {}

impl Image {
    /// Decode a PNG into RGBA8. Enabled only by the `png` feature.
    #[cfg(feature = "png")]
    pub fn from_png(bytes: &[u8]) -> Result<Self, ImageError> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|_| ImageError("invalid PNG"))?;
        let info = reader.info();
        if info.animation_control.is_some() {
            return Err(ImageError("animated PNG is unsupported"));
        }
        if info.width == 0
            || info.height == 0
            || info.width > MAX_DIMENSION
            || info.height > MAX_DIMENSION
            || (info.width as usize)
                .checked_mul(info.height as usize)
                .and_then(|n| n.checked_mul(4))
                .is_none_or(|n| n > MAX_BYTES)
        {
            return Err(ImageError("image is too large"));
        }
        let length = reader
            .output_buffer_size()
            .filter(|n| *n <= MAX_BYTES)
            .ok_or(ImageError("image is too large"))?;
        let mut decoded = vec![0; length];
        let frame = reader
            .next_frame(&mut decoded)
            .map_err(|_| ImageError("invalid PNG"))?;
        let bytes = &decoded[..frame.buffer_size()];
        let mut rgba = Vec::with_capacity(frame.width as usize * frame.height as usize * 4);
        match frame.color_type {
            png::ColorType::Rgba => rgba.extend_from_slice(bytes),
            png::ColorType::Rgb => {
                for pixel in bytes.as_chunks::<3>().0 {
                    rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                }
            }
            png::ColorType::Grayscale => {
                for &value in bytes {
                    rgba.extend_from_slice(&[value, value, value, 255]);
                }
            }
            png::ColorType::GrayscaleAlpha => {
                for pixel in bytes.as_chunks::<2>().0 {
                    rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
                }
            }
            png::ColorType::Indexed => return Err(ImageError("unsupported PNG color format")),
        }
        Self::new(frame.width, frame.height, rgba)
    }
    /// Construct from straight-alpha, row-major RGBA8 bytes.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ImageError> {
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(ImageError("invalid image dimensions"));
        }
        let len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .filter(|n| *n <= MAX_BYTES)
            .ok_or(ImageError("image is too large"))?;
        if pixels.len() != len {
            return Err(ImageError("RGBA byte length does not match dimensions"));
        }
        Ok(Self {
            width,
            height,
            pixels: pixels.into(),
        })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Fit {
    #[default]
    Contain,
    Cover,
}

/// Destination rectangle for a centered image; cover must be clipped to the viewport.
pub fn fit(image: &Image, width: f32, height: f32, mode: Fit) -> Option<(f32, f32, f32, f32)> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let x_scale = width / image.width as f32;
    let y_scale = height / image.height as f32;
    let scale = match mode {
        Fit::Contain => x_scale.min(y_scale),
        Fit::Cover => x_scale.max(y_scale),
    };
    let w = image.width as f32 * scale;
    let h = image.height as f32 * scale;
    Some(((width - w) / 2.0, (height - h) / 2.0, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_size_and_fits() {
        assert!(Image::new(0, 1, vec![]).is_err());
        assert!(Image::new(1, 1, vec![0; 3]).is_err());
        assert!(Image::new(8192, 8192, vec![]).is_err());
        let image = Image::new(2, 1, vec![255; 8]).unwrap();
        assert_eq!(
            fit(&image, 100.0, 100.0, Fit::Contain),
            Some((0.0, 25.0, 100.0, 50.0))
        );
        assert_eq!(
            fit(&image, 100.0, 100.0, Fit::Cover),
            Some((-50.0, 0.0, 200.0, 100.0))
        );
        assert_eq!(fit(&image, f32::NAN, 100.0, Fit::Cover), None);
    }

    #[cfg(feature = "png")]
    #[test]
    fn decodes_rgb_and_rejects_bad_data() {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 1, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[2, 4, 6])
                .unwrap();
        }
        assert_eq!(Image::from_png(&encoded).unwrap().pixels(), &[2, 4, 6, 255]);
        assert!(Image::from_png(b"not png").is_err());
    }
}
