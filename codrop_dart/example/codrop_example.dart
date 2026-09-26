// Copyright (c) 2026 Front Terrain Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

import 'dart:convert';
import 'package:codrop/codrop.dart';

void main() {
  print('=== Codrop Dart SDK Example ===');
  print('Engine Version: ${Codrop.version}');

  final sampleText = '''
Codrop is a modern, unified, general-purpose lossless compression format.
It delivers ultra-high-speed decompression and competitive ratios across cloud,
edge, mobile, and embedded platforms.
''' *
      10;

  final rawBytes = utf8.encode(sampleText);
  print('\nOriginal Size: ${rawBytes.length} bytes');

  // 1. Direct compression with different profiles
  for (final level in CodropLevel.values) {
    final compressed = Codrop.compress(rawBytes, level: level);
    final ratio =
        (compressed.length / rawBytes.length * 100).toStringAsFixed(1);
    print(
        '  [${level.name.toUpperCase().padRight(8)}] Compressed: ${compressed.length} bytes ($ratio%)');
  }

  // 2. Balanced compression and roundtrip verification
  final compressed = Codrop.compress(rawBytes, level: CodropLevel.balanced);
  final decompressed = Codrop.decompress(compressed);

  final recoveredText = utf8.decode(decompressed);
  final match = sampleText == recoveredText;
  print(
      '\nRoundtrip Verification: ${match ? "SUCCESS (Data matches byte-for-byte)" : "FAILED"}');

  // 3. Using Dart Standard Codec interface
  final codecEncoded = codrop.encode(rawBytes);
  final codecDecoded = codrop.decode(codecEncoded);
  print(
      'Dart Codec API test: ${codecDecoded.length == rawBytes.length ? "PASSED" : "FAILED"}');
}
