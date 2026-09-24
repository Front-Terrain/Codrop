use libcodrop::codec::{LzaCodec, LzfCodec, LzhCodec, RawCodec, RleCodec};
use libcodrop::prefilter::{try_encode_prefilter, PrefilterBackend};
use libcodrop::{compress, decompress, CompressionLevel};
use std::time::Instant;

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

fn bench_dataset(name: &str, data: &[u8]) {
    let uncompressed_len = data.len();

    // 1. RAW
    let start_raw_enc = Instant::now();
    let raw_compressed = RawCodec::encode(data);
    let raw_enc_time = start_raw_enc.elapsed();

    let start_raw_dec = Instant::now();
    let raw_decompressed = RawCodec::decode(&raw_compressed, uncompressed_len).unwrap();
    let raw_dec_time = start_raw_dec.elapsed();
    assert_eq!(raw_decompressed.len(), uncompressed_len);

    // 2. RLE
    let start_rle_enc = Instant::now();
    let rle_compressed = RleCodec::encode(data);
    let rle_enc_time = start_rle_enc.elapsed();

    let start_rle_dec = Instant::now();
    let rle_decompressed = RleCodec::decode(&rle_compressed, uncompressed_len).unwrap();
    let rle_dec_time = start_rle_dec.elapsed();
    assert_eq!(rle_decompressed.len(), uncompressed_len);

    // 3. LZF
    let _ = LzfCodec::encode(data);
    let lzf_sample = LzfCodec::encode(data);
    let _ = LzfCodec::decode(&lzf_sample, uncompressed_len).unwrap();

    let iters = 10;
    let start_lzf_enc = Instant::now();
    for _ in 0..iters {
        let _ = LzfCodec::encode(data);
    }
    let lzf_enc_time = start_lzf_enc.elapsed() / iters;

    let start_lzf_dec = Instant::now();
    for _ in 0..iters {
        let _ = LzfCodec::decode(&lzf_sample, uncompressed_len).unwrap();
    }
    let lzf_dec_time = start_lzf_dec.elapsed() / iters;

    // 4. LZH (LZ matching + Canonical Huffman)
    let _ = LzhCodec::encode(data);
    let lzh_sample = LzhCodec::encode(data);
    let lzh_decompressed = LzhCodec::decode(&lzh_sample, uncompressed_len).unwrap();
    assert_eq!(lzh_decompressed.len(), uncompressed_len);

    let lzh_iters = 5;
    let start_lzh_enc = Instant::now();
    for _ in 0..lzh_iters {
        let _ = LzhCodec::encode(data);
    }
    let lzh_enc_time = start_lzh_enc.elapsed() / lzh_iters;

    let start_lzh_dec = Instant::now();
    for _ in 0..lzh_iters {
        let _ = LzhCodec::decode(&lzh_sample, uncompressed_len).unwrap();
    }
    let lzh_dec_time = start_lzh_dec.elapsed() / lzh_iters;

    // 5. LZA (LZ matching + tANS / FSE)
    let _ = LzaCodec::encode(data);
    let lza_sample = LzaCodec::encode(data);
    let lza_decompressed = LzaCodec::decode(&lza_sample, uncompressed_len).unwrap();
    assert_eq!(lza_decompressed.len(), uncompressed_len);

    let lza_iters = 5;
    let start_lza_enc = Instant::now();
    for _ in 0..lza_iters {
        let _ = LzaCodec::encode(data);
    }
    let lza_enc_time = start_lza_enc.elapsed() / lza_iters;

    let start_lza_dec = Instant::now();
    for _ in 0..lza_iters {
        let _ = LzaCodec::decode(&lza_sample, uncompressed_len).unwrap();
    }
    let lza_dec_time = start_lza_dec.elapsed() / lza_iters;

    // 6. Prefilter + LZH/LZA evaluation
    let prefilter_lzh_cand = try_encode_prefilter(data, PrefilterBackend::Lzh, 5);
    let prefilter_lza_cand = try_encode_prefilter(data, PrefilterBackend::Lza, 9);

    // 7. Full CDP container profiles (Fast, Balanced, Compact)
    let cdp_fast = compress(data, CompressionLevel::Fast).unwrap();
    let cdp_balanced = compress(data, CompressionLevel::Balanced).unwrap();
    let cdp_compact = compress(data, CompressionLevel::Compact).unwrap();
    let decompressed_compact = decompress(&cdp_compact).unwrap();
    assert_eq!(decompressed_compact, data);

    let mb = (uncompressed_len as f64) / 1_000_000.0;
    let lzf_enc_mbs = mb / lzf_enc_time.as_secs_f64().max(0.0000001);
    let lzf_dec_mbs = mb / lzf_dec_time.as_secs_f64().max(0.0000001);
    let lzh_enc_mbs = mb / lzh_enc_time.as_secs_f64().max(0.0000001);
    let lzh_dec_mbs = mb / lzh_dec_time.as_secs_f64().max(0.0000001);
    let lza_enc_mbs = mb / lza_enc_time.as_secs_f64().max(0.0000001);
    let lza_dec_mbs = mb / lza_dec_time.as_secs_f64().max(0.0000001);

    let lzf_ratio = (lzf_sample.len() as f64) / (uncompressed_len as f64);
    let lzh_ratio = (lzh_sample.len() as f64) / (uncompressed_len as f64);
    let lza_ratio = (lza_sample.len() as f64) / (uncompressed_len as f64);
    let rle_ratio = (rle_compressed.len() as f64) / (uncompressed_len as f64);

    println!("--------------------------------------------------");
    println!("Benchmark Dataset: {} ({} bytes)", name, uncompressed_len);
    println!("  RAW: Size = {} bytes", raw_compressed.len());
    println!(
        "       Enc: {:.3} ms | Dec: {:.3} ms",
        raw_enc_time.as_secs_f64() * 1000.0,
        raw_dec_time.as_secs_f64() * 1000.0
    );
    println!(
        "  RLE: Size = {} bytes (Ratio: {:.4})",
        rle_compressed.len(),
        rle_ratio
    );
    println!(
        "       Enc: {:.3} ms | Dec: {:.3} ms",
        rle_enc_time.as_secs_f64() * 1000.0,
        rle_dec_time.as_secs_f64() * 1000.0
    );
    println!(
        "  LZF: Size = {} bytes (Ratio: {:.4}, {:.2}% savings)",
        lzf_sample.len(),
        lzf_ratio,
        (1.0 - lzf_ratio) * 100.0
    );
    println!(
        "       Compression:   {:.3} ms ({:.2} MB/s)",
        lzf_enc_time.as_secs_f64() * 1000.0,
        lzf_enc_mbs
    );
    println!(
        "       Decompression: {:.3} ms ({:.2} MB/s)",
        lzf_dec_time.as_secs_f64() * 1000.0,
        lzf_dec_mbs
    );
    println!(
        "  LZH: Size = {} bytes (Ratio: {:.4}, {:.2}% savings)",
        lzh_sample.len(),
        lzh_ratio,
        (1.0 - lzh_ratio) * 100.0
    );
    println!(
        "       Compression:   {:.3} ms ({:.2} MB/s)",
        lzh_enc_time.as_secs_f64() * 1000.0,
        lzh_enc_mbs
    );
    println!(
        "       Decompression: {:.3} ms ({:.2} MB/s)",
        lzh_dec_time.as_secs_f64() * 1000.0,
        lzh_dec_mbs
    );
    println!(
        "  LZA: Size = {} bytes (Ratio: {:.4}, {:.2}% savings)",
        lza_sample.len(),
        lza_ratio,
        (1.0 - lza_ratio) * 100.0
    );
    println!(
        "       Compression:   {:.3} ms ({:.2} MB/s)",
        lza_enc_time.as_secs_f64() * 1000.0,
        lza_enc_mbs
    );
    println!(
        "       Decompression: {:.3} ms ({:.2} MB/s)",
        lza_dec_time.as_secs_f64() * 1000.0,
        lza_dec_mbs
    );

    if let Some(cand) = prefilter_lzh_cand {
        println!(
            "  Prefilter + LZH: Size = {} bytes (Prefilter: {:?})",
            cand.encoded_payload.len(),
            cand.prefilter_type
        );
    }
    if let Some(cand) = prefilter_lza_cand {
        println!(
            "  Prefilter + LZA: Size = {} bytes (Prefilter: {:?})",
            cand.encoded_payload.len(),
            cand.prefilter_type
        );
    }

    println!(
        "  CDP Container Sizes -> Fast: {} bytes | Balanced: {} bytes | Compact: {} bytes",
        cdp_fast.len(),
        cdp_balanced.len(),
        cdp_compact.len()
    );
}

#[test]
fn test_benchmark_suite() {
    let size = 1024 * 1024; // 1 MB

    // 1. Repeated data
    let repeated = vec![0x41u8; size];
    bench_dataset("Repeated Bytes (1MB)", &repeated);

    // 2. Periodic text
    let mut periodic = Vec::with_capacity(size);
    let phrase = b"The quick brown fox jumps over the lazy dog. Codrop fast compression! ";
    while periodic.len() + phrase.len() <= size {
        periodic.extend_from_slice(phrase);
    }
    bench_dataset("Periodic Phrases (1MB)", &periodic);

    // 3. Text-like code / structured JSON
    let mut structured = Vec::with_capacity(size);
    let json_chunk = br#"{"id":1001,"status":"active","name":"compression_engine","metrics":{"entropy":3.14,"ratio":0.42}},"#;
    while structured.len() + json_chunk.len() <= size {
        structured.extend_from_slice(json_chunk);
    }
    bench_dataset("Structured JSON (1MB)", &structured);

    // 4. Random noise (high entropy)
    let mut rng = SimpleRng::new(0xDEADBEEF);
    let random_data = rng.next_bytes(size);
    bench_dataset("Random High-Entropy (1MB)", &random_data);
}
