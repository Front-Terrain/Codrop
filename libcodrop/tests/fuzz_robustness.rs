use libcodrop::error::CodropError;
use libcodrop::format::{BlockHeader, BlockType, StreamHeader};
use libcodrop::{compress, decompress, decompress_with_limit, CompressionLevel};

/// Simple LCG pseudo-random generator for deterministic fuzz testing
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (self.state >> 32) as u32
    }

    fn next_bytes(&mut self, len: usize) -> Vec<u8> {
        let mut buf = Vec::with_capacity(len);
        for _ in 0..len {
            buf.push((self.next_u32() & 0xFF) as u8);
        }
        buf
    }
}

#[test]
fn test_fuzz_roundtrip_various_sizes() {
    let mut rng = SimpleRng::new(0x1337BEEF);
    let sizes = [
        0, 1, 2, 3, 4, 7, 8, 15, 16, 31, 32, 63, 64, 127, 128, 255, 256, 512, 1024, 4096, 16384,
        65536,
    ];

    for &size in &sizes {
        let data = rng.next_bytes(size);
        for &level in &[
            CompressionLevel::Fast,
            CompressionLevel::Balanced,
            CompressionLevel::Compact,
            CompressionLevel::Auto,
        ] {
            let compressed = compress(&data, level).expect("Compression should succeed");
            let decompressed = decompress(&compressed).expect("Decompression should succeed");
            assert_eq!(
                data, decompressed,
                "Roundtrip failed for size {} at level {:?}",
                size, level
            );
        }
    }
}

#[test]
fn test_fuzz_1mb_payload_roundtrip() {
    let size = 1024 * 1024; // 1 MB
    let mut data = Vec::with_capacity(size);
    for i in 0..size {
        // Repeated runs mixed with counter values
        if i % 500 < 200 {
            data.push(0x55);
        } else {
            data.push((i % 251) as u8);
        }
    }

    let compressed =
        compress(&data, CompressionLevel::Auto).expect("1MB compression should succeed");
    let decompressed = decompress(&compressed).expect("1MB decompression should succeed");
    assert_eq!(data, decompressed);
}

#[test]
fn test_encoder_deterministic_property() {
    let mut rng = SimpleRng::new(0xCAFEF00D);
    let data = rng.next_bytes(10000);

    let run1 = compress(&data, CompressionLevel::Balanced).unwrap();
    let run2 = compress(&data, CompressionLevel::Balanced).unwrap();
    assert_eq!(
        run1, run2,
        "Encoder must be bit-for-bit deterministic for identical inputs"
    );
}

#[test]
fn test_fuzz_truncation_resilience() {
    let original = b"Hostile input resilience is a foundational requirement of the Codrop engine. Never trust input bytes!";
    let compressed = compress(original, CompressionLevel::Balanced).unwrap();

    // Truncate at every single byte boundary
    for cut in 0..compressed.len() {
        let truncated = &compressed[..cut];
        let result = decompress(truncated);
        assert!(
            result.is_err(),
            "Truncated stream at cut {}/{} must return Err, got Ok",
            cut,
            compressed.len()
        );
    }
}

#[test]
fn test_fuzz_random_noise_rejection() {
    let mut rng = SimpleRng::new(0xDEADCAFE);

    // Test 200 different random buffers; none should panic or succeed as valid Codrop streams
    for len in [4, 8, 16, 32, 64, 128, 512, 1024, 4096] {
        for _ in 0..20 {
            let noise = rng.next_bytes(len);
            let result = decompress(&noise);
            assert!(
                result.is_err(),
                "Random noise must never decode successfully as a valid stream"
            );
        }
    }
}

#[test]
fn test_fuzz_bit_corruption_detection() {
    let original = b"Codrop stream integrity is guaranteed by CRC32c block checks and XXH3 whole-stream hashes.";
    let mut compressed = compress(original, CompressionLevel::Balanced).unwrap();

    // Mutate individual bytes in the payload and confirm failure
    for i in 4..compressed.len() {
        compressed[i] ^= 0xFF;
        let result = decompress(&compressed);
        assert!(
            result.is_err(),
            "Mutated byte at index {} must fail integrity check",
            i
        );
        compressed[i] ^= 0xFF; // restore
    }
}

