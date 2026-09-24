# Codrop Comprehensive Benchmark & Evaluation Plan

**Ecosystem:** Codrop Project  
**Target Codecs Compared:** Codrop, Zstandard (v1.5.x), Brotli (v1.1.x), gzip / zlib (v1.3.x), LZ4 (v1.9.x)  
**Output Formats:** Human-readable CLI table, machine-readable JSON & CSV

---

## 1. Objectives & Principles of Benchmark Integrity

1. **Empirical Honesty:** Codrop will not claim superiority over established industrial codecs without rigorous, statistically verified benchmarks across diverse corpora.
2. **Reproducibility:** Every benchmark must be reproducible using open datasets and automated benchmark commands with pinned versions and environment metadata.
3. **Multi-Dimensional Metrics:** Codecs cannot be evaluated on ratio alone or throughput alone. The Pareto frontier (ratio vs. compression speed vs. decompression speed vs. memory consumption) must be mapped comprehensively.
4. **Transparent Accounting of Losses:** If Codrop underperforms on specific categories (e.g. ultra-high compression levels compared to LZMA or pre-trained dictionary zstd), this must be explicitly documented with diagnostic analysis.

---

## 2. Benchmark Corpora & Datasets

To ensure comprehensive real-world representation, the benchmark suite evaluates datasets spanning five categories:

### 2.1 Standard Classical Corpora
- **Silesia Compression Corpus (211 MB total):**
  - `dickens`: English literature text (plain text)
  - `mozilla`: Tar archive of Mozilla source/executables
  - `mr`: Medical resonance imaging data (binary 16-bit integers)
  - `osdb`: Relational database table file
  - `samba`: Source code tar archive
  - `webster`: English dictionary (formatted text)
  - `xml`: Large XML dataset
- **Canterbury Corpus:**
  - Standard baseline files for algorithmic sanity checks.
- **Large Text Benchmark:**
  - `enwik8` (100 MB Wikipedia dump)
  - `enwik9` (1 GB Wikipedia dump)

### 2.2 Modern Web & Cloud Payloads
- **JSON Payloads:**
  - Small JSON API responses (1 KB – 64 KB, GitHub API, GeoJSON, Twitter API dumps)
  - Large serialized JSON documents (10 MB – 50 MB, MongoDB / Elasticsearch dumps)
- **Web Source Assets:**
  - Minified JavaScript (React, Vue, lodash, Three.js bundles)
  - Minified CSS (Tailwind, Bootstrap outputs)
  - Formatted HTML pages
- **System Logs & Tabular Data:**
  - Nginx / Apache access logs (ASCII structured, repetitive timestamps and IP patterns)
  - Financial CSV tables (numeric columns with floating-point strings)

### 2.3 Binary & High-Entropy Data
- **Executables & Object Code:**
  - ELF / PE / Mach-O binaries and WebAssembly (`.wasm`) modules
- **Compressed & Encrypted Media (Incompressible stress tests):**
  - JPEG images, PNG files, MP4 fragments, `/dev/urandom` samples (verifies adaptive zero-overhead bypass)

### 2.4 Micro & Small Payloads (< 512 B to 16 KB)
- Critical for RPC, microservices, and database cell compression.
- Evaluates framing overhead and prevents negative compression (`compressed_size > original_size`).

---

## 3. Comparative Test Matrix

Each dataset will be measured across the following codec configurations:

| Codec | Profile / Level | Target Use Case |
| :--- | :--- | :--- |
| **Codrop** | `FAST` | High throughput, real-time streaming, network proxy |
| **Codrop** | `BALANCED` (Default) | General-purpose interchange, web serving |
| **Codrop** | `COMPACT` | Archival storage, cold backups, low bandwidth |
| **Codrop** | `AUTO` | Adaptive heuristic auto-selection |
| **Zstandard** | Level 1, 3 (Default), 7, 19 | Industry standard scalable benchmark |
| **Brotli** | Level 1, 4, 6, 11 | Web serving and high-ratio distribution |
| **gzip (zlib)** | Level 1, 6 (Default), 9 | Ubiquitous legacy baseline |
| **LZ4** | Default (`-1`), High (`-9`) | Ultra-fast throughput baseline |

