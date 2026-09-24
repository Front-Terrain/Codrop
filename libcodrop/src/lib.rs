#![forbid(unsafe_code)]

pub mod analysis;
pub mod checksum;
pub mod codec;
pub mod entropy;
pub mod error;
pub mod format;
pub mod streaming;

use std::io::Cursor;

pub use analysis::classifier::CompressionLevel;
pub use error::CodropError;
pub use format::{BlockHeader, BlockType, HeaderFlags, StreamHeader};
pub use streaming::{Decoder, DecoderOptions, Encoder, EncoderOptions};

/// Compress an in-memory byte slice using the requested compression level.
/// Returns the complete `.cdp` container bytes.
pub fn compress(data: &[u8], level: CompressionLevel) -> Result<Vec<u8>, CodropError> {
    let options = EncoderOptions {
        level,
        block_size: streaming::DEFAULT_BLOCK_SIZE,
        include_block_checksum: true,
        include_stream_checksum: true,
        known_uncompressed_size: Some(data.len() as u64),
    };

    let mut output = Vec::with_capacity(data.len() / 2 + 64);
    let mut encoder = Encoder::new(&mut output, options);
    encoder.write_chunk(data)?;
    encoder.finish()?;

    Ok(output)
}

/// Decompress a `.cdp` container from an in-memory byte slice.
pub fn decompress(compressed: &[u8]) -> Result<Vec<u8>, CodropError> {
    decompress_with_limit(compressed, 1024 * 1024 * 1024) // 1 GB default safety bound
}

/// Decompress a `.cdp` container with an explicit output safety limit.
pub fn decompress_with_limit(compressed: &[u8], max_output_bytes: u64) -> Result<Vec<u8>, CodropError> {
    let options = DecoderOptions {
        max_output_bytes: Some(max_output_bytes),
        verify_block_checksums: true,
        verify_stream_checksum: true,
    };

    let cursor = Cursor::new(compressed);
    let mut decoder = Decoder::new(cursor, options)?;
    let mut output = Vec::new();
    decoder.decompress_to(&mut output)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_empty() {
        let empty: &[u8] = b"";
        let c = compress(empty, CompressionLevel::Balanced).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, empty);
    }

    #[test]
    fn test_roundtrip_text_fast() {
        let text = b"Codrop is a universal adaptive compression system designed for cross-platform efficiency.";
        let c = compress(text, CompressionLevel::Fast).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, text);
    }

    #[test]
    fn test_roundtrip_text_balanced() {
        let text = b"Codrop is a universal adaptive compression system designed for cross-platform efficiency. Repeat: Codrop is a universal adaptive compression system designed for cross-platform efficiency.";
        let c = compress(text, CompressionLevel::Balanced).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, text);
    }

    #[test]
    fn test_roundtrip_repetitive_rle() {
        let data = vec![0x42; 20000];
        let c = compress(&data, CompressionLevel::Auto).unwrap();
        assert!(c.len() < 100, "RLE should compress 20,000 bytes down to <100 bytes, got {}", c.len());
        let d = decompress(&c).unwrap();
        assert_eq!(d, data);
    }

    #[test]
    fn test_roundtrip_incompressible_raw() {
        // High entropy data
        let mut pseudo_random = Vec::with_capacity(8192);
        let mut seed: u32 = 0x87654321;
        for _ in 0..8192 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            pseudo_random.push((seed >> 24) as u8);
        }

        let c = compress(&pseudo_random, CompressionLevel::Auto).unwrap();
        // Zero expansion guarantee: container overhead should be minimal (< 32 bytes)
        assert!(c.len() <= pseudo_random.len() + 32);
        let d = decompress(&c).unwrap();
        assert_eq!(d, pseudo_random);
    }

    #[test]
    fn test_roundtrip_multiblock_streaming() {
        // Create 350 KB payload to force multiple 128 KB blocks
        let mut large = Vec::with_capacity(350 * 1024);
        for i in 0..(350 * 1024) {
            large.push((i % 251) as u8);
        }

        let c = compress(&large, CompressionLevel::Fast).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, large);
    }
}
