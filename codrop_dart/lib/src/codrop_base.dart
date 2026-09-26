// Copyright (c) 2026 Front Terrain Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

import 'dart:convert';
import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'codrop_loader.dart';

/// Compression profile levels supported by the Codrop engine.
enum CodropLevel {
  /// Automatic profile selection based on entropy and heuristic analysis.
  auto(0),

  /// High-speed compression prioritizing maximum throughput.
  fast(1),

  /// Balanced mode optimizing speed and compression ratio.
  balanced(2),

  /// Compact mode providing maximum compression ratio.
  compact(3);

  const CodropLevel(this.value);

  /// The integer value representing this level in the C ABI.
  final int value;
}

/// Target image formats for Codrop perceptual visual compression.
enum CodropImageFormat {
  /// Automatic format selection (defaults to WebP for 75-90% savings).
  auto(0),

  /// Modern WebP format (ultra-compact, visually lossless).
  webp(1),

  /// Optimized PNG format.
  png(2),

  /// JPEG format with configurable quality.
  jpeg(3);

  const CodropImageFormat(this.value);

  /// The integer value representing this format in the C ABI.
  final int value;
}

/// Exception thrown when a Codrop operation fails.
class CodropException implements Exception {
  /// Creates a new [CodropException] with an error code and description.
  const CodropException(this.errorCode, this.message);

  /// Error code returned by the Codrop C engine.
  final int errorCode;

  /// Human-readable explanation of the error.
  final String message;

  /// Creates a [CodropException] mapped from a C ABI error code.
  factory CodropException.fromErrorCode(int code, [String? context]) {
    final prefix = context != null ? '$context: ' : '';
    switch (code) {
      case -1:
        return CodropException(code,
            '${prefix}Invalid parameter or null pointer passed to engine');
      case -2:
        return CodropException(
            code, '${prefix}Data stream is corrupt or malformed');
      case -3:
        return CodropException(code,
            '${prefix}Unsupported stream version, block type, or feature');
      case -4:
        return CodropException(code,
            '${prefix}Decompression safety limit or window size exceeded');
      case -5:
        return CodropException(
            code, '${prefix}Checksum verification failed (CRC32c/xxHash3)');
      case -6:
        return CodropException(code, '${prefix}Compression encoding failed');
      case -99:
        return CodropException(
            code, '${prefix}Internal engine panic or unhandled error');
      default:
        return CodropException(code, '${prefix}Unknown error (code $code)');
    }
  }

  @override
  String toString() => 'CodropException(code: $errorCode): $message';
}

/// Core API for the Codrop universal adaptive compression system.
class Codrop {
  Codrop._();

  /// Explicitly initialize the Codrop native library.
  ///
  /// Useful when running within Flutter apps on mobile or desktop where the native
  /// library is bundled in a custom asset or framework directory.
  static void init({String? libraryPath, DynamicLibrary? dynamicLibrary}) {
    CodropLoader.initialize(
      libraryPath: libraryPath,
      dynamicLibrary: dynamicLibrary,
    );
  }

  /// Whether the Codrop native engine is available on the current platform.
  static bool get isSupported => CodropLoader.isSupported;

  /// Returns the Codrop engine version string (e.g. `1.0.0`).
  static String get version => CodropLoader.bindings.getVersion();

  /// Compresses input [data] using the Codrop compression algorithm.
  ///
  /// [level] controls the compression trade-off:
  /// - [CodropLevel.auto] (0): Heuristic entropy profiler selects optimal block modes.
  /// - [CodropLevel.fast] (1): High-speed LZ match finding.
  /// - [CodropLevel.balanced] (2): Default balance between speed and ratio.
  /// - [CodropLevel.compact] (3): Maximum density search.
  ///
  /// Returns compressed bytes in `.cdp` format.
  /// Throws [CodropException] if compression fails.
  static Uint8List compress(
    List<int> data, {
    CodropLevel level = CodropLevel.balanced,
  }) {
    final bindings = CodropLoader.bindings;
    final srcLen = data.length;

    final Pointer<Uint8> srcPtr = malloc<Uint8>(srcLen > 0 ? srcLen : 1);
    if (data is Uint8List) {
      srcPtr.asTypedList(srcLen).setAll(0, data);
    } else {
      final typedList = srcPtr.asTypedList(srcLen);
      for (var i = 0; i < srcLen; i++) {
        typedList[i] = data[i];
      }
    }

    final outPtrPtr = malloc<Pointer<Uint8>>();
    final outLenPtr = malloc<Size>();
    outPtrPtr.value = nullptr;
    outLenPtr.value = 0;

    try {
      final res = bindings.compress(
        srcPtr,
        srcLen,
        level.value,
        outPtrPtr,
        outLenPtr,
      );

      if (res != 0) {
        throw CodropException.fromErrorCode(res, 'Compression failed');
      }

      final outPtr = outPtrPtr.value;
      final outLen = outLenPtr.value;

      if (outPtr == nullptr) {
        throw const CodropException(
          -99,
          'Null output pointer returned from native compression',
        );
      }

      try {
        return Uint8List.fromList(outPtr.asTypedList(outLen));
      } finally {
        bindings.free(outPtr, outLen);
      }
    } finally {
      malloc.free(srcPtr);
      malloc.free(outPtrPtr);
      malloc.free(outLenPtr);
    }
  }

