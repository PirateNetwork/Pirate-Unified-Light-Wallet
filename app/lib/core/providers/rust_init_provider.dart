import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../ffi/generated/frb_generated.dart';

final rustInitProvider = FutureProvider<void>((ref) async {
  if (Platform.environment.containsKey('FLUTTER_TEST')) {
    return;
  }
  try {
    debugPrint('Initializing Rust library...');
    await RustLib.init().timeout(
      const Duration(seconds: 10),
      onTimeout: () {
        throw TimeoutException(
          'Rust library initialization timed out after 10 seconds',
        );
      },
    );
    debugPrint('Rust library initialized successfully');
  } catch (e, stackTrace) {
    debugPrint('Failed to initialize Rust library: $e');
    debugPrint('Stack trace: $stackTrace');
    rethrow;
  }
});
