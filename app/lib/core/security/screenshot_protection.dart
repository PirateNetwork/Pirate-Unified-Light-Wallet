// Screenshot and screen recording protection
//
// Prevents screenshots when viewing sensitive data like seed phrases

import 'dart:io';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Screenshot protection manager
class ScreenshotProtection {
  static const MethodChannel _channel = MethodChannel(
    'com.pirate.wallet/security',
  );

  static bool _isProtected = false;

  /// Enable screenshot protection
  static Future<void> enable() async {
    if (_isProtected) return;

    try {
      if (Platform.isAndroid ||
          Platform.isIOS ||
          Platform.isMacOS ||
          Platform.isWindows ||
          Platform.isLinux) {
        final enabled = await _channel.invokeMethod(
          'enableScreenshotProtection',
        );
        _isProtected = enabled == true;
      }
    } on PlatformException catch (e) {
      // Platform doesn't support or failed
      debugPrint('Screenshot protection failed: ${e.message}');
    } on MissingPluginException catch (_) {
      // Platform channel not implemented on this platform
    }
  }

  /// Disable screenshot protection
  static Future<void> disable() async {
    if (!_isProtected) return;

    try {
      if (Platform.isAndroid ||
          Platform.isIOS ||
          Platform.isMacOS ||
          Platform.isWindows ||
          Platform.isLinux) {
        await _channel.invokeMethod('disableScreenshotProtection');
        _isProtected = false;
      }
    } on PlatformException catch (e) {
      debugPrint('Failed to disable screenshot protection: ${e.message}');
    } on MissingPluginException catch (_) {
      // Platform channel not implemented on this platform
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
