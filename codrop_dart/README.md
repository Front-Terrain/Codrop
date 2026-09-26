# Codrop for Dart & Flutter

[![Pub Version](https://img.shields.io/pub/v/codrop)](https://pub.dev/packages/codrop)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Platform](https://img.shields.io/badge/platform-flutter%20%7C%20dart%20vm-blue)](https://pub.dev/packages/codrop)

High-performance, memory-safe Dart and Flutter bindings for the **Codrop Universal Adaptive Compression System**.

Codrop is a modern, unified, general-purpose lossless compression format designed to deliver fast decompression and competitive compression ratios across cloud servers, desktops, and mobile devices.

---

## Features

- ⚡ **Ultra-Fast Decompression:** High-throughput decoding powered by modern LZ match finding and finite-state entropy.
- 🎯 **Adaptive Profiling:** Heuristic entropy analysis automatically dispatches RAW, RLE, and LZ block encoders.
- 🛡️ **Memory-Safe C FFI:** Clean C ABI bindings with explicit memory guards against decompression bombs and corrupt streams.
- 🔄 **Standard Dart `Codec`:** Fully implements Dart's standard `Codec<List<int>, List<int>>` and `Converter` interfaces.
- 📱 **Flutter & Desktop Ready:** Works seamlessly on Windows, macOS, Linux, Android, and iOS.

---

## Installation

Add `codrop` to your `pubspec.yaml`:

```yaml
dependencies:
  codrop: ^1.0.0
```

Or run:

```bash
dart pub add codrop
# For Flutter:
flutter pub add codrop
```

---

## Quick Start

### Basic Compression & Decompression

```dart
import 'dart:convert';
import 'package:codrop/codrop.dart';

void main() {
  final input = utf8.encode('Hello, Codrop adaptive compression for Dart & Flutter!');

  // Compress data (defaults to balanced profile)
  final compressed = Codrop.compress(input);

  // Decompress back to original bytes
  final decompressed = Codrop.decompress(compressed);

  print(utf8.decode(decompressed));
}
```

### Compression Profiles

Codrop provides four compression profile levels:

```dart
// Auto: Adaptive entropy heuristic selection
final autoBytes = Codrop.compress(data, level: CodropLevel.auto);

// Fast: Optimized for maximum encoding throughput
final fastBytes = Codrop.compress(data, level: CodropLevel.fast);

// Balanced: Recommended default balancing speed and ratio
final balancedBytes = Codrop.compress(data, level: CodropLevel.balanced);

// Compact: Maximum compression density
final compactBytes = Codrop.compress(data, level: CodropLevel.compact);
```

### Standard Dart Codec

Codrop provides standard Dart `Codec` and `Converter` integrations:

```dart
import 'dart:convert';
import 'package:codrop/codrop.dart';

void main() {
  final data = utf8.encode('Streaming or pipeline compression');

  // Using global codrop instance
  final encoded = codrop.encode(data);
  final decoded = codrop.decode(encoded);

  // Custom codec with specific level
  const fastCodec = CodropCodec(level: CodropLevel.fast);
  final fastEncoded = fastCodec.encode(data);
}
```

### Safety & Decompression Limits

To protect applications from decompression bomb attacks, `Codrop.decompress` accepts a `maxOutputBytes` threshold (default is 1 GB):

```dart
try {
  // Enforce a strict 50 MB decompression limit
  final decompressed = Codrop.decompress(
    compressedData,
    maxOutputBytes: 50 * 1024 * 1024,
  );
} on CodropException catch (e) {
  print('Codrop error (${e.errorCode}): ${e.message}');
}
```

---

## Native Library Setup

`codrop` uses Dart FFI to bind with `libcodrop`.

### Automatic Discovery
In standalone Dart or test environments, the package automatically looks for:
- **Windows:** `libcodrop.dll` or `codrop.dll`
- **macOS:** `libcodrop.dylib`
- **Linux:** `libcodrop.so`

### Custom Path or Mobile Bundling
If your dynamic library is located in a custom path (e.g., Flutter asset bundle or Android `jniLibs`):

```dart
import 'package:codrop/codrop.dart';

void main() {
  // Explicitly initialize with custom library path
  Codrop.init(libraryPath: '/path/to/libcodrop.so');
}
```

---

## License

This package is licensed under the [MIT License](LICENSE).
