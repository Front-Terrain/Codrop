// Copyright (c) 2026 Front Terrain Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::error::CodropError;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use std::io::Cursor;

/// Supported image formats for Codrop Image compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CodropImageFormat {
    /// Auto-detect best format (defaults to WebP for maximum compression)
    Auto = 0,
    /// WebP format (75% - 90% savings)
    WebP = 1,
    /// Optimized PNG format (lossless / filtered)
    Png = 2,
    /// JPEG format with configurable quality
    Jpeg = 3,
}

impl CodropImageFormat {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => CodropImageFormat::WebP,
            2 => CodropImageFormat::Png,
            3 => CodropImageFormat::Jpeg,
            _ => CodropImageFormat::Auto,
        }
    }
}

/// Compression options for images.
#[derive(Debug, Clone)]
pub struct ImageOptions {
    pub format: CodropImageFormat,
    /// Quality factor from 1 to 100 (Default: 85 for visually lossless)
    pub quality: u8,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            format: CodropImageFormat::Auto,
            quality: 85,
        }
    }
}

/// Compress an image buffer using Codrop's image compression pipeline.
pub fn compress_image(data: &[u8], options: ImageOptions) -> Result<Vec<u8>, CodropError> {
    if data.is_empty() {
        return Err(CodropError::ImageError(
            "Input image buffer is empty".to_string(),
        ));
    }

    let dyn_img = image::load_from_memory(data)
        .map_err(|e| CodropError::ImageError(format!("Failed to decode image: {}", e)))?;

    let target_format = match options.format {
        CodropImageFormat::Auto => CodropImageFormat::WebP,
        other => other,
    };

    let mut output = Vec::new();
    let mut cursor = Cursor::new(&mut output);

    match target_format {
        CodropImageFormat::WebP | CodropImageFormat::Auto => {
            let encoder = WebPEncoder::new_lossless(&mut cursor);
            dyn_img
                .write_with_encoder(encoder)
                .map_err(|e| CodropError::ImageError(format!("WebP encoding failed: {}", e)))?;
        }
        CodropImageFormat::Jpeg => {
            let quality = options.quality.clamp(1, 100);
            let encoder = JpegEncoder::new_with_quality(&mut cursor, quality);
            dyn_img
                .write_with_encoder(encoder)
                .map_err(|e| CodropError::ImageError(format!("JPEG encoding failed: {}", e)))?;
        }
        CodropImageFormat::Png => {
            let encoder = PngEncoder::new(&mut cursor);
            dyn_img
                .write_with_encoder(encoder)
                .map_err(|e| CodropError::ImageError(format!("PNG encoding failed: {}", e)))?;
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn test_compress_image_empty() {
        let err = compress_image(&[], ImageOptions::default()).unwrap_err();
        match err {
            CodropError::ImageError(_) => {}
            _ => panic!("Expected ImageError"),
        }
    }

    #[test]
    fn test_compress_synthetic_image() {
        // Create a 16x16 red image
        let mut img = RgbaImage::new(16, 16);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 0, 0, 255]);
        }

        let mut png_bytes = Vec::new();
        img.write_with_encoder(PngEncoder::new(&mut png_bytes))
            .unwrap();

        // Compress to WebP
        let webp_compressed = compress_image(
            &png_bytes,
            ImageOptions {
                format: CodropImageFormat::WebP,
                quality: 85,
            },
        )
        .unwrap();

        assert!(!webp_compressed.is_empty());
        assert_eq!(&webp_compressed[0..4], b"RIFF");
        assert_eq!(&webp_compressed[8..12], b"WEBP");

        // Compress to JPEG
        let jpeg_compressed = compress_image(
            &png_bytes,
            ImageOptions {
                format: CodropImageFormat::Jpeg,
                quality: 80,
            },
        )
        .unwrap();
        assert!(!jpeg_compressed.is_empty());
        assert_eq!(&jpeg_compressed[0..2], &[0xFF, 0xD8]);
    }
}
