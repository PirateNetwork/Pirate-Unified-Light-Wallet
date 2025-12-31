/// Screenshot and screen recording protection
///
/// Prevents screenshots when viewing sensitive data like seed phrases

import 'dart:io';
import 'package:flutter/services.dart';

/// Screenshot protection manager
class ScreenshotProtection {
  static const MethodChannel _channel =
      MethodChannel('com.pirate.wallet/security');

  static bool _isProtected = false;

  /// Enable screenshot protection
  static Future<void> enable() async {
    if (_isProtected) return;

    try {
      if (Platform.isAndroid || Platform.isIOS) {
        await _channel.invokeMethod('enableScreenshotProtection');
        _isProtected = true;
      }
    } on PlatformException catch (e) {
      // Platform doesn't support or failed
      print('Screenshot protection failed: ${e.message}');
    }
  }

  /// Disable screenshot protection
  static Future<void> disable() async {
    if (!_isProtected) return;

    try {
      if (Platform.isAndroid || Platform.isIOS) {
        await _channel.invokeMethod('disableScreenshotProtection');
        _isProtected = false;
      }
    } on PlatformException catch (e) {
      print('Failed to disable screenshot protection: ${e.message}');
    }
  }

  /// Check if currently protected
  static bool get isProtected => _isProtected;

  /// Protect a screen temporarily (auto-disable on dispose)
  static ScreenProtection protect() {
    return ScreenProtection._();
  }
}

/// Screen protection lifecycle manager
class ScreenProtection {
  ScreenProtection._() {
    ScreenshotProtection.enable();
  }

  /// Dispose and remove protection
  void dispose() {
    ScreenshotProtection.disable();
  }
}

/// Widget mixin for automatic screenshot protection
mixin ScreenshotProtectionMixin {
  ScreenProtection? _protection;

  /// Enable protection for this screen
  void enableScreenshotProtection() {
    _protection = ScreenshotProtection.protect();
  }

  /// Disable protection
  void disableScreenshotProtection() {
    _protection?.dispose();
    _protection = null;
  }

  /// Call in dispose
  void disposeScreenshotProtection() {
    _protection?.dispose();
  }
}

