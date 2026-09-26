// Copyright (c) 2026 Front Terrain Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

/// High-performance universal adaptive compression system for Dart and Flutter.
///
/// Codrop provides fast, memory-safe, lossless compression powered by modern
/// LZ-family match finding and asymmetric numeral systems (ANS) entropy coding.
///
/// ## Quick Start
///
/// ```dart
/// import 'dart:convert';
/// import 'package:codrop/codrop.dart';
///
/// void main() {
///   final original = utf8.encode('Hello, Codrop adaptive compression!');
///
///   // Fast compression
///   final compressed = Codrop.compress(original, level: CodropLevel.balanced);
///
///   // Decompression
///   final decompressed = Codrop.decompress(compressed);
///   print(utf8.decode(decompressed));
/// }
/// ```
library;

export 'src/codrop_base.dart';
export 'src/codrop_bindings.dart' show CodropBindings;
export 'src/codrop_loader.dart' show CodropLoader;
