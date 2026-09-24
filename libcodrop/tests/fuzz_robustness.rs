use libcodrop::error::CodropError;
use libcodrop::{CompressionLevel, compress, decompress, decompress_with_limit};

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
    let sizes = [0, 1, 2, 3, 4, 7, 8, 15, 16, 31, 32, 63, 64, 127, 128, 255, 256, 512, 1024, 4096, 16384, 65536];

    for &size in &sizes {
        let data = rng.next_bytes(size);
        for &level in &[CompressionLevel::Fast, CompressionLevel::Balanced, CompressionLevel::Auto] {
            let compressed = compress(&data, level).expect("Compression should succeed");
            let decompressed = decompress(&compressed).expect("Decompression should succeed");
            assert_eq!(data, decompressed, "Roundtrip failed for size {} at level {:?}", size, level);
        }
    }
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