#[test]
fn test_fuzz_decompression_bomb_clamp() {
    let original = vec![0x41; 100_000]; // 100 KB
    let compressed = compress(&original, CompressionLevel::Auto).unwrap();

    // Limit to 50 KB max output
    let result = decompress_with_limit(&compressed, 50_000);
    assert!(
        matches!(result, Err(CodropError::DecompressionBombDetected { .. })),
        "Decompression bomb safety clamp must trigger when exceeding limit"
    );
}

#[test]
fn test_corruption_invalid_magic() {
    let bad_magic = b"ZIP1\x10\x00\x00";
    let err = decompress(bad_magic).unwrap_err();
    assert!(matches!(err, CodropError::InvalidMagic(_)));
}

#[test]
fn test_corruption_invalid_reserved_header_flags() {
    let mut header = StreamHeader::default();
    header.flags.has_stream_checksum = false;
    let mut buf = Vec::new();
    header.write_to(&mut buf).unwrap();

    // Invert reserved bit 6 in flags (bytes 5..6)
    buf[5] |= 1 << 6;
    // Recompute CRC-8 so header CRC passes, but reserved flag check fails
    let new_crc = libcodrop::checksum::Crc8::compute(&buf[4..buf.len() - 1]);
    let last = buf.len() - 1;
    buf[last] = new_crc;

    let err = decompress(&buf).unwrap_err();
    assert!(matches!(err, CodropError::CorruptedHeader(_)));
}

#[test]
fn test_corruption_invalid_reserved_block_bits() {
    let mut header = StreamHeader::default();
    header.flags.has_stream_checksum = false;
    let mut buf = Vec::new();
    header.write_to(&mut buf).unwrap();

    // Block with bits 5..7 set
    buf.push(0x80); // reserved bit 7 set with RAW type
    buf.extend_from_slice(&1u16.to_le_bytes()); // compressed size
    buf.push(1); // uncompressed size ULEB128
    buf.push(0x42); // payload

    let err = decompress(&buf).unwrap_err();
    assert!(matches!(err, CodropError::CorruptedHeader(_)));
}

#[test]
fn test_corruption_corrupted_rle_length() {
    let mut header = StreamHeader::default();
    header.flags.has_stream_checksum = false;
    let mut buf = Vec::new();
    header.write_to(&mut buf).unwrap();

    // RLE block with zero run length
    let rle_block = BlockHeader {
        block_type: BlockType::Rle,
        has_checksum: false,
        is_last: true,
        compressed_size: 2,
        uncompressed_size: 10,
        checksum: None,
    };
    rle_block.write_to(&mut buf).unwrap();
    buf.push(0x41); // byte
    buf.push(0x00); // run length = 0 (illegal in RLE)

    let err = decompress(&buf).unwrap_err();
    assert!(matches!(err, CodropError::CorruptedEntropyStream(_)));
}

#[test]
fn test_lzf_roundtrip_text_and_code() {
    let source_code = b"
        pub fn encode_literal_length(len: usize, out: &mut Vec<u8>) -> u8 {
            if len < 15 {
                len as u8
            } else {
                let mut rem = len - 15;
                while rem >= 255 {
                    out.push(255);
                    rem -= 255;
                }
                out.push(rem as u8);
                15
            }
        }
        pub fn encode_literal_length(len: usize, out: &mut Vec<u8>) -> u8 {
            if len < 15 {
                len as u8
            } else {
                let mut rem = len - 15;
                while rem >= 255 {
                    out.push(255);
                    rem -= 255;
                }
                out.push(rem as u8);
                15
            }
        }
    ";

    for &level in &[CompressionLevel::Fast, CompressionLevel::Auto] {
        let compressed = compress(source_code, level).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, source_code);
    }
}

