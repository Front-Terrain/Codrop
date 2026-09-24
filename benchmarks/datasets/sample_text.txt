# Codrop Binary Format & Compression Specification (v0.1.0-draft)

**Status:** Draft  
**Target Format Version:** 1 (`0x01`)  
**MIME Type:** `application/x-codrop`  
**File Extension:** `.cdp`  
**Standard Endianness:** Little-Endian (LE)

---

## 1. Codrop Goals & Vision

Codrop is designed as a modern, portable, general-purpose compression ecosystem delivering a single, unified format across cloud servers, browsers (WebAssembly), desktops, mobile runtimes, and embedded edge systems.

### Primary Objectives
1. **Unified Multi-Platform Implementation:** Decouple algorithmic evolution from file format stability. A single core engine written in pure, memory-safe Rust with zero mandatory runtime dependencies, compiles to native targets and WebAssembly without behavioral divergence.
2. **Stable Format, Evolving Encoder, Portable Decoder:** The `.cdp` container format specifies an immutable virtual decompression model. Encoders are free to employ heuristic match finders, optimal parsers, and custom cost models without altering decoder compatibility.
3. **Adaptive Multi-Strategy Pipeline:** Rather than forcing one-size-fits-all algorithms (e.g., standard Deflate or Brotli), Codrop dynamically classifies incoming blocks into distinct structural profiles (e.g., pure raw, run-length dominant, high-entropy binary, text/structured JSON/code, repetitive tabular) and applies optimal matching and entropy primitives per block.
4. **Streaming & Low-Memory Operational Modes:** Native single-pass chunked streaming with constant working memory (down to 64 KB history buffers for memory-constrained microcontrollers and WASM workers) alongside high-ratio multi-megabyte window modes for archive and server-to-server synchronization.
5. **Robust Security & Hostile-Input Resilience:** Explicit bounding on maximum allocation, reference offsets, decompression ratio clamps, and multi-tier integrity checks (checksumming per block and per stream).

---

## 2. Non-Goals

1. **Lossy Compression:** Codrop is exclusively lossless. No perceptual discarding of audio, image, or video high frequencies is supported within the container.
2. **Built-in Cryptographic Encryption:** Encryption belongs in transport (TLS) or authenticated container layers (AEAD / age / Noise). Codrop does not incorporate proprietary or unvetted ciphers inside the framing format.
3. **Universal Superiority Claims:** Codrop does not promise to beat specialized domain compressors (e.g., FLAC for audio, QOI for uncompressed imagery, or zstd-dict trained on thousands of identical schema files) without an equivalent pre-shared dictionary.
4. **Kitchen-Sink Archive Container:** Codrop compresses byte streams. It is not an archive format containing file trees, POSIX permissions, or extended attributes (which remain the role of `tar`, `cpio`, or dedicated archive layers).

---

## 3. High-Level Architecture

The Codrop system is layered into distinct conceptual boundaries:

