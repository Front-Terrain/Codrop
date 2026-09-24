use std::io::Cursor;

use libcodrop::error::CodropError;
use libcodrop::format::{BlockHeader, BlockType, StreamHeader};
use libcodrop::streaming::{Decoder, DecoderOptions};
use libcodrop::{compress, decompress, CompressionLevel};

/// Helper to decode a .cdp stream and return the decoded bytes along with inspected block headers.
fn decode_and_inspect(cdp_data: &[u8]) -> (Vec<u8>, Vec<BlockHeader>) {
    let options = DecoderOptions {
        max_output_bytes: Some(10 * 1024 * 1024),
        verify_block_checksums: true,
        verify_stream_checksum: true,
    };
    let cursor = Cursor::new(cdp_data);
    let mut decoder = Decoder::new(cursor, options).expect("valid stream header");
    let mut headers = Vec::new();
    let mut output = Vec::new();

    // Read blocks one by one
    let mut reader = Cursor::new(cdp_data);
    let _stream_header = StreamHeader::read_from(&mut reader).expect("stream header");
    loop {
        let block_header = BlockHeader::read_from(&mut reader).expect("block header");
        if block_header.block_type == BlockType::EndOfStream {
            headers.push(block_header);
            break;
        }
        let pos = reader.position() as usize;
        reader.set_position((pos + block_header.compressed_size as usize) as u64);
        headers.push(block_header);
    }

    decoder
        .decompress_to(&mut output)
        .expect("decompression succeeds");
    (output, headers)
}

#[test]
fn test_golden_vector_raw() {
    // 256 distinct bytes with no repetitions cannot be compressed by LZ/RLE/Huffman -> chooses RAW
    let raw_input: Vec<u8> = (0..=255).collect();
    let cdp = compress(&raw_input, CompressionLevel::Fast).expect("encode succeeds");
    let (decoded, headers) = decode_and_inspect(&cdp);

    assert_eq!(decoded, raw_input);
    assert_eq!(headers[0].block_type, BlockType::Raw);
    assert!(headers[0].has_checksum);
}

#[test]
fn test_golden_vector_rle() {
    // 50,000 repeating bytes -> encoder chooses RLE
    let rle_input = vec![0x42u8; 50_000];
    let cdp = compress(&rle_input, CompressionLevel::Fast).expect("encode succeeds");
    let (decoded, headers) = decode_and_inspect(&cdp);

    assert_eq!(decoded, rle_input);
    assert_eq!(headers[0].block_type, BlockType::Rle);
    assert!(headers[0].compressed_size < 10);
}

#[test]
fn test_golden_vector_lzf() {
    // Text phrases under Fast profile -> encoder chooses LZF
    let phrase =
        b"Codrop LZF Fast Compression Engine. Fast byte-aligned matching without entropy tables. ";
    let mut lzf_input = Vec::new();
    for _ in 0..100 {
        lzf_input.extend_from_slice(phrase);
    }

    let cdp = compress(&lzf_input, CompressionLevel::Fast).expect("encode succeeds");
    let (decoded, headers) = decode_and_inspect(&cdp);

    assert_eq!(decoded, lzf_input);
    assert_eq!(headers[0].block_type, BlockType::Lzf);
}

#[test]
fn test_golden_vector_lzh() {
    // Text data with entropy variance under Balanced profile -> encoder chooses LZH
    let text = b"The quick brown fox jumps over the lazy dog. Canonical Huffman entropy coding with Kraft inequality bounded depths.";
    let mut lzh_input = Vec::new();
    for _ in 0..80 {
        lzh_input.extend_from_slice(text);
    }

    let cdp = compress(&lzh_input, CompressionLevel::Balanced).expect("encode succeeds");
    let (decoded, headers) = decode_and_inspect(&cdp);

    assert_eq!(decoded, lzh_input);
    // Block type may be LZH or TextPrefilter depending on which is smaller
    assert!(
        headers[0].block_type == BlockType::Lzh
            || headers[0].block_type == BlockType::TextPrefilter
    );
}

#[test]
fn test_golden_vector_lza() {
    // Structured data under Compact profile -> evaluates LZA (tANS)
    let text = b"Compact Asymmetric Numeral Systems (tANS) FSE-style entropy coding engine.";
    let mut lza_input = Vec::new();
    for _ in 0..60 {
        lza_input.extend_from_slice(text);
    }

    let cdp = compress(&lza_input, CompressionLevel::Compact).expect("encode succeeds");
    let (decoded, headers) = decode_and_inspect(&cdp);

    assert_eq!(decoded, lza_input);
    assert!(headers[0].compressed_size < lza_input.len() as u32);
}

#[test]
fn test_golden_vector_json_prefilter() {
    // Repeated JSON objects and keys
    let json_doc = br#"[
  {"transaction_id": "tx_1001", "account_type": "savings", "balance_usd": 15000.50, "is_verified": true},
  {"transaction_id": "tx_1002", "account_type": "checking", "balance_usd": 4200.75, "is_verified": true},
  {"transaction_id": "tx_1003", "account_type": "investment", "balance_usd": 89000.00, "is_verified": false},
  {"transaction_id": "tx_1004", "account_type": "checking", "balance_usd": 120.00, "is_verified": true}
]"#;

    let cdp = compress(json_doc, CompressionLevel::Compact).expect("encode succeeds");
    let (decoded, _) = decode_and_inspect(&cdp);
    assert_eq!(decoded, json_doc);
}

#[test]
fn test_golden_vector_delta_prefilter() {
    // Monotonic sawtooth ramp
    let mut ramp = Vec::with_capacity(4096);
    for i in 0..4096 {
        ramp.push((i % 256) as u8);
    }

    let cdp = compress(&ramp, CompressionLevel::Compact).expect("encode succeeds");
    let (decoded, _) = decode_and_inspect(&cdp);
    assert_eq!(decoded, ramp);
}

#[test]
fn test_golden_vector_multi_block_streaming() {
    // Generate data larger than 4 x DEFAULT_BLOCK_SIZE (128 KB) to force multiple blocks
    let phrase =
        b"Streaming block test across multiple 128KB container chunks with independent checksums. ";
    let target_size = 512 * 1024 + 1024; // > 4 blocks of 128 KB
    let mut stream_data = Vec::with_capacity(target_size);
    while stream_data.len() + phrase.len() <= target_size {
        stream_data.extend_from_slice(phrase);
    }

    let cdp = compress(&stream_data, CompressionLevel::Balanced).expect("encode succeeds");
    let (decoded, headers) = decode_and_inspect(&cdp);

    assert_eq!(decoded, stream_data);
    // Must have at least 4 blocks plus EndOfStream
    assert!(headers.len() >= 5);
    assert_eq!(headers.last().unwrap().block_type, BlockType::EndOfStream);
}

#[test]
fn test_golden_vector_checksum_tamper_detection() {
    let input = b"Tamper detection verification vector for CRC32c integrity verification.";
    let mut cdp = compress(input, CompressionLevel::Balanced).expect("encode succeeds");

    // Tamper with payload byte in the block
    let last_payload_idx = cdp.len() - 10;
    cdp[last_payload_idx] ^= 0x55;

    let err = decompress(&cdp).unwrap_err();
    match err {
        CodropError::BlockChecksumMismatch { .. }
        | CodropError::StreamChecksumMismatch { .. }
        | CodropError::CorruptedEntropyStream(_)
        | CodropError::CorruptedPrefilterData(_) => {}
        other => panic!("expected checksum/stream corruption error, got {:?}", other),
    }
}
