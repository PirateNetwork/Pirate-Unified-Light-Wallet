import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter_acrylic/flutter_acrylic.dart';
import 'package:window_manager/window_manager.dart';

/// Desktop window effects manager
class WindowEffects {
  WindowEffects._();

  static Future<void> initialize() async {
    if (!_isDesktop) return;

    try {
      // Initialize window manager
      await windowManager.ensureInitialized();

      // Set window effects based on platform
      if (Platform.isWindows) {
        await _initializeWindows();
      } else if (Platform.isMacOS) {
        await _initializeMacOS();
      } else if (Platform.isLinux) {
        await _initializeLinux();
      }
    } catch (e) {
      debugPrint('Failed to initialize window effects: $e');
    }
  }

  static bool get _isDesktop =>
      Platform.isWindows || Platform.isMacOS || Platform.isLinux;

  static Future<void> _initializeWindows() async {
    try {
      // Try Mica effect (Windows 11)
      await Window.setEffect(
        effect: WindowEffect.mica,
        color: const Color(0xFF0A0B0E),
        dark: true,
      );
    } catch (e) {
      try {
        // Fallback to Acrylic (Windows 10)
        await Window.setEffect(
          effect: WindowEffect.acrylic,
          color: const Color(0xFF0A0B0E),
          dark: true,
        );
      } catch (e) {
        debugPrint('Acrylic not supported, using solid color');
      }
    }
  }

  static Future<void> _initializeMacOS() async {
    try {
      // macOS vibrancy
      await Window.setEffect(effect: WindowEffect.sidebar, dark: true);
    } catch (e) {
      debugPrint('Vibrancy not supported: $e');
    }
  }

  static Future<void> _initializeLinux() async {
    // Linux doesn't support special effects
    // Just set transparent background
    try {
      await Window.setEffect(
        effect: WindowEffect.transparent,
        color: const Color(0xFF0A0B0E),
        dark: true,
      );
    } catch (e) {
      debugPrint('Transparency not supported: $e');
    }
  }

  /// Update window title
  static Future<void> setTitle(String title) async {
    if (!_isDesktop) return;
    await windowManager.setTitle(title);
  }

  /// Toggle fullscreen
  static Future<void> toggleFullscreen() async {
    if (!_isDesktop) return;
    final isFullScreen = await windowManager.isFullScreen();
    await windowManager.setFullScreen(!isFullScreen);
  }

  /// Set window size
  static Future<void> setSize(Size size) async {
    if (!_isDesktop) return;
    await windowManager.setSize(size);
  }

  /// Center window
  static Future<void> center() async {
    if (!_isDesktop) return;
    await windowManager.center();
  }

  /// Get window size
  static Future<Size> getSize() async {
    if (!_isDesktop) return Size.zero;
    return windowManager.getSize();
  }

  /// Set minimum size
  static Future<void> setMinimumSize(Size size) async {
    if (!_isDesktop) return;
    await windowManager.setMinimumSize(size);
  }

  /// Set maximum size
  static Future<void> setMaximumSize(Size size) async {
    if (!_isDesktop) return;
    await windowManager.setMaximumSize(size);
  }

  /// Show window
  static Future<void> show() async {
    if (!_isDesktop) return;
    await windowManager.show();
  }

  /// Hide window
  static Future<void> hide() async {
    if (!_isDesktop) return;
    await windowManager.hide();
  }
}
