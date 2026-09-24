export const LEVEL_AUTO = 0;
export const LEVEL_FAST = 1;
export const LEVEL_BALANCED = 2;
export const LEVEL_COMPACT = 3;
export const version: string;

/**
 * Compresses an input buffer into .cdp format using Codrop.
 */
export function compress(input: Uint8Array | Buffer, level?: number): Buffer;

/**
 * Decompresses a .cdp buffer back into original uncompressed bytes.
 */
export function decompress(compressed: Uint8Array | Buffer, maxOutputBytes?: number): Buffer;
