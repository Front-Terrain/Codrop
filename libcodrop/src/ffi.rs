//! C ABI Foreign Function Interface (FFI) for Codrop.
//!
//! Exposes a stable, panic-safe C-compatible API for embedding Codrop into
//! C, C++, Python, Node.js, and mobile applications.

#![allow(unsafe_code)]

use std::os::raw::c_char;
use std::panic::catch_unwind;

use crate::{compress, decompress_with_limit, CompressionLevel};

/// Return codes for C ABI functions.
pub const CODROP_OK: i32 = 0;
pub const CODROP_ERR_NULL_PTR: i32 = -1;
pub const CODROP_ERR_CORRUPT: i32 = -2;
pub const CODROP_ERR_DECODE_FAILED: i32 = -2; // Backward-compatible alias
pub const CODROP_ERR_UNSUPPORTED: i32 = -3;
pub const CODROP_ERR_LIMIT_EXCEEDED: i32 = -4;
pub const CODROP_ERR_CHECKSUM_MISMATCH: i32 = -5;
pub const CODROP_ERR_ENCODE_FAILED: i32 = -6;
pub const CODROP_ERR_PANIC: i32 = -99;

static VERSION_CSTR: &[u8] = b"1.0.0\0";

/// Returns a static null-terminated C string of the Codrop version.
#[no_mangle]
pub extern "C" fn codrop_version() -> *const c_char {
    VERSION_CSTR.as_ptr() as *const c_char
}

/// Compresses `src_len` bytes from `src` using the specified `level`.
///
/// # Level Values
/// - `0`: Fast (LZF)
/// - `1`: Balanced (LZH)
/// - `2`: Compact (LZA)
/// - `3`: Auto
///
/// On success, allocates an output buffer, stores the pointer in `*out_ptr`,
/// the size in `*out_len`, and returns `CODROP_OK (0)`.
/// The caller must free `*out_ptr` by calling [`codrop_free`].
///
/// # Safety
/// - `src` must point to at least `src_len` valid, readable bytes.
/// - `out_ptr` and `out_len` must be non-null, properly aligned pointers.
#[no_mangle]
pub unsafe extern "C" fn codrop_compress(
    src: *const u8,
    src_len: usize,
    level: u8,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    let result = catch_unwind(|| {
        if src.is_null() || out_ptr.is_null() || out_len.is_null() {
            return CODROP_ERR_NULL_PTR;
        }

        let input_slice = std::slice::from_raw_parts(src, src_len);
        let comp_level = match level {
            0 => CompressionLevel::Fast,
            1 => CompressionLevel::Balanced,
            2 => CompressionLevel::Compact,
            _ => CompressionLevel::Auto,
        };

        match compress(input_slice, comp_level) {
            Ok(compressed_bytes) => {
                let mut boxed = compressed_bytes.into_boxed_slice();
                *out_len = boxed.len();
                *out_ptr = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                CODROP_OK
            }
            Err(_) => CODROP_ERR_ENCODE_FAILED,
        }
    });

    result.unwrap_or(CODROP_ERR_PANIC)
}

