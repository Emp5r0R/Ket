//! Bounded in-memory raster normalization for offline QR imports.

use std::io::Cursor;

use image::{ColorType, ImageDecoder, ImageFormat, ImageReader, Limits};
use zeroize::Zeroizing;

use crate::QrError;

pub const MAX_QR_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PIXELS: u64 = 4 * 1024 * 1024;
const MAX_SIDE: u32 = 4096;
const MAX_DECODER_ALLOCATION: u64 = 32 * 1024 * 1024;
const OUTPUT_SIDE: u32 = 960;

/// An 8-bit luminance raster no larger than 960 pixels on its longest side.
pub struct QrImage {
    pub width: u32,
    pub height: u32,
    pub luminance: Zeroizing<Vec<u8>>,
}

/// Decodes a PNG or JPEG (sniffed from its bytes, never from a name or MIME
/// type) and area-averages it into a bounded luminance raster. SVG, HTML, and
/// every other format are rejected.
pub fn normalize_qr_image(bytes: &[u8]) -> Result<QrImage, QrError> {
    normalize(bytes).map_err(|()| QrError::UnsupportedImage)
}

fn normalize(bytes: &[u8]) -> Result<QrImage, ()> {
    if bytes.is_empty() || bytes.len() > MAX_QR_IMAGE_BYTES {
        return Err(());
    }
    let format = image::guess_format(bytes).map_err(|_| ())?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err(());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODER_ALLOCATION);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|_| ())?;
    let (width, height) = decoder.dimensions();
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE || pixels > MAX_PIXELS {
        return Err(());
    }
    let channels = match decoder.color_type() {
        ColorType::L8 => 1,
        ColorType::La8 => 2,
        ColorType::Rgb8 => 3,
        ColorType::Rgba8 => 4,
        _ => return Err(()),
    };
    let total = pixels * channels;
    if decoder.total_bytes() != total {
        return Err(());
    }
    // Bound our own allocation independently of the decoder limits.
    let mut decoded = Zeroizing::new(vec![0_u8; usize::try_from(total).map_err(|_| ())?]);
    decoder.read_image(&mut decoded).map_err(|_| ())?;
    let longest = width.max(height).max(OUTPUT_SIDE);
    let out_width = (width * OUTPUT_SIDE / longest).max(1);
    let out_height = (height * OUTPUT_SIDE / longest).max(1);
    let mut luminance = Zeroizing::new(vec![0_u8; (out_width * out_height) as usize]);
    // Area-average every source pixel into its output cell; point sampling
    // aliases dense screenshots and thin terminal line gaps into broken modules.
    for y in 0..out_height {
        let top = y * height / out_height;
        let bottom = ((y + 1) * height / out_height).max(top + 1);
        for x in 0..out_width {
            let left = x * width / out_width;
            let right = ((x + 1) * width / out_width).max(left + 1);
            let mut sum = 0_u64;
            for source_y in top..bottom {
                for source_x in left..right {
                    let source = ((u64::from(source_y) * u64::from(width) + u64::from(source_x))
                        * channels) as usize;
                    sum += u64::from(composited_luma(&decoded[source..], channels as usize));
                }
            }
            let count = u64::from(bottom - top) * u64::from(right - left);
            luminance[(y * out_width + x) as usize] = (sum / count) as u8;
        }
    }
    Ok(QrImage {
        width: out_width,
        height: out_height,
        luminance,
    })
}

/// Grey level of one pixel, composited over white so transparency reads as quiet zone.
fn composited_luma(pixel: &[u8], channels: usize) -> u32 {
    let luma = if channels <= 2 {
        u32::from(pixel[0])
    } else {
        (u32::from(pixel[0]) + 2 * u32::from(pixel[1]) + u32::from(pixel[2])) / 4
    };
    let alpha = if channels == 2 || channels == 4 {
        u32::from(pixel[channels - 1])
    } else {
        255
    };
    (luma * alpha + 255 * (255 - alpha)) / 255
}

#[cfg(test)]
mod tests {
    use image::{ImageBuffer, ImageEncoder, Luma};

    use super::*;

    fn png(pixels: &[u8], width: u32, height: u32, color: ColorType) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(pixels, width, height, color.into())
            .unwrap();
        bytes
    }

    #[test]
    fn only_bounded_png_and_jpeg_rasters_are_accepted() {
        let decoded = normalize_qr_image(&png(&[0, 255, 255, 0], 2, 2, ColorType::L8)).unwrap();
        assert_eq!(decoded.luminance.as_slice(), &[0, 255, 255, 0]);
        for rejected in [
            b"<svg><image href='file:///etc/passwd'/></svg>".to_vec(),
            b"https://evil.example/image.png".to_vec(),
            b"GIF89a".to_vec(),
            vec![0; MAX_QR_IMAGE_BYTES + 1],
            Vec::new(),
        ] {
            assert_eq!(
                normalize_qr_image(&rejected).err(),
                Some(QrError::UnsupportedImage)
            );
        }
        let mut truncated = png(&[0, 255, 255, 0], 2, 2, ColorType::L8);
        truncated.truncate(35);
        assert!(normalize_qr_image(&truncated).is_err());
    }

    #[test]
    fn dimension_bombs_fail_before_output_allocation() {
        let image = ImageBuffer::<Luma<u8>, _>::from_pixel(1, MAX_SIDE + 1, Luma([0]));
        let bytes = png(image.as_raw(), image.width(), image.height(), ColorType::L8);
        assert!(normalize_qr_image(&bytes).is_err());
    }

    #[test]
    fn transparency_composites_onto_white_and_jpeg_is_supported() {
        let bytes = png(&[0, 0, 0, 0, 0, 0, 0, 255], 2, 1, ColorType::Rgba8);
        assert_eq!(
            normalize_qr_image(&bytes).unwrap().luminance.as_slice(),
            &[255, 0]
        );
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .write_image(&[255; 12], 2, 2, ColorType::Rgb8.into())
            .unwrap();
        assert_eq!(normalize_qr_image(&jpeg).unwrap().width, 2);
    }

    #[test]
    fn large_images_are_area_averaged_to_the_output_bound() {
        let image = ImageBuffer::<Luma<u8>, _>::from_pixel(1_920, 1_080, Luma([0]));
        let bytes = png(image.as_raw(), 1_920, 1_080, ColorType::L8);
        let normalized = normalize_qr_image(&bytes).unwrap();
        assert_eq!((normalized.width, normalized.height), (960, 540));
    }
}
