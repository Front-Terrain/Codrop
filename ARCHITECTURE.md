# Codrop Architecture & Research Design Document

**Author:** Lead Systems Engineer & Compression Researcher  
**Ecosystem:** Codrop Project  
**Target Architecture:** Cross-Platform (Rust Native, WASM, C ABI, Mobile)

---

## 1. Research: Deep Dive into Existing Codecs

To design an enduring compression system, we must dissect the strengths, failure modes, and architectural boundaries of existing codecs.

### 1.1 Comparative Analysis Matrix

| Codec | Match Finding | Entropy Coding | Window / History | Strengths | Tradeoffs / Weaknesses |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Deflate (gzip / zlib)** | Hash chain (3-byte matches) | Canonical Huffman (Static / Dynamic) | Fixed 32 KB | Ubiquitous hardware & OS support; low memory footprint. | Rigid 32 KB window limits ratio on large repetitive data; slow Huffman decompression; bit-level entropy packing. |
| **Brotli** | Hash tables, complex dictionary, context modeling | Canonical Huffman + 2nd order context | Sliding window up to 16 MB + 120 KB static web dictionary | Exceptional ratio on web assets (HTML, JS, CSS); static dictionary provides instant small payload compression. | Slow compression at high levels (10-11); high memory consumption during compression; complex decoder state machine. |
| **Zstandard (zstd)** | Hash tables, binary search trees, row-hash | Finite State Entropy (tANS) + Huffman | 8 MB default (up to 2 GB) | Symmetrical decompression speed across levels; scalable levels (1–22); fast tANS entropy; dictionary training. | Heavy code size / binary footprint; complex C codebase with extensive macro architecture difficult to embed into lightweight WASM without bloat. |
| **LZ4** | Byte-aligned hash table (4-byte minimum) | None (Raw literals + byte-aligned tokens) | 64 KB default | Blazing decompression speed (multiple GB/s per core); minimal CPU overhead. | Poor compression ratio on non-trivial repetitive data; lacks entropy stage. |
| **Snappy** | Hash table (4-byte matches) | None | 64 KB block framing | High, predictable throughput; zero bit-level operations; robust against malformed input. | Compression ratio is low; unsuitable when bandwidth or storage costs outweigh CPU cycles. |
| **LZMA (xz / 7-Zip)** | Multi-probe binary trees / patricia trees | Range Coder (arithmetic variant) + bit models | Up to 1 GB+ | Highest compression ratio in general-purpose computing; tight modeling. | Extremely slow compression and decompression; prohibitive memory usage; unsuitable for real-time streaming or low-power devices. |

---

## 2. Tradeoffs & Codrop's Differentiation Vector

### 2.1 Where Existing Codecs Fall Short
1. **The Web & Edge Fragmentation Problem:** Web browsers natively ship Brotli and gzip, but lack native zstd streaming decompression APIs in JavaScript without bundling large, multi-megabyte C/WASM runtimes.
2. **Monolithic Algorithm Inflexibility:** Deflate and Brotli use one fundamental matching pipeline regardless of input characteristics. Passing random or encrypted data incurs CPU cycles attempting futile LZ matching before falling back.
3. **Small Payload Inefficiency:** Standard zstd or gzip headers easily exceed 10–30 bytes, resulting in negative compression on tiny microservice payloads (JSON RPC, IPC, WebSocket frames under 512 bytes).
4. **Decoder Code-Size Bloat:** Standard zstd reference implementations with full dictionary support and ASM can exceed 500 KB–1 MB compiled, which is excessive for browser micro-workers and embedded microcontrollers.

### 2.2 Codrop's Strategic Differentiation
1. **Adaptive Strategy Dispatch:** Codrop analyzes block entropy and character distribution *before* committing CPU budget to match finding. Uncompressible blocks become instant RAW; repetitive tabular blocks receive byte-delta filters; code and text trigger dictionary-assisted tokenization.
2. **Modular Entropy Hierarchy:**
   - Level `FAST`: Raw byte tokens (LZ4-speed class).
   - Level `BALANCED`: Canonical Huffman with fast lookup tables.
   - Level `COMPACT`: Table-based Asymmetric Numeral Systems (tANS) for fractional-bit precision.
3. **Zero Negative Compression Guarantee:** Enforced at the block level. If a compressed block does not beat raw size by a configured delta, it is emitted verbatim as a RAW block.
4. **Lightweight & Portable Decoder Target:** The Rust core compiles to a lean WASM module (~50–80 KB compressed) with zero heap fragmentation, safe bounds checking, and predictable memory overhead.

