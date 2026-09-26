// Copyright (c) 2026 Front Terrain Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

import 'dart:convert';
import 'dart:typed_data';

import 'package:codrop/codrop.dart';
import 'package:test/test.dart';

void main() {
  group('Codrop Engine Tests', () {
    test('version reports release candidate', () {
      final ver = Codrop.version;
      expect(ver, isNotEmpty);
      expect(ver, contains('1.0.0'));
    });

    test('roundtrip compression and decompression on UTF-8 string', () {
      final input = utf8.encode(
        'Codrop Universal Adaptive Compression System for Dart & Flutter! ' *
            20,
      );
      final compressed = Codrop.compress(input);

      // Verify compression actually reduced size for redundant text
      expect(compressed.length, lessThan(input.length));

      final decompressed = Codrop.decompress(compressed);
      expect(decompressed, equals(input));
      expect(utf8.decode(decompressed), equals(utf8.decode(input)));
    });

    test('roundtrip across all compression levels', () {
      final text =
          'Lorem ipsum dolor sit amet, consectetur adipiscing elit. ' * 50;
      final input = utf8.encode(text);

      for (final level in CodropLevel.values) {
        final compressed = Codrop.compress(input, level: level);
        expect(compressed, isNotEmpty);
        final decompressed = Codrop.decompress(compressed);
        expect(decompressed, equals(input));
      }
    });

    test('handles empty input data', () {
      final empty = Uint8List(0);
      final compressed = Codrop.compress(empty);
      expect(compressed, isNotEmpty); // CDP header & metadata still exist
      final decompressed = Codrop.decompress(compressed);
      expect(decompressed, isEmpty);
    });

    test('binary pattern roundtrip', () {
      final binary = Uint8List(2048);
      for (var i = 0; i < binary.length; i++) {
        binary[i] = i % 256;
      }

      final compressed = Codrop.compress(binary, level: CodropLevel.compact);
      final decompressed = Codrop.decompress(compressed);
      expect(decompressed, equals(binary));
    });

    test('CodropCodec standard Dart converter interface', () {
      final data =
          utf8.encode('Testing Dart standard Codec interface with Codrop');
      final encoded = codrop.encode(data);
      final decoded = codrop.decode(encoded);
      expect(decoded, equals(data));
    });

    test('throws CodropException on corrupt data', () {
      final corrupted =
          Uint8List.fromList([0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02]);
      expect(
        () => Codrop.decompress(corrupted),
        throwsA(isA<CodropException>()),
      );
    });

    test('throws CodropException when max output limit is exceeded', () {
      final largeInput = Uint8List(1024 * 100); // 100 KB
      final compressed = Codrop.compress(largeInput);

      // Try decompressing with limit set to only 10 bytes
      expect(
        () => Codrop.decompress(compressed, maxOutputBytes: 10),
        throwsA(isA<CodropException>()),
      );
    });
  });
}
