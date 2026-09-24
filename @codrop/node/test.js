const assert = require('assert');
const codrop = require('./index');

console.log('Codrop version:', codrop.version);

const sample = Buffer.from('Hello from Codrop WebAssembly Node.js runtime! Testing adaptive compression in pure JS/WASM.');
console.log('Original bytes:', sample.length);

const compressed = codrop.compress(sample, codrop.LEVEL_BALANCED);
console.log('Compressed bytes:', compressed.length);

const restored = codrop.decompress(compressed);
console.log('Restored bytes:', restored.length);

assert.deepStrictEqual(restored, sample);
console.log('Test PASSED! Exact roundtrip confirmed.');
