# Codrop: Universal Adaptive Compression System

Codrop is a general-purpose, lossless compression system designed around:

> **Stable format, evolving encoder, portable decoder.**

The `.cdp` container format is rigorously specified so that future encoder optimizations do not break backwards compatibility with existing decoders.

---

## Current Status: Milestone 0 (M0 - Minimal Working Codec)

The current implementation is **Milestone 0 (M0)**:
- **Format:** Fully implements the `CDP1` binary stream container format (version 1.0).
- **Supported Block Types:**
  - `RAW` (Block Type 0): Verbatim byte passthrough with zero expansion.
  - `RLE` (Block Type 1): Run-Length Encoding with ULEB128 run counts.
- **Expansion Safeguard:** The encoder tests whether RLE produces a smaller representation than the original data; if not, it automatically emits `RAW` to prevent expansion.
- **Integrity Validation:** Header CRC-8, per-block Castagnoli CRC32c, whole-stream 64-bit hashing, and explicit `EndOfStream` sentinel validation.
- **Security:** Strict bounds checking, memory limit enforcement, and decompression bomb prevention.

*Note: Higher-level compression schemes (LZF, LZH/Huffman, LZA/tANS) and pre-filters are defined in the format specification and will be introduced in subsequent milestones (M1+). Encountering them in M0 returns a clean unsupported feature error.*

---

## CLI Usage

### Basic Commands

```bash
# Compress a file to .cdp
codrop compress file.txt -o file.cdp

# Decompress a .cdp container
codrop decompress file.cdp -o restored.txt

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
| `3` | **Unsupported Feature** | Unsupported block type (e.g. LZF/LZH/LZA in M0) or format version |

---

## Ecosystem Architecture

- **`libcodrop/`**: Core Rust reference implementation with zero non-standard runtime dependencies (`#![forbid(unsafe_code)]`).
- **`codrop-cli/`**: Universal command-line interface (`codrop compress`, `decompress`, `inspect`).

---

## Technical Specifications & Documentation

1. [Format & Protocol Specification (`CODROP_SPEC.md`)](file:///c:/Users/rishi/FrontTerrain/Codrop/CODROP_SPEC.md)
   - Formal binary layout of the `.cdp` container.
   - Header, flags, block types (RAW, RLE, LZF, LZH, LZA).
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

- [x] **M0 (Current):** Minimal Working Codec (`CDP1`, Header, RAW, RLE, Streaming foundation, CLI, Fuzzing)
- [ ] **M1:** Fast Byte-Aligned LZ (`LZF`)
- [ ] **M2:** Canonical Huffman Entropy & Hash Chains (`LZH`)
- [ ] **M3:** Adaptive Profiler & Strategy Classifier
- [ ] **M4:** Advanced Streaming & Window Ring Optimization
- [ ] **M5:** Finite State Entropy / tANS (`LZA`)
- [ ] **M6:** Cross-Platform Bindings (C ABI, WASM, Node.js, Python, Mobile)
