use libcodrop::{compress, decompress, decompress_with_limit, CompressionLevel};

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
fn test_property_exact_reversibility_across_all_profiles() {
    let mut rng = SimpleRng::new(0xCAFE_BABE_0001);
    let profiles = [
        CompressionLevel::Fast,
        CompressionLevel::Balanced,
        CompressionLevel::Compact,
        CompressionLevel::Auto,
    ];

    for len in [0, 1, 2, 7, 16, 64, 255, 512, 1024, 4096, 16384, 65536] {
        let original = rng.next_bytes(len);
        for &profile in &profiles {
            let compressed = compress(&original, profile).expect("compression succeeds");
            let restored = decompress(&compressed).expect("decompression succeeds");
            assert_eq!(
                restored, original,
                "failed for len {len}, profile {:?}",
                profile
            );
        }
    }
}

#[test]
fn test_property_non_expansion_guarantee() {
    let mut rng = SimpleRng::new(0xDEAD_BEEF_0002);
    // On high-entropy random data where LZ / entropy coders expand, Codrop MUST select RAW fallback
    for len in [128, 512, 1024, 4096, 16384, 65536] {
        let random_data = rng.next_bytes(len);
        let compressed =
            compress(&random_data, CompressionLevel::Auto).expect("compression succeeds");
        // Container overhead (Stream header + block header + checksums) is strictly bounded (< 128 bytes)
        assert!(
            compressed.len() <= random_data.len() + 128,
            "Expansion detected for len {len}: compressed {} vs uncompressed {}",
            compressed.len(),
            random_data.len()
        );
        let restored = decompress(&compressed).expect("decompression succeeds");
        assert_eq!(restored, random_data);
    }
}

#[test]
fn test_property_encoder_determinism() {
    let mut rng = SimpleRng::new(0xFEED_FACE_0003);
    for len in [10, 100, 1000, 10000] {
        let data = rng.next_bytes(len);
        for &profile in &[
            CompressionLevel::Fast,
            CompressionLevel::Balanced,
            CompressionLevel::Compact,
        ] {
            let run1 = compress(&data, profile).unwrap();
            let run2 = compress(&data, profile).unwrap();
            assert_eq!(
                run1, run2,
                "Determinism violated for len {len}, profile {:?}",
                profile
            );
        }
    }
}

#[test]
fn test_property_decoder_panic_freedom_on_arbitrary_fuzzed_data() {
    let mut rng = SimpleRng::new(0x1234_5678_9ABC);
    for len in [0, 1, 2, 4, 8, 16, 32, 64, 128, 512, 1024, 4096] {
        for _ in 0..20 {
            let junk = rng.next_bytes(len);
            // Decompress must return Ok or Err, but NEVER panic, loop infinitely, or abort
            let _ = decompress(&junk);
            let _ = decompress_with_limit(&junk, 1024);
        }
    }
}