/// Decompresses `src_len` bytes from `src`.
///
/// If `max_output_bytes` is 0, defaults to 1 GB safety limit.
/// On success, allocates an output buffer, stores the pointer in `*out_ptr`,
/// the size in `*out_len`, and returns `CODROP_OK (0)`.
/// The caller must free `*out_ptr` by calling [`codrop_free`].
///
/// # Safety
/// - `src` must point to at least `src_len` valid, readable bytes.
/// - `out_ptr` and `out_len` must be non-null, properly aligned pointers.
#[no_mangle]
pub unsafe extern "C" fn codrop_decompress(
    src: *const u8,
    src_len: usize,
    max_output_bytes: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    let result = catch_unwind(|| {
        if src.is_null() || out_ptr.is_null() || out_len.is_null() {
            return CODROP_ERR_NULL_PTR;
        }

        let input_slice = std::slice::from_raw_parts(src, src_len);
        let limit = if max_output_bytes == 0 {
            1024 * 1024 * 1024
        } else {
            max_output_bytes as u64
        };

        match decompress_with_limit(input_slice, limit) {
            Ok(decompressed_bytes) => {
                let mut boxed = decompressed_bytes.into_boxed_slice();
                *out_len = boxed.len();
                *out_ptr = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                CODROP_OK
            }
            Err(e) => match e {
                crate::CodropError::HeaderChecksumMismatch { .. }
                | crate::CodropError::BlockChecksumMismatch { .. }
                | crate::CodropError::StreamChecksumMismatch { .. } => CODROP_ERR_CHECKSUM_MISMATCH,
                crate::CodropError::DecompressionBombDetected { .. }
                | crate::CodropError::MemoryLimitExceeded { .. }
                | crate::CodropError::InvalidWindowSize(_) => CODROP_ERR_LIMIT_EXCEEDED,
                crate::CodropError::UnsupportedVersion { .. }
                | crate::CodropError::UnsupportedBlockType(_)
                | crate::CodropError::UnsupportedPrefilter(_)
                | crate::CodropError::UnknownDictionary { .. } => CODROP_ERR_UNSUPPORTED,
                _ => CODROP_ERR_CORRUPT,
            },
        }
    });

    result.unwrap_or(CODROP_ERR_PANIC)
}

/// Frees memory previously allocated by [`codrop_compress`] or [`codrop_decompress`].
///
/// # Safety
/// - `ptr` must have been returned by [`codrop_compress`] or [`codrop_decompress`].
/// - `len` must match the exact length stored in `*out_len` by the allocator function.
#[no_mangle]
pub unsafe extern "C" fn codrop_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        let _ = catch_unwind(|| {
            let slice_ptr = std::ptr::slice_from_raw_parts_mut(ptr, len);
            drop(Box::from_raw(slice_ptr));
        });
    }
}

