"""
Codrop Python Bindings (v1.0.0-rc1)
Universal Adaptive Compression System
"""

import ctypes
import os
import sys
from pathlib import Path

# Locate dynamic library
def _find_lib():
    names = ["libcodrop.dll", "codrop.dll"] if sys.platform == "win32" else (["libcodrop.dylib"] if sys.platform == "darwin" else ["libcodrop.so"])
    
    # Check package directory first
    pkg_dir = Path(__file__).resolve().parent
    for dll_name in names:
        candidates = [
            pkg_dir / dll_name,
            pkg_dir.parent.parent / "target" / "release" / dll_name,
            pkg_dir.parent.parent / "target" / "debug" / dll_name,
        ]
        for c in candidates:
            if c.exists():
                return ctypes.CDLL(str(c))
        try:
            return ctypes.CDLL(dll_name)
        except OSError:
            pass
    return None
    
    # Try system loader
    try:
        return ctypes.CDLL(dll_name)
    except OSError:
        return None

_lib = _find_lib()

if _lib is not None:
    _lib.codrop_version.restype = ctypes.c_char_p
    _lib.codrop_compress.argtypes = [
        ctypes.POINTER(ctypes.c_uint8),
        ctypes.c_size_t,
        ctypes.c_uint8,
        ctypes.POINTER(ctypes.POINTER(ctypes.c_uint8)),
        ctypes.POINTER(ctypes.c_size_t),
    ]
    _lib.codrop_compress.restype = ctypes.c_int32

    _lib.codrop_decompress.argtypes = [
        ctypes.POINTER(ctypes.c_uint8),
        ctypes.c_size_t,
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.POINTER(ctypes.c_uint8)),
        ctypes.POINTER(ctypes.c_size_t),
    ]
    _lib.codrop_decompress.restype = ctypes.c_int32

    _lib.codrop_free.argtypes = [ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t]
    _lib.codrop_free.restype = None

LEVEL_AUTO = 0
LEVEL_FAST = 1
LEVEL_BALANCED = 2
LEVEL_COMPACT = 3

class CodropError(Exception):
    pass

def version() -> str:
    if _lib is None:
        return "1.0.0-rc1"
    return _lib.codrop_version().decode("utf-8")

def compress(data: bytes, level: int = LEVEL_BALANCED) -> bytes:
    if _lib is None:
        raise CodropError("Codrop native library not found. Please compile libcodrop with 'cargo build --release'.")
    if not isinstance(data, (bytes, bytearray)):
        raise TypeError("Input data must be bytes or bytearray")
    
    src_len = len(data)
    src_arr = (ctypes.c_uint8 * src_len).from_buffer_copy(data) if src_len > 0 else (ctypes.c_uint8 * 1)()
    src_ptr = ctypes.cast(src_arr, ctypes.POINTER(ctypes.c_uint8))

    out_ptr = ctypes.POINTER(ctypes.c_uint8)()
    out_len = ctypes.c_size_t(0)

    res = _lib.codrop_compress(src_ptr, src_len, level, ctypes.byref(out_ptr), ctypes.byref(out_len))
    if res != 0:
        raise CodropError(f"Compression failed with error code: {res}")

    try:
        compressed_bytes = bytes(ctypes.string_at(out_ptr, out_len.value))
    finally:
        _lib.codrop_free(out_ptr, out_len.value)

    return compressed_bytes

def decompress(data: bytes, max_output_bytes: int = 1024 * 1024 * 1024) -> bytes:
    if _lib is None:
        raise CodropError("Codrop native library not found. Please compile libcodrop with 'cargo build --release'.")
    if not isinstance(data, (bytes, bytearray)):
        raise TypeError("Input data must be bytes or bytearray")

    src_len = len(data)
    src_arr = (ctypes.c_uint8 * src_len).from_buffer_copy(data) if src_len > 0 else (ctypes.c_uint8 * 1)()
    src_ptr = ctypes.cast(src_arr, ctypes.POINTER(ctypes.c_uint8))

    out_ptr = ctypes.POINTER(ctypes.c_uint8)()
    out_len = ctypes.c_size_t(0)

    res = _lib.codrop_decompress(src_ptr, src_len, max_output_bytes, ctypes.byref(out_ptr), ctypes.byref(out_len))
    if res != 0:
        raise CodropError(f"Decompression failed with error code: {res}")

    try:
        decompressed_bytes = bytes(ctypes.string_at(out_ptr, out_len.value))
    finally:
        _lib.codrop_free(out_ptr, out_len.value)

    return decompressed_bytes
