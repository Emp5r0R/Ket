//! Offline QR reading for invites, adapted from Abyssal's shared Rust QR code.
//!
//! Images are decoded within fixed size and allocation bounds, normalized to
//! luminance, then read with a short list of repairs (inverted, light-gap
//! closing, halving) so screenshots of terminal QR codes and photos still
//! read. Decoded text is inert data: callers must verify it as an invite.

mod decode;
mod raster;

pub use decode::{MAX_QR_TEXT_BYTES, decode_qr_luminance};
pub use raster::{MAX_QR_IMAGE_BYTES, QrImage, normalize_qr_image};

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum QrError {
    #[error("the image is not a supported PNG or JPEG within the size limits")]
    UnsupportedImage,
    #[error("the QR frame is malformed")]
    InvalidFrame,
}

/// Reads the first QR code in a PNG or JPEG image; `Ok(None)` when none is found.
pub fn decode_qr_image(bytes: &[u8]) -> Result<Option<String>, QrError> {
    let image = normalize_qr_image(bytes)?;
    decode_qr_luminance(&image.luminance, image.width, image.height, true)
}

#[cfg(test)]
mod tests {
    use image::{ColorType, ImageEncoder};
    use qrcode::{Color, EcLevel, QrCode};

    use super::*;

    /// Renders text as a PNG the way a terminal screenshot would look.
    fn qr_png(text: &str, scale: usize, margin: usize) -> Vec<u8> {
        let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).unwrap();
        let modules = code.width();
        let colors = code.to_colors();
        let side = (modules + 2 * margin) * scale;
        let mut pixels = vec![255_u8; side * side];
        for y in 0..side {
            for x in 0..side {
                let (module_x, module_y) = (x / scale, y / scale);
                let inside = (margin..modules + margin).contains(&module_x)
                    && (margin..modules + margin).contains(&module_y);
                if inside
                    && colors[(module_y - margin) * modules + module_x - margin] == Color::Dark
                {
                    pixels[y * side + x] = 0;
                }
            }
        }
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(&pixels, side as u32, side as u32, ColorType::L8.into())
            .unwrap();
        bytes
    }

    #[test]
    fn invite_screenshots_decode_end_to_end() {
        let invite = format!("ket:invite:{}", "Ab3dEf6hIj9kLm2nOp5q".repeat(12));
        assert_eq!(
            decode_qr_image(&qr_png(&invite, 6, 4)).unwrap(),
            Some(invite.clone())
        );
        // A large, downscaled screenshot still reads after area averaging.
        assert_eq!(
            decode_qr_image(&qr_png(&invite, 14, 4)).unwrap(),
            Some(invite)
        );
        assert_eq!(
            decode_qr_image(b"not an image"),
            Err(QrError::UnsupportedImage)
        );
    }
}