```
┌────────────────────────────────────────────────────────────────────────┐
│                              Public API                                │
│           codrop::compress()  /  codrop::decompress()                  │
│           codrop::Encoder     /  codrop::Decoder (Stream)              │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
┌───────────────────────────────────▼────────────────────────────────────┐
│                    Adaptive Classifier & Scheduler                     │
│   Sampling / Entropy Calculation / Run Analysis / Match-Density Est.   │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
┌───────────────────────────────────▼────────────────────────────────────┐
│                       Data Transformation Layer                        │
│          Identity | Run-Length Pre-filter | Word/Byte Filter           │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
┌───────────────────────────────────▼────────────────────────────────────┐
│                         Match-Finding Engine                           │
│  Fast Hash-Chain (Level 1-2) | Double-Hash (Level 3-4) | Optimal/DP    │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
┌───────────────────────────────────▼────────────────────────────────────┐
│                         Entropy Coding Layer                           │
│     Raw Literals | Canonical Huffman | Finite State Entropy (tANS)    │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
┌───────────────────────────────────▼────────────────────────────────────┐
│                       Framing & Container Layer                        │
│      Magic / Headers / Dynamic Block Packaging / Checksums (CRC/xxH)   │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 4. The `.cdp` Container Format Specification

A Codrop stream consists of a **Magic Identifier**, a **Stream Header**, zero or more **Data Blocks**, and an optional **Stream Footer (Terminator & Verification)**.

```
+---------------+---------------+--------------------+---------------+---------------+
| Magic (4B)    | Header (Var)  | Block 0            | Block 1 ...   | Footer (Opt)  |
| 0x43 0x44 ... | Version/Flags | Type|Size|Data|Chk | Type|Size...  | Term|StreamChk|
+---------------+---------------+--------------------+---------------+---------------+
```

All numerical fields in multi-byte integers are serialized in **Little-Endian** format unless explicitly stated.

---

## 5. Header Specification

### 5.1 Magic Bytes
The stream begins with 4 constant identification bytes:
```text
Offset 0x00: 0x43 ('C')
Offset 0x01: 0x44 ('D')
Offset 0x02: 0x50 ('P')
Offset 0x03: 0x31 ('1')  => ASCII representation "CDP1"
```
*Note for single-byte packet modes (Small Payload Profile):* When operating in zero-framing datagram mode (e.g. embedded IPC), an alternate micro-magic `0xCD 0x01` (2 bytes) can be selected via configuration flags, but standard interchange files MUST use `0x43 0x44 0x50 0x31`.

### 5.2 Header Layout

```text
Byte 0..3:   Magic (0x31504443)
Byte 4:      Format Version (Major: 4 bits, Minor: 4 bits). Format 1.0 = 0x10.
Byte 5..6:   Header Flags (16-bit Bitfield, Little-Endian)
             - Bit 0: Has Uncompressed Size (0 = Unknown/Streaming, 1 = Known)
             - Bit 1: Has Stream Checksum (XXH3_64 / CRC32c)
             - Bit 2: Has Dictionary ID
             - Bit 3: Independent Blocks (Self-contained decoding per block)
             - Bit 4..5: Window Size Exponent (00 = 64KB, 01 = 1MB, 10 = 8MB, 11 = Custom)
             - Bit 6..15: Reserved (Must be 0 in v1.0, decoders must reject if unknown critical bits set)
Byte 7:      Window Descriptor (Present if Custom Window Size selected; exponent 16..27 => 64KB..128MB)
Byte 8..N:   Optional Uncompressed Size (Present if Flag Bit 0 == 1; ULEB128 encoded)
Byte N..M:   Optional Dictionary Identifier (Present if Flag Bit 2 == 1; 4-byte LE ID)
Byte M..K:   Header Checksum (CRC-8 of Header bytes 4 through M, polynomial 0x07)
```

### 5.3 Window Descriptor
To bound memory consumption during decompression:
- The decompressor MUST allocate or reserve a ring buffer equal to the negotiated window size.
- Window sizes range from $2^{16}$ (64 KB) to $2^{27}$ (128 MB). Default for standard files: $2^{21}$ (2 MB).
- Fast embedded profiles cap window size at 64 KB or 256 KB.

---

## 6. Block Specification

A Codrop stream is partitioned into one or more sequential blocks. This structure facilitates streaming, parallel decompression, memory limiting, and adaptive selection.

### 6.1 Block Header
Each block begins with a 3-byte to 5-byte Block Header:

```text
Byte 0:      Block Metadata
             - Bits 0..2: Block Type
                 000 (0): RAW / STORED (No compression)
                 001 (1): RLE (Run-Length Encoded)
                 010 (2): LZF (Codrop Fast LZ - Byte-aligned matches, raw literals)
                 011 (3): LZH (Codrop LZ + Canonical Huffman entropy coding)
                 100 (4): LZA (Codrop LZ + Finite State Entropy / tANS)
                 101 (5): TEXT_PREFILTER (Transform + LZH/LZA)
                 110 (6): RESERVED_CUSTOM
                 111 (7): END_OF_STREAM (Terminator block)
             - Bit 3: Block Checksum Present (1 = 4-byte CRC32c appended to block)
             - Bit 4: Last Block Flag (1 = Stream terminates after this block)
             - Bits 5..7: Reserved (0)
Byte 1..2:   Compressed Size (16-bit LE, or variable if bit extended)
             If Compressed Size == 0xFFFF, followed by 4-byte LE extended size (up to 4 GB).
