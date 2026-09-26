#![deny(unsafe_code)]

pub mod checksum;
pub mod codec;
pub mod entropy;
pub mod error;
pub mod ffi;
pub mod format;
pub mod image_codec;
pub mod prefilter;
pub mod streaming;

use std::io::Cursor;

pub use error::CodropError;
pub use format::{BlockHeader, BlockType, HeaderFlags, StreamHeader};
pub use image_codec::{compress_image, CodropImageFormat, ImageOptions};
pub use streaming::{
    CompressionLevel, Decoder, DecoderOptions, Encoder, EncoderOptions, DEFAULT_BLOCK_SIZE,
};

/// Compress an in-memory byte slice into a valid `.cdp` container.
pub fn compress(data: &[u8], level: CompressionLevel) -> Result<Vec<u8>, CodropError> {
    let options = EncoderOptions {
        level,
        block_size: DEFAULT_BLOCK_SIZE,
        include_block_checksum: true,
        include_stream_checksum: true,
        known_uncompressed_size: Some(data.len() as u64),
    };

    let mut output = Vec::with_capacity(data.len() + 64);
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
pub fn decompress_with_limit(
    compressed: &[u8],
    max_output_bytes: u64,
) -> Result<Vec<u8>, CodropError> {
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
        let c = compress(empty, CompressionLevel::Auto).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, empty);
    }

    #[test]
    fn test_roundtrip_single_byte_zero() {
        let data = [0u8];
        let c = compress(&data, CompressionLevel::Fast).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, data);
    }

    #[test]
    fn test_roundtrip_single_byte_max() {
        let data = [255u8];
        let c = compress(&data, CompressionLevel::Balanced).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, data);
    }

    #[test]
    fn test_roundtrip_small_text() {
        let text = b"hello world";
        let c = compress(text, CompressionLevel::Auto).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, text);
    }

    #[test]
    fn test_roundtrip_repetitive_rle() {
        let data = b"AAAAAAAAAAAAAAAAAAAAAAAA";
        let c = compress(data, CompressionLevel::Auto).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, data);
    }

    #[test]
    fn test_roundtrip_alternating() {
        let data = b"ABABABABABABABAB";
        let c = compress(data, CompressionLevel::Compact).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, data);
    }

    #[test]
    fn test_roundtrip_structured_json() {
        let json = br#"{"name":"codrop","version":1}"#;
        let c = compress(json, CompressionLevel::Balanced).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, json);
    }

    #[test]
    fn test_roundtrip_binary_all_bytes() {
        let mut all_bytes = Vec::with_capacity(256);
        for b in 0..=255u8 {
            all_bytes.push(b);
        }
        let c = compress(&all_bytes, CompressionLevel::Auto).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, all_bytes);
    }

    #[test]
    fn test_roundtrip_large_deterministic_buffers() {
        for &size in &[1024, 64 * 1024, 128 * 1024, 256 * 1024] {
            let mut buf = Vec::with_capacity(size);
            for i in 0..size {
                buf.push((i % 251) as u8);
            }
            let c = compress(&buf, CompressionLevel::Balanced).unwrap();
            let d = decompress(&c).unwrap();
            assert_eq!(d, buf, "Failed for size {}", size);
        }
    }

    #[test]
    fn test_expansion_safeguard_forces_raw() {
        // High entropy / random data where RLE would expand
        let mut buf = Vec::with_capacity(500);
        for i in 0..500 {
            buf.push((i * 37 % 256) as u8);
        }
        let c = compress(&buf, CompressionLevel::Auto).unwrap();
        // Since it's RAW, container overhead is just header (~10B) + block header (~8B) + EOS/hash (~9B) < 40B
        assert!(c.len() <= buf.len() + 40);
        let d = decompress(&c).unwrap();
        assert_eq!(d, buf);
    }

    #[test]
    fn test_unsupported_future_block_type_error() {
        // Construct a stream that declares BlockType::TextPrefilter (5), which is unsupported until Phase 6
        let mut header = StreamHeader::default();
        header.flags.has_stream_checksum = false;
        let mut buf = Vec::new();
        header.write_to(&mut buf).unwrap();

        let rc_block = BlockHeader {
            block_type: BlockType::ReservedCustom,
            has_checksum: false,
            is_last: true,
            compressed_size: 4,
            uncompressed_size: 4,
            checksum: None,
        };
        rc_block.write_to(&mut buf).unwrap();
        buf.extend_from_slice(b"1234");

        let err = decompress(&buf).unwrap_err();
        assert_eq!(
            err,
            CodropError::UnsupportedBlockType(BlockType::ReservedCustom)
        );
    }
}
