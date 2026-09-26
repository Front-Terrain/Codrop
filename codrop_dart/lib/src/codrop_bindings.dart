// Copyright (c) 2026 Front Terrain Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

import 'dart:ffi';
import 'package:ffi/ffi.dart';

// Native function typedefs
typedef _CodropVersionC = Pointer<Utf8> Function();
typedef _CodropVersionDart = Pointer<Utf8> Function();

typedef _CodropCompressC = Int32 Function(
  Pointer<Uint8> src,
  Size srcLen,
  Uint8 level,
  Pointer<Pointer<Uint8>> outPtr,
  Pointer<Size> outLen,
);
typedef _CodropCompressDart = int Function(
  Pointer<Uint8> src,
  int srcLen,
  int level,
  Pointer<Pointer<Uint8>> outPtr,
  Pointer<Size> outLen,
);

typedef _CodropDecompressC = Int32 Function(
  Pointer<Uint8> src,
  Size srcLen,
  Size maxOutputBytes,
  Pointer<Pointer<Uint8>> outPtr,
  Pointer<Size> outLen,
);
typedef _CodropDecompressDart = int Function(
  Pointer<Uint8> src,
  int srcLen,
  int maxOutputBytes,
  Pointer<Pointer<Uint8>> outPtr,
  Pointer<Size> outLen,
);

typedef _CodropFreeC = Void Function(Pointer<Uint8> ptr, Size len);
typedef _CodropFreeDart = void Function(Pointer<Uint8> ptr, int len);

/// Direct FFI bindings for `libcodrop`.
class CodropBindings {
  CodropBindings(DynamicLibrary dylib)
      : _version = dylib
            .lookup<NativeFunction<_CodropVersionC>>('codrop_version')
            .asFunction<_CodropVersionDart>(),
        _compress = dylib
            .lookup<NativeFunction<_CodropCompressC>>('codrop_compress')
            .asFunction<_CodropCompressDart>(),
        _decompress = dylib
            .lookup<NativeFunction<_CodropDecompressC>>('codrop_decompress')
            .asFunction<_CodropDecompressDart>(),
        _free = dylib
            .lookup<NativeFunction<_CodropFreeC>>('codrop_free')
            .asFunction<_CodropFreeDart>();

  final _CodropVersionDart _version;
  final _CodropCompressDart _compress;
  final _CodropDecompressDart _decompress;
  final _CodropFreeDart _free;

  /// Returns the null-terminated version string of Codrop.
  String getVersion() {
    final ptr = _version();
    if (ptr == nullptr) return 'unknown';
    return ptr.toDartString();
  }

  /// Compresses input bytes into an allocated buffer.
  int compress(
    Pointer<Uint8> src,
    int srcLen,
    int level,
    Pointer<Pointer<Uint8>> outPtr,
    Pointer<Size> outLen,
  ) {
    return _compress(src, srcLen, level, outPtr, outLen);
  }

  /// Decompresses .cdp bytes into an allocated buffer.
  int decompress(
    Pointer<Uint8> src,
    int srcLen,
    int maxOutputBytes,
    Pointer<Pointer<Uint8>> outPtr,
    Pointer<Size> outLen,
  ) {
    return _decompress(src, srcLen, maxOutputBytes, outPtr, outLen);
  }

  /// Frees an allocated buffer.
  void free(Pointer<Uint8> ptr, int len) {
    _free(ptr, len);
  }
}