#[test]
fn test_lzf_large_distance_match() {
    // 50 KB of random data, followed by 100 bytes pattern, then 100 bytes same pattern
    let mut rng = SimpleRng::new(0x987654321);
    let mut data = rng.next_bytes(50_000);
    let pattern = b"This is a repeated pattern across a 50KB displacement in LZF history buffer!";
    data.extend_from_slice(pattern);
    data.extend_from_slice(pattern);

    let compressed = compress(&data, CompressionLevel::Fast).unwrap();
    let decompressed = decompress(&compressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_lzf_extreme_extended_lengths() {
    // Extended literal run: 2000 unique-ish bytes
    let mut data = Vec::new();
    for i in 0..2000 {
        data.push((i % 251) as u8);
    }
    // Followed by extended match: repeat that 2000 bytes pattern
    data.extend_from_within(0..2000);

    let compressed = compress(&data, CompressionLevel::Fast).unwrap();
    let decompressed = decompress(&compressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_lzf_decoder_fuzz_never_panics() {
    use libcodrop::codec::LzfCodec;

    let mut rng = SimpleRng::new(0xFEEDFACE);
    for len in [0, 1, 2, 3, 5, 8, 16, 32, 64, 128, 512, 1024] {
        for _ in 0..50 {
            let corrupted_payload = rng.next_bytes(len);
            let expected_len = (rng.next_u32() % 2048) as usize;
            // The decoder MUST return either Ok or Err, but NEVER panic or UB
            let _ = LzfCodec::decode(&corrupted_payload, expected_len);
        }
    }
}

#[test]
fn test_lzf_malformed_token_truncation() {
    use libcodrop::codec::LzfCodec;
    // Expected 10 bytes, but compressed payload is completely empty
    let err = LzfCodec::decode(&[], 10).unwrap_err();
    assert_eq!(err, CodropError::UnexpectedEof);
}

#[test]
fn test_lzf_malformed_match_overflows_expected() {
    use libcodrop::codec::LzfCodec;
    // Token with lit=0, match=10, but expected uncompressed size is only 5
    // match nibble = 7 => match_len = 10
    let token = 0x07;
    let offset = 1u16.to_le_bytes();
    let payload = [token, offset[0], offset[1]];
    let err = LzfCodec::decode(&payload, 5).unwrap_err();
    match err {
        CodropError::InvalidMatchLength {
            length: 10,
            remaining: 5,
        } => {}
        other => panic!("Expected InvalidMatchLength, got {:?}", other),
    }
}

#[test]
fn test_lzf_malformed_offset_zero_rejected() {
    use libcodrop::codec::LzfCodec;
    let token = 0x00; // lit=0, match=3
    let offset = 0u16.to_le_bytes();
    let payload = [token, offset[0], offset[1]];
    let err = LzfCodec::decode(&payload, 3).unwrap_err();
    match err {
        CodropError::InvalidOffset {
            offset: 0,
            max_valid: 0,
        } => {}
        other => panic!("Expected InvalidOffset with 0, got {:?}", other),
    }
}

#[test]
fn test_lzf_malformed_extended_literal_truncated() {
    use libcodrop::codec::LzfCodec;
    // Token indicates lit >= 15 (high nibble = 0xF), but no extended length bytes provided
    let token = 0xF0;
    let payload = [token];
    let err = LzfCodec::decode(&payload, 20).unwrap_err();
    assert_eq!(err, CodropError::UnexpectedEof);
}

#[test]
fn test_lzh_decoder_fuzz_never_panics() {
    use libcodrop::codec::LzhCodec;

    let mut rng = SimpleRng::new(0xABCDEF01);
    for len in [0, 1, 2, 3, 5, 8, 16, 32, 64, 128, 512, 1024] {
        for _ in 0..50 {
            let corrupted_payload = rng.next_bytes(len);
            let expected_len = (rng.next_u32() % 2048) as usize;
            // The LZH decoder MUST return either Ok or Err, but NEVER panic or UB
            let _ = LzhCodec::decode(&corrupted_payload, expected_len);
        }
    }
}

#[test]
fn test_lzh_malformed_empty_payload_non_zero_expected() {
    use libcodrop::codec::LzhCodec;
    let err = LzhCodec::decode(&[], 50).unwrap_err();
    assert!(matches!(err, CodropError::UnexpectedEof));
}

#[test]
fn test_lzh_malformed_truncated_table_descriptors() {
    use libcodrop::codec::LzhCodec;
    // Preamble has 3 bytes (num_lit_len: u16, num_dist: u8), but payload is only 2 bytes
    let err = LzhCodec::decode(&[0x10, 0x00], 100).unwrap_err();
    assert!(matches!(err, CodropError::UnexpectedEof));

    // Preamble has 3 bytes, but code-length byte stream is abruptly cut
    let err = LzhCodec::decode(&[0x10, 0x00, 0x05], 100).unwrap_err();
    assert!(matches!(err, CodropError::UnexpectedEof));
}

#[test]
fn test_lzh_malformed_oversubscribed_tree_rejected() {
    use libcodrop::codec::LzhCodec;
    // Build a payload where code length descriptors specify two symbols of length 1 (sum = 2/2 = 1)
    // and then a third symbol of length 1 (sum = 3/2 = 1.5 > 1, violating Kraft's inequality)
    let num_lit_len = 3u16.to_le_bytes();
    let num_dist = 0u8;
    // Literal 0 has length 1, literal 1 has length 1, literal 2 has length 1
    let payload = [num_lit_len[0], num_lit_len[1], num_dist, 1, 1, 1];
    let err = LzhCodec::decode(&payload, 10).unwrap_err();
    assert!(matches!(err, CodropError::CorruptedEntropyStream(_)));
}

#[test]
fn test_lzh_malformed_match_exceeds_uncompressed_bounds() {
    use libcodrop::codec::lzh::{encode_length, serialize_table_descriptors};
    use libcodrop::codec::LzhCodec;
    use libcodrop::entropy::bitstream::BitWriter;
    use libcodrop::entropy::huffman::{build_canonical_codes, build_code_lengths};

    // Construct valid Huffman tables with 1 literal and 1 length symbol (length 10)
    let mut lit_len_freq = [0u32; 286];
    let mut dist_freq = [0u32; 32];
    lit_len_freq[b'A' as usize] = 1;
    let (len_sym, extra_bits, extra_val) = encode_length(10);
    lit_len_freq[len_sym] = 1;
    lit_len_freq[256] = 1; // EOB
    dist_freq[0] = 1; // distance 1

    let lit_len_lengths = build_code_lengths(&lit_len_freq, 286);
    let dist_lengths = build_code_lengths(&dist_freq, 32);
    let lit_len_codes = build_canonical_codes(&lit_len_lengths);
    let dist_codes = build_canonical_codes(&dist_lengths);

    let mut payload = Vec::new();
    serialize_table_descriptors(&lit_len_lengths, &dist_lengths, &mut payload);

    let mut bitwriter = BitWriter::new();
    // Emit 'A'
    bitwriter.write_bits(
        lit_len_codes[b'A' as usize] as u32,
        lit_len_lengths[b'A' as usize],
    );
    // Emit length symbol for 10, but expected_len will only be 5!
    bitwriter.write_bits(lit_len_codes[len_sym] as u32, lit_len_lengths[len_sym]);
    bitwriter.write_bits(extra_val, extra_bits);
    // Emit distance symbol 0 (distance 1)
    bitwriter.write_bits(dist_codes[0] as u32, dist_lengths[0]);
    bitwriter.write_bits(lit_len_codes[256] as u32, lit_len_lengths[256]);

    payload.extend_from_slice(&bitwriter.into_bytes());

    // Expected len is 5. 'A' takes 1, match length 10 exceeds remaining 4.
    let err = LzhCodec::decode(&payload, 5).unwrap_err();
    match err {
        CodropError::InvalidMatchLength {
            length: 10,
            remaining: 4,
        } => {}
        other => panic!(
            "Expected InvalidMatchLength {{ length: 10, remaining: 4 }}, got {:?}",
            other
        ),
    }
}

#[test]
fn test_lzh_roundtrip_structured_payloads() {
    let payloads: Vec<(&str, &[u8])> = vec![
        ("JSON", br#"{"users":[{"id":1,"name":"Alice","roles":["admin","editor"]},{"id":2,"name":"Bob","roles":["viewer"]}]}"#),
        ("HTML", b"<!DOCTYPE html><html><head><title>Codrop</title></head><body><h1>Codrop LZH</h1><p>Fast and safe entropy codec.</p></body></html>"),
        ("CSV", b"id,first_name,last_name,email,ip_address\n1,Jeanette,Penddreth,jpenddreth0@census.gov,26.58.193.2\n2,Geri,Marriner,gmarriner1@sfgate.com,170.21.233.155\n"),
        ("Repeated_log", b"[2026-09-24 12:00:01] INFO  codec::lzh - Processing block sequence 4096 bytes\n[2026-09-24 12:00:02] INFO  codec::lzh - Processing block sequence 4096 bytes\n"),
    ];

    for (name, data) in payloads {
        let compressed = compress(data, CompressionLevel::Balanced)
            .unwrap_or_else(|_| panic!("Compression failed for {}", name));
        let decompressed =
            decompress(&compressed).unwrap_or_else(|_| panic!("Decompression failed for {}", name));
        assert_eq!(decompressed, data, "Roundtrip failed for {}", name);
    }
}

#[test]
fn test_lza_decoder_fuzz_never_panics() {
    use libcodrop::codec::LzaCodec;

    let mut rng = SimpleRng::new(0x12345678);
    for len in [0, 1, 2, 3, 5, 8, 16, 32, 64, 128, 512, 1024] {
        for _ in 0..50 {
            let corrupted_payload = rng.next_bytes(len);
            let expected_len = (rng.next_u32() % 2048) as usize;
            // The LZA decoder MUST return either Ok or Err, but NEVER panic or UB
            let _ = LzaCodec::decode(&corrupted_payload, expected_len);
        }
    }
}

#[test]
fn test_lza_malformed_empty_payload_non_zero_expected() {
    use libcodrop::codec::LzaCodec;
    let err = LzaCodec::decode(&[], 50).unwrap_err();
    assert!(matches!(err, CodropError::UnexpectedEof));
}

#[test]
fn test_lza_malformed_truncated_descriptors() {
    use libcodrop::codec::LzaCodec;
    // Preamble has 2 bytes (max_symbol), but payload is only 1 byte
    let err = LzaCodec::decode(&[0x10], 100).unwrap_err();
    assert!(matches!(err, CodropError::UnexpectedEof));

    // Preamble specifies max_symbol, but count bytes are abruptly cut
    let err = LzaCodec::decode(&[0x10, 0x00], 100).unwrap_err();
    assert!(matches!(err, CodropError::UnexpectedEof));
}

#[test]
fn test_lza_roundtrip_structured_payloads_compact() {
    let payloads: Vec<(&str, &[u8])> = vec![
        ("JSON_compact", br#"{"system":"codrop","version":1.0,"features":["RAW","RLE","LZF","LZH","LZA"],"active":true,"compression_ratio":0.125}"#),
        ("HTML_compact", b"<!DOCTYPE html><html><head><title>Codrop LZA</title></head><body><h1>Asymmetric Numeral Systems</h1><p>Compact and deterministic entropy coding.</p></body></html>"),
        ("CSV_compact", b"id,name,role,department,salary\n1,Alice,Engineer,Core,120000\n2,Bob,Researcher,Algorithms,135000\n3,Charlie,Architect,Systems,150000\n"),
        ("Log_compact", b"[2026-09-24 20:00:00] INFO  codrop::lza - Initializing ANS state machine L=1024\n[2026-09-24 20:00:01] INFO  codrop::lza - Initializing ANS state machine L=1024\n"),
    ];

    for (name, data) in payloads {
        let compressed = compress(data, CompressionLevel::Compact)
            .unwrap_or_else(|_| panic!("Compression failed for {}", name));
        let decompressed =
            decompress(&compressed).unwrap_or_else(|_| panic!("Decompression failed for {}", name));
        assert_eq!(decompressed, data, "Compact roundtrip failed for {}", name);
    }
}

#[test]
fn test_prefilter_decoder_fuzz_never_panics() {
    use libcodrop::prefilter::decode_prefiltered_payload;

    let mut rng = SimpleRng::new(0x98765432);
    for len in [0, 1, 2, 3, 5, 8, 12, 16, 32, 64, 128, 512, 1024] {
        for _ in 0..50 {
            let corrupted_payload = rng.next_bytes(len);
            let uncompressed_len = (rng.next_u32() % 2048) as usize;
            let max_output_size = 4096;
            // The prefilter decoder MUST return either Ok or Err, but NEVER panic or UB
            let _ =
                decode_prefiltered_payload(&corrupted_payload, uncompressed_len, max_output_size);
        }
    }
}

#[test]
fn test_delta_decoder_fuzz_never_panics() {
    use libcodrop::prefilter::delta::delta_decode;

    let mut rng = SimpleRng::new(0x43218765);
    for len in [0, 1, 2, 5, 10, 64, 256] {
        for _ in 0..30 {
            let src = rng.next_bytes(len);
            let stride = (rng.next_u32() % 16) as u8;
            let _ = delta_decode(&src, stride, 1024);
        }
    }
}

#[test]
fn test_dict_invert_fuzz_never_panics() {
    use libcodrop::prefilter::dict::{invert_tokens, lookup_static_dict, DICT_ID_JSON};

    let dict = lookup_static_dict(DICT_ID_JSON, 1).unwrap();
    let mut rng = SimpleRng::new(0x55667788);
    for len in [0, 1, 2, 5, 16, 64, 256] {
        for _ in 0..30 {
            let src = rng.next_bytes(len);
            let esc = (rng.next_u32() & 0xFF) as u8;
            let _ = invert_tokens(&src, dict, esc, 1024);
        }
    }
}
