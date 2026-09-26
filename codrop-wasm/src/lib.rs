//! WebAssembly bindings for Codrop.
//!
//! Provides browser- and Node.js-compatible WebAssembly endpoints for compressing
//! and decompressing `.cdp` streams.

use libcodrop::{compress as codrop_compress_rs, decompress_with_limit, CompressionLevel};

/// Returns the current Codrop WASM engine version string.
pub fn version() -> &'static str {
    "1.0.0"
}

/// Compress an in-memory byte slice using the given level (0: Fast, 1: Balanced, 2: Compact, 3: Auto).
pub fn compress(data: &[u8], level: u8) -> Result<Vec<u8>, String> {
    let comp_level = match level {
        0 => CompressionLevel::Fast,
        1 => CompressionLevel::Balanced,
        2 => CompressionLevel::Compact,
        _ => CompressionLevel::Auto,
    };
    codrop_compress_rs(data, comp_level).map_err(|e| e.to_string())
}

/// Decompress a `.cdp` byte slice with an optional maximum size limit (default 1 GB).
pub fn decompress(compressed: &[u8], max_size: Option<u64>) -> Result<Vec<u8>, String> {
    let limit = max_size.unwrap_or(1024 * 1024 * 1024);
    decompress_with_limit(compressed, limit).map_err(|e| e.to_string())
}

// C-style WebAssembly entry points for universal web/runtime integration

/// Compresses a buffer into a newly allocated output buffer.
///
/// # Safety
///
/// * `src` must point to at least `src_len` valid, readable bytes.
/// * `out_ptr` and `out_len` must be non-null and point to valid, writable memory.
/// * The allocated buffer returned via `*out_ptr` must be deallocated using [`wasm_free`].
#[no_mangle]
pub unsafe extern "C" fn wasm_compress(
    src: *const u8,
    src_len: usize,
    level: u8,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    libcodrop::ffi::codrop_compress(src, src_len, level, out_ptr, out_len)
}

/// Decompresses a `.cdp` buffer into a newly allocated output buffer.
///
/// # Safety
///
/// * `src` must point to at least `src_len` valid, readable bytes.
/// * `out_ptr` and `out_len` must be non-null and point to valid, writable memory.
/// * The allocated buffer returned via `*out_ptr` must be deallocated using [`wasm_free`].
#[no_mangle]
pub unsafe extern "C" fn wasm_decompress(
    src: *const u8,
    src_len: usize,
    max_output_bytes: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    libcodrop::ffi::codrop_decompress(src, src_len, max_output_bytes, out_ptr, out_len)
}

/// Frees a buffer allocated by [`wasm_compress`] or [`wasm_decompress`].
///
/// # Safety
///
/// * `ptr` must be a pointer returned by [`wasm_compress`] or [`wasm_decompress`].
/// * `len` must match the length written into `out_len`.
#[no_mangle]
pub unsafe extern "C" fn wasm_free(ptr: *mut u8, len: usize) {
    libcodrop::ffi::codrop_free(ptr, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_version() {
        assert_eq!(version(), "1.0.0");
    }

    #[test]
    fn test_wasm_compress_decompress_roundtrip() {
        let input = b"Hello from Codrop WebAssembly engine!";
        let compressed = compress(input, 1).expect("compression should succeed");
        let decompressed = decompress(&compressed, None).expect("decompression should succeed");
        assert_eq!(decompressed, input);
    }
}
