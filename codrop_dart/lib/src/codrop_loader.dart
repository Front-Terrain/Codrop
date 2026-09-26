// Copyright (c) 2026 Front Terrain Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

import 'dart:ffi';
import 'dart:io';

import 'codrop_bindings.dart';

/// Manages loading and caching the `libcodrop` dynamic library.
class CodropLoader {
  CodropLoader._();

  static CodropBindings? _bindings;

  /// Returns the active `CodropBindings` instance, resolving it if necessary.
  static CodropBindings get bindings {
    _bindings ??= CodropBindings(loadDynamicLibrary());
    return _bindings!;
  }

  /// Explicitly initialize Codrop with a specific library path or loaded `DynamicLibrary`.
  static void initialize(
      {String? libraryPath, DynamicLibrary? dynamicLibrary}) {
    if (dynamicLibrary != null) {
      _bindings = CodropBindings(dynamicLibrary);
      return;
    }
    if (libraryPath != null) {
      _bindings = CodropBindings(DynamicLibrary.open(libraryPath));
      return;
    }
    _bindings = CodropBindings(loadDynamicLibrary());
  }

  /// Whether the native library is loaded or available on this platform.
  static bool get isSupported {
    try {
      final b = bindings;
      return b.getVersion().isNotEmpty;
    } catch (_) {
      return false;
    }
  }

  /// Discovers and opens the platform-appropriate dynamic library for Codrop.
  static DynamicLibrary loadDynamicLibrary([String? customPath]) {
    if (customPath != null && customPath.isNotEmpty) {
      return DynamicLibrary.open(customPath);
    }

    if (Platform.isWindows) {
      return _loadWindows();
    } else if (Platform.isMacOS) {
      return _loadMacOS();
    } else if (Platform.isLinux || Platform.isAndroid) {
      return _loadLinuxOrAndroid();
    } else if (Platform.isIOS) {
      return DynamicLibrary.process();
    }

    throw UnsupportedError(
      'Codrop native library is not supported on ${Platform.operatingSystem}.',
    );
  }

  static DynamicLibrary _loadWindows() {
    const dllNames = ['libcodrop.dll', 'codrop.dll'];
    for (final name in dllNames) {
      try {
        return DynamicLibrary.open(name);
      } catch (_) {}
    }

    final candidates = [
      'libcodrop.dll',
      'target/release/libcodrop.dll',
      'target/debug/libcodrop.dll',
      '../target/release/libcodrop.dll',
      '../target/debug/libcodrop.dll',
      '../../target/release/libcodrop.dll',
      '../../target/debug/libcodrop.dll',
    ];

    for (final path in candidates) {
      if (File(path).existsSync()) {
        try {
          return DynamicLibrary.open(File(path).absolute.path);
        } catch (_) {}
      }
    }

    try {
      return DynamicLibrary.process();
    } catch (_) {}

    throw StateError(
      'Could not locate libcodrop.dll on Windows. Ensure libcodrop is compiled '
      'via "cargo build --release" or provide the path using Codrop.init(libraryPath: ...).',
    );
  }

  static DynamicLibrary _loadMacOS() {
    const names = ['libcodrop.dylib', 'codrop.dylib'];
    for (final name in names) {
      try {
        return DynamicLibrary.open(name);
      } catch (_) {}
    }

    try {
      return DynamicLibrary.process();
    } catch (_) {}

    final candidates = [
      'libcodrop.dylib',
      'target/release/libcodrop.dylib',
      'target/debug/libcodrop.dylib',
      '../target/release/libcodrop.dylib',
      '../../target/release/libcodrop.dylib',
    ];

    for (final path in candidates) {
      if (File(path).existsSync()) {
        try {
          return DynamicLibrary.open(File(path).absolute.path);
        } catch (_) {}
      }
    }

    throw StateError(
      'Could not locate libcodrop.dylib on macOS. Ensure libcodrop is compiled '
      'or provide the path using Codrop.init(libraryPath: ...).',
    );
  }

  static DynamicLibrary _loadLinuxOrAndroid() {
    const names = ['libcodrop.so', 'codrop.so'];
    for (final name in names) {
      try {
        return DynamicLibrary.open(name);
      } catch (_) {}
    }

    try {
      return DynamicLibrary.process();
    } catch (_) {}

    final candidates = [
      'libcodrop.so',
      'target/release/libcodrop.so',
      'target/debug/libcodrop.so',
      '../target/release/libcodrop.so',
      '../../target/release/libcodrop.so',
    ];

    for (final path in candidates) {
      if (File(path).existsSync()) {
        try {
          return DynamicLibrary.open(File(path).absolute.path);
        } catch (_) {}
      }
    }

    throw StateError(
      'Could not locate libcodrop.so. Ensure libcodrop is compiled '
      'or provide the path using Codrop.init(libraryPath: ...).',
    );
  }
}
