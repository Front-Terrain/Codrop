/**
 * Codrop Universal Adaptive Compression System
 * C ABI Header (v1.0.0-rc1)
 *
 * Copyright (c) Front Terrain Inc.
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#ifndef CODROP_H
#define CODROP_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Return error codes */
#define CODROP_OK 0
#define CODROP_ERROR_INVALID_PARAM -1
#define CODROP_ERROR_CORRUPT -2
#define CODROP_ERROR_UNSUPPORTED -3
#define CODROP_ERROR_LIMIT_EXCEEDED -4
#define CODROP_ERROR_INTERNAL -99

/* Compression profiles */
#define CODROP_LEVEL_AUTO 0
#define CODROP_LEVEL_FAST 1
#define CODROP_LEVEL_BALANCED 2
#define CODROP_LEVEL_COMPACT 3

/**
 * Returns the null-terminated version string of Codrop (e.g. "1.0.0-rc1").
 */
const char* codrop_version(void);

/**
 * Compresses an input buffer into a newly allocated output buffer.
 *
 * @param src Pointer to input data
 * @param src_len Length of input data in bytes
 * @param level Compression profile (0=Auto, 1=Fast, 2=Balanced, 3=Compact)
 * @param out_ptr Pointer to receive pointer to allocated output buffer
 * @param out_len Pointer to receive length of allocated output buffer
 * @return CODROP_OK on success, or negative error code on failure.
 *
 * The buffer written to *out_ptr must be freed using codrop_free().
 */
int32_t codrop_compress(
    const uint8_t* src,
    size_t src_len,
    uint8_t level,
    uint8_t** out_ptr,
    size_t* out_len
);

/**
 * Decompresses a .cdp buffer into a newly allocated output buffer.
 *
 * @param src Pointer to compressed .cdp data
 * @param src_len Length of compressed data in bytes
 * @param max_output_bytes Maximum allowed uncompressed bytes (safety limit, 0 = default 1GB)
 * @param out_ptr Pointer to receive pointer to allocated output buffer
 * @param out_len Pointer to receive length of allocated output buffer
 * @return CODROP_OK on success, or negative error code on failure.
 *
 * The buffer written to *out_ptr must be freed using codrop_free().
 */
int32_t codrop_decompress(
    const uint8_t* src,
    size_t src_len,
    size_t max_output_bytes,
    uint8_t** out_ptr,
    size_t* out_len
);

/**
 * Frees a buffer allocated by codrop_compress or codrop_decompress.
 *
 * @param ptr Pointer to buffer previously allocated by Codrop
 * @param len Length returned in out_len
 */
void codrop_free(uint8_t* ptr, size_t len);

#ifdef __cplusplus
}
#endif

#endif /* CODROP_H */