---

## 3. Technology Implementation Phasing

### 3.1 Initial Implementation (Phase 1–4)
- **Container Framing:** Magic `CDP1`, Header, Dynamic Block Headers, Frame Checksums (CRC32c / Adler32).
- **Match Finders:**
  - *Fast Matcher:* Hash table with direct 4-byte indexing for linear-speed compression.
  - *Chained Matcher:* Hash chains with bounded search depth for higher ratios.
- **Block Types:**
  - Type 0: `RAW / STORED` (zero overhead fallback).
  - Type 1: `RLE` (fast byte-run collapsing).
  - Type 2: `LZF` (Fast LZ with byte-aligned token packing).
  - Type 3: `LZH` (LZ + Canonical Huffman).
- **Adaptive Engine:** Fast Shannon entropy calculator and heuristic classifier.
- **Verification:** CRC32c per-block and XXH3-64 stream integrity check.

### 3.2 Deferred Implementation (Phase 5–7)
- **tANS (Finite State Entropy):** Implemented in Phase 5 after Huffman baseline is thoroughly fuzzed and verified.
- **Text & JSON Pre-filters:** Byte-level dictionary substitution for common programming tokens (`function`, `true`, `false`, `null`, `style=`, `<div>`).
- **Pre-Shared Static Dictionaries:** Web asset dictionary similar to Brotli's static dictionary.

### 3.3 Experimentally Promising Research Vectors
- **SIMD Match Finding:** Vectorized byte-difference comparisons (AVX2/NEON) to accelerate match length calculations.
- **Context-Adaptive Run Coding:** Dynamic switching between run-length and match states for binary columnar datasets.

---

## 4. System Component Decomposition

```
codrop/
├── libcodrop/                  # Core Rust library
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              # Public high-level APIs
│       ├── error.rs            # CodropError enum and Result aliases
│       ├── format/             # Binary format definitions
│       │   ├── mod.rs
│       │   ├── header.rs       # Stream header parsing and serialization
│       │   ├── block.rs        # Block headers and types
│       │   └── flags.rs        # Flag bitfields
│       ├── analysis/           # Adaptive data profiling
│       │   ├── mod.rs
│       │   ├── entropy.rs      # Shannon entropy & histogram
│       │   └── classifier.rs   # Strategy selector
│       ├── matcher/            # LZ match finders
│       │   ├── mod.rs
│       │   ├── fast.rs         # Direct hash table (FAST level)
│       │   ├── chain.rs        # Hash chain (BALANCED level)
│       │   └── optimal.rs      # Bounded lazy match parser
│       ├── entropy/            # Entropy coders
│       │   ├── mod.rs
│       │   ├── huffman/        # Canonical Huffman encoder/decoder
│       │   │   ├── tree.rs
│       │   │   └── bitstream.rs
│       │   └── rle.rs          # Run-length coder
│       ├── codec/              # Block codecs
│       │   ├── mod.rs
│       │   ├── raw.rs          # Passthrough codec
│       │   ├── lzf.rs          # Fast byte-aligned LZ
│       │   └── lzh.rs          # LZ + Huffman pipeline
│       ├── streaming/          # Streaming encoder and decoder
│       │   ├── mod.rs
│       │   ├── encoder.rs      # Write / Flush / Finish
│       │   └── decoder.rs      # Read / Chunked stream
│       └── checksum/           # Integrity validation
│           ├── mod.rs
│           ├── crc32c.rs       # CRC32c implementation
│           └── xxh3.rs         # Stream hash
├── codrop-cli/                 # Command-line utility
│   ├── Cargo.toml
│   └── src/
│       └── main.rs             # CLI commands: compress, decompress, inspect, bench
├── codrop-wasm/                # WebAssembly package
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs              # wasm-bindgen bindings
├── codrop-python/              # PyO3 bindings
├── @codrop/node                # Node.js NAPI bindings
└── codrop-mobile/              # C FFI bindings for Swift/Kotlin
```

---

## 5. Algorithmic Deep Dive

### 5.1 Adaptive Profiler & Heuristic Classifier

The adaptive profiler examines an input chunk $S$ with length $N$.
1. **Histogram & Entropy:** A 256-bin table counts byte frequencies over a sample of size $M = \min(N, 4096)$ bytes.
   $$p_i = \frac{\text{count}[i]}{M}, \quad H = -\sum_{i=0}^{255} p_i \log_2(p_i)$$