/// Compresses an image buffer (PNG, JPEG, WebP, BMP) using perceptual visual compression.
///
/// # Format Values
/// - `0`: Auto (WebP for 75% - 90% savings)
/// - `1`: WebP
/// - `2`: PNG (Optimized)
/// - `3`: JPEG
///
/// # Quality Values
/// - `1..=100`: Quality factor (0 defaults to 85 for visually lossless compression)
///
/// On success, allocates an output buffer, stores the pointer in `*out_ptr`,
/// the size in `*out_len`, and returns `CODROP_OK (0)`.
/// The caller must free `*out_ptr` by calling [`codrop_free`].
///
/// # Safety
/// - `src` must point to at least `src_len` valid, readable bytes.
/// - `out_ptr` and `out_len` must be non-null, properly aligned pointers.
#[no_mangle]
pub unsafe extern "C" fn codrop_image_compress(
    src: *const u8,
    src_len: usize,
    format: u8,
    quality: u8,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    let result = catch_unwind(|| {
        if src.is_null() || out_ptr.is_null() || out_len.is_null() {
            return CODROP_ERR_NULL_PTR;
        }

        let input_slice = std::slice::from_raw_parts(src, src_len);
        let options = crate::image_codec::ImageOptions {
            format: crate::image_codec::CodropImageFormat::from_u8(format),
            quality: if quality == 0 { 85 } else { quality },
        };

        match crate::image_codec::compress_image(input_slice, options) {
            Ok(compressed_bytes) => {
                let mut boxed = compressed_bytes.into_boxed_slice();
                *out_len = boxed.len();
                *out_ptr = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                CODROP_OK
            }
            Err(_) => CODROP_ERR_ENCODE_FAILED,
        }
    });

    result.unwrap_or(CODROP_ERR_PANIC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn test_c_abi_version() {
        let v_ptr = codrop_version();
        assert!(!v_ptr.is_null());
        let cstr = unsafe { CStr::from_ptr(v_ptr) };
        assert_eq!(cstr.to_str().unwrap(), "1.0.0");
    }

    #[test]
    fn test_c_abi_compress_and_decompress_roundtrip() {
        let data = b"Testing C ABI roundtrip with Codrop compression!";
        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: usize = 0;

        let res_enc = unsafe {
            codrop_compress(
                data.as_ptr(),
                data.len(),
                1, // Balanced
                &mut out_ptr,
                &mut out_len,
            )
        };
        assert_eq!(res_enc, CODROP_OK);
        assert!(!out_ptr.is_null());
        assert!(out_len > 0);

        let mut dec_ptr: *mut u8 = std::ptr::null_mut();
        let mut dec_len: usize = 0;
        let res_dec = unsafe { codrop_decompress(out_ptr, out_len, 0, &mut dec_ptr, &mut dec_len) };
        assert_eq!(res_dec, CODROP_OK);
        assert!(!dec_ptr.is_null());
        assert_eq!(dec_len, data.len());

        let recovered = unsafe { std::slice::from_raw_parts(dec_ptr, dec_len) };
        assert_eq!(recovered, data);

        unsafe {
            codrop_free(out_ptr, out_len);
            codrop_free(dec_ptr, dec_len);
        }
    }

    #[test]
    fn test_c_abi_null_ptr_safeguard() {
        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: usize = 0;
        let res = unsafe { codrop_compress(std::ptr::null(), 10, 0, &mut out_ptr, &mut out_len) };
        assert_eq!(res, CODROP_ERR_NULL_PTR);
    }

    #[test]
    fn test_c_abi_malformed_returns_corrupt() {
        let corrupt_data = b"NOT_CDP_DATA";
        let mut dec_ptr: *mut u8 = std::ptr::null_mut();
        let mut dec_len: usize = 0;
        let res = unsafe { codrop_decompress(corrupt_data.as_ptr(), corrupt_data.len(), 0, &mut dec_ptr, &mut dec_len) };
        assert_eq!(res, CODROP_ERR_CORRUPT);
    }

    #[test]
    fn test_c_abi_limit_exceeded() {
        let data = b"Repeated data for decompression limit check!".repeat(10);
        let mut comp_ptr: *mut u8 = std::ptr::null_mut();
        let mut comp_len: usize = 0;
        let res_enc = unsafe { codrop_compress(data.as_ptr(), data.len(), 0, &mut comp_ptr, &mut comp_len) };
        assert_eq!(res_enc, CODROP_OK);

        // Decompress with limit of only 10 bytes
        let mut dec_ptr: *mut u8 = std::ptr::null_mut();
        let mut dec_len: usize = 0;
        let res_dec = unsafe { codrop_decompress(comp_ptr, comp_len, 10, &mut dec_ptr, &mut dec_len) };
        assert_eq!(res_dec, CODROP_ERR_LIMIT_EXCEEDED);

        unsafe { codrop_free(comp_ptr, comp_len); }
    }

    #[test]
    fn test_c_abi_image_compress() {
        use image::codecs::png::PngEncoder;
        use image::{Rgba, RgbaImage};

        let mut img = RgbaImage::new(8, 8);
        for p in img.pixels_mut() {
            *p = Rgba([0, 255, 0, 255]);
        }
        let mut png_bytes = Vec::new();
        img.write_with_encoder(PngEncoder::new(&mut png_bytes)).unwrap();

        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: usize = 0;

        let res = unsafe {
            codrop_image_compress(
                png_bytes.as_ptr(),
                png_bytes.len(),
                1, // WebP
                85,
                &mut out_ptr,
                &mut out_len,
            )
        };
        assert_eq!(res, CODROP_OK);
        assert!(!out_ptr.is_null());
        assert!(out_len > 0);

        unsafe {
            let slice = std::slice::from_raw_parts(out_ptr, out_len);
            assert_eq!(&slice[0..4], b"RIFF");
            assert_eq!(&slice[8..12], b"WEBP");
            codrop_free(out_ptr, out_len);
        }
    }
}