[Optional]:  Uncompressed Size (Present for Raw and RLE blocks, or if independent block is set).
```

### 6.2 Block Types and Payloads

#### Type 0: RAW / STORED
- Used when compression fails to shrink input or data is completely incompressible (random bytes, pre-encrypted payloads).
- Data follows the block header verbatim for `Compressed Size` bytes.
- Guarantees zero negative compression overhead beyond header framing.

#### Type 1: RLE (Run-Length Encoded)
- Optimized for sparse buffers, null padding, memory dumps, and raster bitmaps.
- Encodes runs of identical bytes: `[Byte Value] [Run Length: ULEB128]`.

#### Type 2: LZF (Codrop Fast LZ)
- Designed for maximum throughput (matching Snappy / LZ4 speeds).
- Byte-oriented token stream:
  - Token byte: `[Literal Length (4 bits) | Match Length - 3 (4 bits)]`
  - Extended literal/match lengths follow as necessary.
  - Offset is encoded as a 16-bit or 24-bit Little-Endian relative displacement.
  - Literals are stored raw without entropy coding.

#### Type 3: LZH (Codrop LZ + Canonical Huffman)
- Balanced compression profile.
- Token stream decomposes sequences into:
  1. Literal bytes.
  2. Match lengths (3 to 65,535 bytes).
  3. Match offsets (1 to Window Size).
- Literals and Match symbols are compressed using **Canonical Huffman Coding** with table descriptors transmitted in the block preamble.

#### Type 4: LZA (Codrop LZ + tANS / Finite State Entropy)
- High-compression compact profile.
- Employs asymmetric numeral systems (ANS) / table-based ANS (tANS) for state-of-the-art entropy compression with fractional bit precision.
- Normalizes symbol probabilities to a power-of-two state table size ($2^{10}$ or $2^{11}$).

#### Type 5: TEXT_PREFILTER
- Applies an invertible byte-level transform before LZ matching:
  - Common English and code token dictionary indexing (transforms frequent keywords like `function`, `return`, `{"`, `true`, `false`, `null`, `class` into reserved single-byte tokens $0x80..0xFF$).
  - Delta encoding for numerical sequence tables (CSV columns, timestamp lists).
- Following prefiltering, block is compressed using LZH or LZA.

---

## 7. Versioning Policy

Codrop uses a strict two-component format versioning strategy:

```
Format Version = Major (4 bits) . Minor (4 bits)
```

1. **Major Version (`0x1`):** Represents backward-incompatible structural breaks (e.g. alterations to magic bytes, block framing structure, or fundamental arithmetic encoding models).
2. **Minor Version (`0x0`):** Represents backward-compatible additions (e.g. new optional block flags, non-critical metadata payloads). Decoders encounter unknown minor versions with standard flags without failing, skipping unrecognized non-critical extensions.
3. **Codec Profile Level:** Recorded in the stream metadata to denote minimum decoder capability required to parse optional blocks.

---

## 8. Streaming Design

Streaming operates with strict time and space complexity bounds:

### 8.1 Chunked Consumption
```rust
pub trait StreamCompressor {
    fn write(&mut self, chunk: &[u8]) -> Result<usize, CodropError>;
    fn flush(&mut self) -> Result<Vec<u8>, CodropError>;
    fn finish(&mut self) -> Result<Vec<u8>, CodropError>;
}
```
- The encoder maintains an internal sliding buffer equal to the selected Window Size (e.g., 2 MB).
- Whenever the accumulation reaches a configured `Block Size` (default: 128 KB or 256 KB), an adaptive analysis step runs, the block is emitted into the destination stream, and the history buffer shifts forward.

### 8.2 Decompression Streaming
- The decoder consumes input chunks incrementally.
- Block headers identify boundaries cleanly. The decoder emits decompressed output as soon as a block or block sub-sequence is decoded into the ring buffer, enabling constant-memory pipe workflows (`cat data.cdp | codrop decompress | grep pattern`).

---

## 9. Integrity & Error Detection Mechanisms

Codrop incorporates multi-tiered validation to combat bit rot, transmission truncation, and hostile corruption:

1. **Header CRC-8 (Polynomial 0x07):** Prevents misinterpreting window sizes or flags due to damaged header bytes.
2. **Block CRC32c (Castagnoli Polynomial 0x1EDC6F41):** Hardware-accelerated on modern x86 (SSE4.2) and ARM (ARMv8 CRC32) architectures. Each block can optionally verify its decompressed output against this 32-bit checksum.
3. **Stream Checksum (XXH3-64 or CRC32c):** The stream footer includes an optional 64-bit XXH3 checksum computed over the complete uncompressed byte sequence.
4. **Truncation Sentinel:** Every complete Codrop stream MUST terminate with an explicit `END_OF_STREAM` block or an intact Footer record. An abrupt EOF prior to the terminator is flagged as `CodropError::UnexpectedEof`.

---

## 10. Decoder Requirements

Every compliant Codrop decoder MUST enforce:
1. **Bounded Allocations:** Memory allocation MUST be bounded by the negotiated window size plus maximum block output buffer size. Encoders cannot trigger arbitrary allocations via corrupted header values.
2. **Strict Pointer Sanitization:** All match offsets $D$ must satisfy $1 \le D \le \text{Current Ring Buffer History}$. Any offset pointing into uninitialized history MUST cause immediate decoding failure (`CodropError::InvalidOffset`).
3. **No Undefined Behavior:** Decoding untrusted, randomized, or malicious inputs must return an explicit `Err(CodropError)` without panic, segfault, buffer overrun, or infinite looping.
4. **Decompression Bomb Guard:** Configurable maximum output threshold (e.g. `max_output_bytes: Option<u64>`). When decompressed bytes exceed the limit, decoding halts immediately.

---

## 11. Encoder Requirements

1. **Format Compliance:** Must produce byte sequences conforming strictly to the binary layout defined in Section 4–6.
2. **Deterministic Output:** For a specified compression level, window size, and platform, encoding identical byte buffers must produce bit-for-bit identical `.cdp` output.
3. **Expansion Safeguard:** If compression of a block yields `compressed_bytes >= original_bytes`, the encoder MUST automatically fallback to Block Type 0 (`RAW`).

---

## 12. Adaptive Compression Strategy

The encoder selects algorithms dynamically based on a fast preamble assessment:

```text
               Input Chunk (e.g., 128 KB)
                           │
             ┌─────────────▼─────────────┐
             │ Fast Analysis & Sampling  │
             │ - Shannon Entropy H(X)    │
             │ - Null / Byte Repetition  │
             │ - Character Set (ASCII?)  │
             │ - Short Hash Match Probe  │
             └─────────────┬─────────────┘
                           │
      ┌────────────────────┼────────────────────┐
      ▼                    ▼                    ▼
H(X) > 7.7           H(X) < 1.0           ASCII / Structured
Incompressible       Sparse / Zeroed      Text / JSON / Logs
      │                    │                    │
┌─────▼─────┐        ┌─────▼─────┐        ┌─────▼─────┐
│ Type 0:   │        │ Type 1:   │        │ Type 5:   │
│ STORED    │        │ RLE Block │        │ Text Pre  │
│ (Raw)     │        └───────────┘        │ + LZ/ANS  │
└───────────┘                             └─────┬─────┘
                                                │
                                    ┌───────────▼───────────┐
                                    │ Level Selection Check │
                                    │ FAST     => Type 2 LZF│
                                    │ BALANCED => Type 3 LZH│
                                    │ COMPACT  => Type 4 LZA│
                                    └───────────────────────┘
```

### Metrics Evaluated in Sampling
- **Entropy Estimate:** Evaluated on a 4 KB sample using a fast histogram ($-\sum p_i \log_2 p_i$). If $H > 7.75$ bits/byte, data is likely encrypted or already compressed; bypass matching directly.
- **Match Probing:** Step-sampled hash probes determine if duplicate 4-byte sequences exist. If repeat probability is under 2%, skip expensive deep chain matching.

---

## 13. Error Model

All Codrop operations return structured, non-panicking errors:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodropError {
    InvalidMagic([u8; 4]),
    UnsupportedVersion { major: u8, minor: u8 },
    HeaderChecksumMismatch { expected: u8, actual: u8 },
    CorruptedHeader(&'static str),
    InvalidBlockType(u8),
    InvalidOffset { offset: usize, max_valid: usize },
    InvalidMatchLength { length: usize, remaining_output: usize },
    CorruptedEntropyStream(&'static str),
    BlockChecksumMismatch { expected: u32, actual: u32 },
    StreamChecksumMismatch { expected: u64, actual: u64 },
    UnexpectedEof,
    DecompressionBombDetected { limit: u64, requested: u64 },
    MemoryLimitExceeded { limit: usize, requested: usize },
    Io(String),
}
```

---

## 14. Security Considerations

1. **Untrusted Input Ingestion:** Codrop decoders will run in web browser WASM environments and edge daemons processing user uploads. Decoders must assume all bytes are hostile.
2. **Memory Exponent Limitation:** Header-specified window sizes are strictly checked against a safe upper bound ($2^{27}$ = 128 MB). Unreasonable requests error out prior to buffer allocation.
3. **No Recursive Expansions:** Dictionary references and token prefilters are strictly non-recursive. Single-symbol replacements cannot expand indefinitely.
4. **Zero-Safe Decompressor Loop:** Iteration over literal and match tokens explicitly subtracts from remaining uncompressed budget. Match lengths cannot trigger negative overflow or slice out of bounds.

---

## 15. Compatibility Policy

- **Forward Compatibility:** Any 1.x decoder will parse all valid 1.0 streams. If an unknown non-essential block flag or metadata block is encountered, it is safely ignored.
- **Backward Compatibility:** Future 2.x decoders will retain the complete 1.x decompression engine.
- **Encoder Freedom:** Any encoder improvement (e.g. enhanced match-finding heuristic, better lazy evaluation, improved entropy model) that generates valid 1.x block tokens is supported without decoder updates.

---

## 16. API Design

### 16.1 High-Level Ergonomic API
```rust
pub fn compress(data: &[u8], level: CompressionLevel) -> Result<Vec<u8>, CodropError>;
pub fn decompress(compressed: &[u8]) -> Result<Vec<u8>, CodropError>;
pub fn decompress_with_limit(compressed: &[u8], max_output_bytes: usize) -> Result<Vec<u8>, CodropError>;
```

### 16.2 Streaming API
```rust
pub struct Encoder<W: std::io::Write> { /* fields */ }
impl<W: std::io::Write> Encoder<W> {
    pub fn new(writer: W, options: EncoderOptions) -> Self;
    pub fn write_chunk(&mut self, data: &[u8]) -> Result<usize, CodropError>;
    pub fn finish(self) -> Result<W, CodropError>;
}

pub struct Decoder<R: std::io::Read> { /* fields */ }
impl<R: std::io::Read> Decoder<R> {
    pub fn new(reader: R, options: DecoderOptions) -> Self;
    pub fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize, CodropError>;
}
```

---

## 17. Benchmark Methodology

To ensure integrity and scientific reproducibility:
1. **Metrics Tracked:**
   - Compression Ratio: $\frac{\text{Compressed Size}}{\text{Uncompressed Size}}$
   - Compression Throughput (MB/s, user-space wall clock)
   - Decompression Throughput (MB/s, user-space wall clock)
   - Peak Memory Allocation (RSS via jemalloc / heap profiler)
2. **Reference Baselines:**
   - `gzip` (Deflate, levels 1, 6, 9)
   - `brotli` (levels 1, 6, 11)
   - `zstd` (levels 1, 3, 7, 19)
   - `lz4` / `snappy` (for fast-profile comparisons)
3. **Corpora Evaluated:**
   - Standard: Silesia Corpus, Canterbury Corpus, Large Text (enwik8/enwik9).
   - Real-World: Web assets (minified JS, CSS, JSON API payloads), CSV log dumps, binary game assets, small payloads (<1 KB, 1–16 KB).

---

## 18. Future Extension Mechanism

1. **Metadata Blocks (`Type 6`):** Can carry non-decompressed metadata (e.g., indexing markers, parallel seek tables, provenance hashes). Decoders unaware of the metadata type safely skip it using the length prefix.
2. **External Pre-Shared Dictionaries:** Flag bit 2 indicates pre-shared dictionary ID. Decoders verify availability of the matching dictionary hash before beginning block parsing.
3. **Hardware Accelerators:** Block format is designed to align with SIMD vectorization (AVX2 / NEON bit-unpacking and match replication).