2. **Decision Matrix:**
   - If $H \ge 7.75$ bits/byte: The data exhibits near-maximal randomness (compressed archives, encrypted data). Commit immediately to **Block Type 0 (RAW)**, saving 100% of match-finding CPU cycles.
   - If $p_{\text{dominant}} \ge 0.60$ or unique symbols $< 8$: The block is heavily homogeneous. Commit to **Block Type 1 (RLE)**.
   - If $H < 7.75$: Compute a 4-byte hash match probe. If match density is high, dispatch to **LZF** or **LZH** depending on requested `CompressionLevel`.

### 5.2 Match Finding: Fast vs. Chained

#### Fast Matcher (Level `FAST`)
- Hash table of size $2^{14}$ to $2^{16}$ entries.
- For each position $i$:
  $$\text{hash} = ((\text{read\_u32}(i) \times 0x9E3779B1) \gg 16) \pmod{\text{TableSize}}$$
- Direct replacement: check previous position at `table[hash]`. If bytes match for $\ge 3$ bytes, record match and step forward by match length. If no match, emit literal. Single-probe, zero memory chaining.

#### Chained Matcher (Level `BALANCED` / `COMPACT`)
- Maintains a head table and a link array (sliding window chain).
- Evaluates up to `max_chain_depth` matches (e.g. 16 to 64 depth).
- Evaluates lazy matching: if match at position $i$ is found with length $L$, test position $i+1$. If position $i+1$ finds length $L' > L$, emit position $i$ as literal and take the longer match.

### 5.3 Entropy Coding: Canonical Huffman

To achieve fast decompression:
1. Symbol lengths are limited to a maximum depth of 15 bits (allowing standard 16-bit register lookups).
2. The decompressor generates a primary lookup table of $2^{10} = 1024$ entries for single-cycle symbol resolution.
3. Codes longer than 10 bits resolve via a secondary branch table.
4. Table descriptors in the block preamble are run-length compressed to minimize overhead on small blocks.

---

## 6. Execution Flow & State Machines

### 6.1 Encoder State Machine
```
[Uncompressed Input]
         │
         ▼
[Buffer in Window Ring] ─── Has Full Block? ───► No ───► Wait for Write/Finish
         │ Yes
         ▼
[Profile & Classify]
         │
         ├────────────────────────┬────────────────────────┐
         ▼                        ▼                        ▼
     High Entropy             Repetitive               Compressible
   [Emit RAW Block]        [Emit RLE Block]      [LZ Match Finding]
         │                        │                        │
         │                        │                        ▼
         │                        │                 Tokens & Literals
         │                        │                        │
         │                        │                [Huffman Encode]
         │                        │                        │
         └────────────────────────┼────────────────────────┘
                                  ▼
                        [Compute Block Checksum]
                                  │
                                  ▼
                         [Write Block Header]
                                  │
                                  ▼
                        [Emit Block Payload]
                                  │
                                  ▼
                       [Slide History Window]
```

### 6.2 Decoder State Machine
```
[Read 4-byte Magic] ─── Valid "CDP1"? ───► No ───► Err(InvalidMagic)
         │ Yes
         ▼
[Parse Stream Header] ─── CRC-8 Valid? ───► No ───► Err(HeaderChecksumMismatch)
         │ Yes
         ▼
[Allocate Ring Buffer] (Size = Window Descriptor)
         │
         ▼
┌──► [Read Block Header] ─── Type == END_OF_STREAM? ───► [Check Stream Hash] ──► Done
│        │
│        ▼
│    Check Compressed Size & Bounds
│        │
│        ├───────────────┬───────────────┬───────────────┐
│        ▼               ▼               ▼               ▼
│    Type 0 (RAW)   Type 1 (RLE)   Type 2 (LZF)    Type 3 (LZH)
│    Copy direct    Expand runs    Decode tokens   Decode Huffman
│        │               │         Copy matches    Expand tokens
│        └───────────────┼───────────────┴───────────────┘
│                        ▼
│              [Verify Block CRC32c] (if enabled)
│                        ▼
│              [Emit Decompressed Chunk]
│                        ▼
└────────────────────── Next Block
```

---

## 7. Memory & Allocation Strategy

- **Encoder Working Memory:**
  - Small profile: 64 KB window + 32 KB hash table $\approx$ 128 KB total footprint.
  - Default profile: 2 MB window + 512 KB hash chains $\approx$ 3.5 MB total footprint.
- **Decoder Working Memory:**
  - Guaranteed bounded by negotiated window size (default 2 MB) + one output block buffer (128 KB).
  - No unbounded recursion, no dynamic tree allocations in decoding loops. Tables are allocated in flat, reusable vectors or stack buffers where appropriate.
