# Codrop: Universal Adaptive Compression System

Codrop is a general-purpose, lossless compression system designed around:

> **Stable format, evolving encoder, portable decoder.**

The `.cdp` container format is rigorously specified so that future encoder optimizations do not break backwards compatibility with existing decoders.

---

## Current Status: Phase 7 — Optimization, Hardening, Benchmarking & Release Readiness (v1.0.0)

Codrop is at **Release Candidate (`v1.0.0`)**:
- **Format Stability:** Strictly locked to the `CDP1` binary stream container format (version 1.0).
- **Supported Block Types:**
  - `RAW` (Block Type 0): Verbatim byte passthrough with zero expansion.
  - `RLE` (Block Type 1): Run-Length Encoding with ULEB128 run counts.
  - `LZF` (Block Type 2): Fast byte-oriented LZ engine with SIMD/8-byte vectorized match extension, 4-bit nibble token encoding, byte-aligned extended length encoding, 16-bit / 24-bit LE displacement offsets, and zero-allocation token emission.
  - `LZH` (Block Type 3): LZ matching coupled with Canonical Huffman entropy coding. Features 8-byte vectorized match extension, maximum code length bounded to 15 bits (enforced by Kraft inequality rebalancing), primary table decoder ($\le 1024$ entries), $O(1)$ length/distance table index lookups, and compact RLE-compressed table descriptors.
  - `LZA` (Block Type 4): Compact LZ matching combined with table-based Asymmetric Numeral Systems (tANS / FSE-style) entropy coding. Utilizes a deterministic finite state machine ($L = 1024$ states) spread via coprime permutation (step 643), unified 318-symbol token alphabet, normalized frequency allocation, compact RLE table descriptors, and $O(1)$ branchless table-lookup decoding.
  - `TextPrefilter` (Block Type 5): Reversible, data-aware prefilters and static dictionary transformations before LZH or LZA backend entropy compression.
    - **Delta Prefilter:** Modulo-256 byte-difference encoding with configurable stride (1..=8) for sequential and audio/waveform numeric bytes.
    - **Static Dictionaries:** Versioned registry of compiled domain dictionaries (`DICT_ID_JSON: 0x0001`, `DICT_ID_WEB: 0x0002`, version 1).
    - **JSON Prefilter:** Conservative JSON detection and repeated key/value token substitution.
    - **Text Prefilter:** Conservative text and UTF-8 detection with dynamic repeated word/phrase discovery and compact token tables.
    - **Exact Losslessness:** Dynamic minimum-frequency escape byte (`esc_byte`) mechanism with byte stuffing ensures 100% byte-for-byte exact reversibility on all inputs.
- **Universal Cross-Language Support:**
  - **C ABI (`libcodrop`):** Standard FFI with panic containment (`catch_unwind`), null-pointer safety, error codes, and dynamic buffer management (`codrop_compress`, `codrop_decompress`, `codrop_free`, `codrop_version`).
  - **WebAssembly (`codrop-wasm`):** Zero-overhead WASM crate ready for browser workers, Edge runtimes, and Node.js.
- **Hardening & Quality:**
  - **Memory Safety:** Core codec enforces `#![deny(unsafe_code)]` with all unsafe strictly isolated and documented in C ABI FFI wrappers.
  - **Golden Vectors:** Permanent cross-version regression corpus verifying every codec, prefilter, dictionary, streaming chunk, and CRC32c tamper detection.
  - **Property Tests:** Invariant validation verifying exact reversibility, non-expansion guarantee (RAW fallback), encoder determinism, and panic-free decoding on arbitrary fuzzed data.
  - **CLI Hardening:** Stdin/stdout streaming support (`-` / `-o -`), `--max-size` decompression limit protection, `-V/--version`, and explicit, predictable exit codes.

---

## CLI Usage

### Basic Commands

```bash
# Compress a file to .cdp (uses Balanced profile with LZH by default)
codrop compress file.txt -o file.cdp

# Compress using fast profile (LZF)
codrop compress file.txt -l fast -o file.cdp

# Compress using compact profile (LZA / tANS)
codrop compress file.txt -l compact -o file.cdp

# Stream compression from stdin to stdout
cat file.txt | codrop compress - -o - > file.cdp

# Stream decompression from stdin to stdout with a 64MB memory limit
cat file.cdp | codrop decompress - -o - --max-size 67108864 > restored.txt

# Inspect .cdp container metadata and block layout
codrop inspect file.cdp
```

### Exit Codes

The `codrop` CLI returns standard, predictable exit codes:

| Code | Meaning | Description |
| :--- | :--- | :--- |
| `0` | **Success** | Operation completed successfully |
| `1` | **General Failure** | Command-line argument syntax error, file I/O error |
| `2` | **Malformed Stream** | Invalid magic, corrupted header, checksum mismatch, unexpected EOF |
| `3` | **Unsupported Feature** | Unsupported block type or format version |
| `4` | **Decompression Limit Exceeded** | Decompressed output exceeded configured `--max-size` limit |

---

## Ecosystem Architecture

- **`libcodrop/`**: Core Rust reference implementation with zero non-standard runtime dependencies (`#![deny(unsafe_code)]`). Exposes high-level safe API and C ABI FFI.
- **`codrop-wasm/`**: Universal WebAssembly module targeting browsers, Node.js, and WASM runtimes.
- **`codrop-cli/`**: Universal command-line interface (`codrop compress`, `decompress`, `inspect`).

---

## Technical Specifications & Documentation

1. [Format & Protocol Specification (`CODROP_SPEC.md`)](file:///c:/Users/rishi/FrontTerrain/Codrop/CODROP_SPEC.md)
   - Formal binary layout of the `.cdp` container (v1.0).
   - Header, flags, block types (RAW, RLE, LZF, LZH, LZA, Prefilter, Metadata, EndOfStream).
   - Window sizing, checksums, and streaming semantics.
   - Security constraints and decompression bomb prevention.

2. [System Architecture & Research Document (`ARCHITECTURE.md`)](file:///c:/Users/rishi/FrontTerrain/Codrop/ARCHITECTURE.md)
   - Comparative analysis of Deflate, Brotli, Zstandard, LZ4, Snappy, LZMA.
   - Algorithmic design of match finders, entropy stages, and state machines.

3. [Benchmark & Evaluation Plan (`BENCHMARK_PLAN.md`)](file:///c:/Users/rishi/FrontTerrain/Codrop/BENCHMARK_PLAN.md)
   - Methodology, corpora (Silesia, Large Text, modern web assets, small payloads).
   - Machine-readable result schemas and reproducibility standards.

---

## Development Milestones

- [x] **M0 (Phase 2):** Minimal Working Codec (`CDP1`, Header, RAW, RLE, Streaming foundation, CLI, Fuzzing)
- [x] **M1 (Phase 3):** Fast Byte-Aligned LZ (`LZF` Block Type 2, Token Encoding, Safe Overlapping Matches)
- [x] **M2 (Phase 4):** Canonical Huffman Entropy & Hash Chains (`LZH` Block Type 3, Bitstream, Tree Rebalancing)
- [x] **M5 (Phase 5):** Finite State Entropy / tANS (`LZA` Block Type 4, Table-based ANS State Machine)
- [x] **Phase 6:** Data-Aware Prefilters & Static Dictionaries (Text, JSON, Delta, Versioned Dictionaries)
- [x] **M6:** Cross-Platform Bindings (C ABI, WASM, Node.js, Python, Dart/Flutter for pub.dev, Mobile)
