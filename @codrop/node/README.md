# @codrop/node

Official Node.js and WebAssembly SDK for the **Codrop Universal Adaptive Compression System**.

Zero native build dependencies (pure WASM + JavaScript runtime).

## Installation

```bash
npm install @codrop/node
```

## Quick Start

```javascript
const codrop = require('@codrop/node');

// Uncompressed buffer
const data = Buffer.from('Hello from Codrop! Universal adaptive compression for Node.js.');

// Compress (.cdp format)
const compressed = codrop.compress(data, codrop.LEVEL_BALANCED);
console.log(`Original: ${data.length} bytes, Compressed: ${compressed.length} bytes`);

// Decompress
const restored = codrop.decompress(compressed);
console.log(restored.toString('utf-8'));
```

## Compression Levels

- `codrop.LEVEL_AUTO` (0): Adaptive profile
- `codrop.LEVEL_FAST` (1): LZF byte-aligned LZ engine
- `codrop.LEVEL_BALANCED` (2): LZH (LZ + Canonical Huffman)
- `codrop.LEVEL_COMPACT` (3): LZA (LZ + tANS / Finite State Entropy)

## License

Licensed under MIT OR Apache-2.0.
