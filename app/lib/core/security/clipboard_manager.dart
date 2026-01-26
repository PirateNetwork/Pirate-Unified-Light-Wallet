// Secure clipboard manager with auto-clear timer

import 'dart:async';
import 'package:flutter/services.dart';

/// Secure clipboard manager
class ClipboardManager {
  static Timer? _clearTimer;
  static const Duration _defaultClearDelay = Duration(seconds: 30);

  /// Copy to clipboard with auto-clear
  static Future<void> copyWithAutoClear(
    String text, {
    Duration clearAfter = _defaultClearDelay,
    VoidCallback? onCleared,
  }) async {
    // Cancel existing timer
    _clearTimer?.cancel();

    // Copy to clipboard
    await Clipboard.setData(ClipboardData(text: text));

    // Set timer to clear
    _clearTimer = Timer(clearAfter, () {
      _clearClipboard();
      onCleared?.call();
    });
  }

  /// Copy sensitive data (seeds, private keys) with short timeout
  static Future<void> copySensitive(
    String text, {
    Duration clearAfter = const Duration(seconds: 15),
    VoidCallback? onCleared,
  }) async {
    return copyWithAutoClear(
      text,
      clearAfter: clearAfter,
      onCleared: onCleared,
    );
  }

  /// Copy address with normal timeout
  static Future<void> copyAddress(
    String address, {
    VoidCallback? onCleared,
  }) async {
    return copyWithAutoClear(
      address,
      clearAfter: const Duration(seconds: 60),
      onCleared: onCleared,
    );
  }

  /// Clear clipboard immediately
  static Future<void> clearNow() async {
    _clearTimer?.cancel();
    await _clearClipboard();
  }

  /// Cancel auto-clear timer
  static void cancelAutoClear() {
    _clearTimer?.cancel();
    _clearTimer = null;
  }

  static Future<void> _clearClipboard() async {
    await Clipboard.setData(const ClipboardData(text: ''));
  }

  /// Get remaining time until auto-clear
  static Duration? get remainingTime {
    // Note: Timer doesn't expose remaining time, would need custom implementation
    return null;
  }
}

/// Clipboard data type
enum ClipboardDataType {
  /// Seed phrase
  seed,
  /// Private key
  privateKey,
  /// Address
  address,
  /// Transaction ID
  txid,
  /// General text
  text,
}

/// Extension for clipboard data types
extension ClipboardDataTypeExtension on ClipboardDataType {
  /// Get recommended clear delay
  Duration get clearDelay {
    switch (this) {
      case ClipboardDataType.seed:
      case ClipboardDataType.privateKey:
        return const Duration(seconds: 10);
      case ClipboardDataType.address:
        return const Duration(seconds: 60);
      case ClipboardDataType.txid:
        return const Duration(seconds: 30);
      case ClipboardDataType.text:
        return const Duration(seconds: 30);
    }
  }

  /// Get display name
  String get displayName {
    switch (this) {
      case ClipboardDataType.seed:
        return 'Seed Phrase';
      case ClipboardDataType.privateKey:
        return 'Private Key';
      case ClipboardDataType.address:
        return 'Address';
      case ClipboardDataType.txid:
        return 'Transaction ID';
      case ClipboardDataType.text:
        return 'Text';
    }
  }
}