  /// Compresses an image using perceptual visual compression (WebP, PNG, JPEG).
  ///
  /// [format] controls target output format (defaults to [CodropImageFormat.auto] / WebP).
  /// [quality] is a factor from 1 to 100 (defaults to 85 for visually lossless 75-90% savings).
  ///
  /// Returns the compressed image bytes.
  /// Throws [CodropException] if image compression fails.
  static Uint8List compressImage(
    List<int> data, {
    CodropImageFormat format = CodropImageFormat.auto,
    int quality = 85,
  }) {
    final bindings = CodropLoader.bindings;
    final srcLen = data.length;

    final Pointer<Uint8> srcPtr = malloc<Uint8>(srcLen > 0 ? srcLen : 1);
    if (data is Uint8List) {
      srcPtr.asTypedList(srcLen).setAll(0, data);
    } else {
      final typedList = srcPtr.asTypedList(srcLen);
      for (var i = 0; i < srcLen; i++) {
        typedList[i] = data[i];
      }
    }

    final Pointer<Pointer<Uint8>> outPtrPtr = malloc<Pointer<Uint8>>();
    final Pointer<Size> outLenPtr = malloc<Size>();

    try {
      final res = bindings.compressImage(
        srcPtr,
        srcLen,
        format.value,
        quality.clamp(1, 100),
        outPtrPtr,
        outLenPtr,
      );

      if (res != 0) {
        throw CodropException.fromErrorCode(res, 'Image compression failed');
      }

      final outPtr = outPtrPtr.value;
      final outLen = outLenPtr.value;

      if (outPtr == nullptr) {
        throw const CodropException(
          -99,
          'Null output pointer returned from native image compression',
        );
      }

      try {
        return Uint8List.fromList(outPtr.asTypedList(outLen));
      } finally {
        bindings.free(outPtr, outLen);
      }
    } finally {
      malloc.free(srcPtr);
      malloc.free(outPtrPtr);
      malloc.free(outLenPtr);
    }
  }

  /// Decompresses a `.cdp` stream back into raw uncompressed bytes.
  ///
  /// [maxOutputBytes] provides a safety limit against decompression bombs (defaults to 1 GB).
  ///
  /// Returns the uncompressed byte buffer.
  /// Throws [CodropException] if decompression fails or limits are exceeded.
  static Uint8List decompress(
    List<int> data, {
    int maxOutputBytes = 1024 * 1024 * 1024,
  }) {
    final bindings = CodropLoader.bindings;
    final srcLen = data.length;

    final Pointer<Uint8> srcPtr = malloc<Uint8>(srcLen > 0 ? srcLen : 1);
    if (data is Uint8List) {
      srcPtr.asTypedList(srcLen).setAll(0, data);
    } else {
      final typedList = srcPtr.asTypedList(srcLen);
      for (var i = 0; i < srcLen; i++) {
        typedList[i] = data[i];
      }
    }

    final outPtrPtr = malloc<Pointer<Uint8>>();
    final outLenPtr = malloc<Size>();
    outPtrPtr.value = nullptr;
    outLenPtr.value = 0;

    try {
      final res = bindings.decompress(
        srcPtr,
        srcLen,
        maxOutputBytes,
        outPtrPtr,
        outLenPtr,
      );

      if (res != 0) {
        throw CodropException.fromErrorCode(res, 'Decompression failed');
      }

      final outPtr = outPtrPtr.value;
      final outLen = outLenPtr.value;

      if (outPtr == nullptr) {
        throw const CodropException(
          -99,
          'Null output pointer returned from native decompression',
        );
      }

      try {
        return Uint8List.fromList(outPtr.asTypedList(outLen));
      } finally {
        bindings.free(outPtr, outLen);
      }
    } finally {
      malloc.free(srcPtr);
      malloc.free(outPtrPtr);
      malloc.free(outLenPtr);
    }
  }
}

/// An encoder that compresses byte data using Codrop.
class CodropEncoder extends Converter<List<int>, Uint8List> {
  /// Creates a new [CodropEncoder] with the specified compression level.
  const CodropEncoder({this.level = CodropLevel.balanced});

  /// The compression profile level used for encoding.
  final CodropLevel level;

  @override
  Uint8List convert(List<int> input) => Codrop.compress(input, level: level);
}

/// A decoder that decompresses Codrop `.cdp` streams.
class CodropDecoder extends Converter<List<int>, Uint8List> {
  /// Creates a new [CodropDecoder] with the specified maximum output size.
  const CodropDecoder({this.maxOutputBytes = 1024 * 1024 * 1024});

  /// Maximum allowed decompressed byte count (defaults to 1 GB).
  final int maxOutputBytes;

  @override
  Uint8List convert(List<int> input) =>
      Codrop.decompress(input, maxOutputBytes: maxOutputBytes);
}

/// A standard Dart `Codec` for Codrop compression and decompression.
class CodropCodec extends Codec<List<int>, List<int>> {
  /// Creates a new [CodropCodec].
  const CodropCodec({
    this.level = CodropLevel.balanced,
    this.maxOutputBytes = 1024 * 1024 * 1024,
  });

  /// The compression profile level used when encoding.
  final CodropLevel level;

  /// The maximum allowed decompressed bytes when decoding.
  final int maxOutputBytes;

  @override
  Converter<List<int>, Uint8List> get encoder => CodropEncoder(level: level);

  @override
  Converter<List<int>, Uint8List> get decoder =>
      CodropDecoder(maxOutputBytes: maxOutputBytes);
}

/// Default global instance of [CodropCodec].
const CodropCodec codrop = CodropCodec();
