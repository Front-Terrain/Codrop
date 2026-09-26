const fs = require('fs');
const path = require('path');

let wasmInstance = null;

function getWasmInstance() {
  if (wasmInstance) return wasmInstance;
  const wasmPath = path.join(__dirname, 'codrop_wasm.wasm');
  const wasmBuffer = fs.readFileSync(wasmPath);
  const wasmModule = new WebAssembly.Module(wasmBuffer);
  wasmInstance = new WebAssembly.Instance(wasmModule, {});
  return wasmInstance;
}

const LEVEL_AUTO = 0;
const LEVEL_FAST = 1;
const LEVEL_BALANCED = 2;
const LEVEL_COMPACT = 3;

/**
 * Compress a Buffer or Uint8Array using Codrop.
 * @param {Uint8Array|Buffer} input - Input uncompressed bytes
 * @param {number} [level=2] - Compression level (0=Auto, 1=Fast, 2=Balanced, 3=Compact)
 * @returns {Buffer} Compressed bytes in .cdp format
 */
function compress(input, level = LEVEL_BALANCED) {
  const instance = getWasmInstance();
  const { memory, wasm_compress, wasm_free } = instance.exports;

  const srcBytes = Buffer.isBuffer(input) ? input : Buffer.from(input);
  const srcLen = srcBytes.length;

  // Allocate 8 bytes for out_ptr (4 bytes) and out_len (4 bytes) in wasm32
  // We can write to a small scratch area in memory, or use memory.grow
  // To be safe, allocate scratch space at the end of memory
  const scratchOffset = 1024; // Use first 1KB as scratch
  const view = new DataView(memory.buffer);

  // Write input buffer starting at offset 2048
  const inputOffset = 2048;
  const memUint8 = new Uint8Array(memory.buffer);
  
  // Ensure memory has enough pages
  const neededBytes = inputOffset + srcLen + 65536;
  if (neededBytes > memory.buffer.byteLength) {
    const additionalPages = Math.ceil((neededBytes - memory.buffer.byteLength) / 65536);
    memory.grow(additionalPages);
  }

  const updatedMem = new Uint8Array(memory.buffer);
  updatedMem.set(srcBytes, inputOffset);

  const outPtrAddr = scratchOffset;
  const outLenAddr = scratchOffset + 4;

  const res = wasm_compress(inputOffset, srcLen, level, outPtrAddr, outLenAddr);
  if (res !== 0) {
    throw new Error(`Codrop compression failed with error code: ${res}`);
  }

  const updatedView = new DataView(memory.buffer);
  const resultPtr = updatedView.getUint32(outPtrAddr, true);
  const resultLen = updatedView.getUint32(outLenAddr, true);

  const finalMem = new Uint8Array(memory.buffer);
  const output = Buffer.from(finalMem.subarray(resultPtr, resultPtr + resultLen));

  wasm_free(resultPtr, resultLen);
  return output;
}

/**
 * Decompress a .cdp buffer using Codrop.
 * @param {Uint8Array|Buffer} compressed - Input compressed .cdp bytes
 * @param {number} [maxOutputBytes=1073741824] - Maximum allowed decompressed bytes
 * @returns {Buffer} Decompressed bytes
 */
function decompress(compressed, maxOutputBytes = 1024 * 1024 * 1024) {
  const instance = getWasmInstance();
  const { memory, wasm_decompress, wasm_free } = instance.exports;

  const srcBytes = Buffer.isBuffer(compressed) ? compressed : Buffer.from(compressed);
  const srcLen = srcBytes.length;

  const scratchOffset = 1024;
  const inputOffset = 2048;

  const neededBytes = inputOffset + srcLen + 65536;
  if (neededBytes > memory.buffer.byteLength) {
    const additionalPages = Math.ceil((neededBytes - memory.buffer.byteLength) / 65536);
    memory.grow(additionalPages);
  }

  const updatedMem = new Uint8Array(memory.buffer);
  updatedMem.set(srcBytes, inputOffset);

  const outPtrAddr = scratchOffset;
  const outLenAddr = scratchOffset + 4;

  const res = wasm_decompress(inputOffset, srcLen, maxOutputBytes, outPtrAddr, outLenAddr);
  if (res !== 0) {
    throw new Error(`Codrop decompression failed with error code: ${res}`);
  }

  const updatedView = new DataView(memory.buffer);
  const resultPtr = updatedView.getUint32(outPtrAddr, true);
  const resultLen = updatedView.getUint32(outLenAddr, true);

  const finalMem = new Uint8Array(memory.buffer);
  const output = Buffer.from(finalMem.subarray(resultPtr, resultPtr + resultLen));

  wasm_free(resultPtr, resultLen);
  return output;
}

module.exports = {
  compress,
  decompress,
  LEVEL_AUTO,
  LEVEL_FAST,
  LEVEL_BALANCED,
  LEVEL_COMPACT,
  version: '1.0.0',
};