---

## 4. Measurement Metrics & Instrument Protocols

### 4.1 Primary Metrics
1. **Compressed Size (Bytes):** Exact serialized bytes written to disk/stream.
2. **Compression Ratio:**
   $$\text{Ratio} = \frac{\text{Compressed Size}}{\text{Uncompressed Size}}$$
   $$\text{Savings \%} = \left(1 - \frac{\text{Compressed Size}}{\text{Uncompressed Size}}\right) \times 100\%$$
3. **Compression Throughput (MB/s):**
   $$\text{MB/s} = \frac{\text{Uncompressed Size (Bytes)}}{10^6 \times \text{Elapsed Time (Seconds)}}$$
4. **Decompression Throughput (MB/s):** Measured by decompressing from memory into memory (or sink buffer) to isolate CPU performance from disk I/O.
5. **Peak Memory Consumption (RSS):** Tracked using allocation wrappers / OS process memory tracking.
6. **Framing Overhead on Small Payloads:** Byte difference between raw and compressed on payloads $\le 1024$ bytes.

### 4.2 Statistical Controls
- **Warm-up Iterations:** Minimum 3 warm-up runs to ensure file system caches, CPU branch predictors, and memory allocators reach steady-state.
- **Sample Count:** Minimum 10 measurement iterations per file.
- **Reporting:** Median, 5th percentile, and 95th percentile throughput to filter out OS scheduler noise.
- **CPU Affinity:** Single-core pinning (via `taskset` on Linux or equivalent core-binding) to ensure consistent frequency without cross-core cache invalidation.

---

## 5. Machine-Readable Result Schema

Results are exported to `benchmarks/results/benchmark_run_<timestamp>.json`:

```json
{
  "system_info": {
    "os": "Windows 11 / Linux / macOS",
    "cpu": "x86_64 / aarch64",
    "cpu_cores": 8,
    "compiler": "rustc 1.8x",
    "timestamp_utc": "2026-09-24T13:30:00Z"
  },
  "benchmarks": [
    {
      "dataset": "silesia/dickens",
      "original_size_bytes": 10192446,
      "codec": "codrop",
      "level": "BALANCED",
      "compressed_size_bytes": 3845120,
      "ratio": 0.37725,
      "savings_pct": 62.275,
      "compress_mb_per_sec": 78.4,
      "decompress_mb_per_sec": 312.6,
      "peak_memory_kb": 3520
    }
  ]
}
```

---

## 6. Automated Benchmark Suite Architecture

```
benchmarks/
├── Cargo.toml
├── benches/
│   ├── throughput.rs          # Criterion micro-benchmarks
│   ├── small_payloads.rs      # Micro-benchmark for <1KB packets
│   └── comparative.rs         # Direct Codrop vs. Zstd vs. Brotli vs. Gzip
├── datasets/
│   ├── download_corpora.sh    # Script to fetch public Silesia/Canterbury
│   └── synthetic/             # Generators for JSON, logs, random noise
├── harness/
│   ├── src/
│   │   ├── main.rs            # Orchestrator CLI for full matrix runs
│   │   ├── runner.rs          # Process executor and timer
│   │   └── reporter.rs        # Table renderer & JSON/CSV exporter
└── results/                   # Historical benchmark runs for regression detection
```

---

## 7. Milestone Criteria for Phase Completion

Before declaring the Phase 9 Benchmarking milestone complete:
1. Codrop `FAST` must demonstrate decompression throughput competitive with mid-tier LZ codecs ($> 300\text{ MB/s}$ on modern single-core x86_64).
2. Codrop `BALANCED` must achieve a compression ratio substantially outperforming gzip level 6 on text and web assets while maintaining faster decompression.
3. On high-entropy incompressible inputs (e.g. `/dev/urandom` or `.jpg`), Codrop `AUTO` must incur $\le 0.1\%$ size expansion and bypass match finding within $< 5\text{ ms/MB}$.
4. Small payloads ($< 256\text{ bytes}$) must not expand beyond header framing (under 8–12 bytes total container overhead).
