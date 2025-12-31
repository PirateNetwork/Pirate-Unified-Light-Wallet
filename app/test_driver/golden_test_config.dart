import 'dart:io';
import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';

/// Configuration for golden tests
class GoldenTestConfig {
  static void configure() {
    if (Platform.isMacOS) {
      // Use higher tolerance on macOS due to font rendering differences
      goldenFileComparator = _MacOSGoldenFileComparator();
    } else if (Platform.isLinux) {
      goldenFileComparator = _LinuxGoldenFileComparator();
    }
  }
}

class _MacOSGoldenFileComparator extends LocalFileComparator {
  _MacOSGoldenFileComparator() : super(Uri.parse('test'));

  @override
  Future<bool> compare(Uint8List imageBytes, Uri golden) async {
    final result = await GoldenFileComparator.compareLists(
      imageBytes,
      await getGoldenBytes(golden),
    );

    // Allow 2% difference for font rendering
    if (!result.passed && result.diffPercent < 2.0) {
      return true;
    }

    return result.passed;
  }
}

class _LinuxGoldenFileComparator extends LocalFileComparator {
  _LinuxGoldenFileComparator() : super(Uri.parse('test'));

  @override
  Future<bool> compare(Uint8List imageBytes, Uri golden) async {
    final result = await GoldenFileComparator.compareLists(
      imageBytes,
      await getGoldenBytes(golden),
    );

    // Allow 1% difference for Linux
    if (!result.passed && result.diffPercent < 1.0) {
      return true;
    }

    return result.passed;
  }
}

