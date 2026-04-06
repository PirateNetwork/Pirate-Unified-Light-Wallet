import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// Managed clipboard helper for wallet-sensitive data.
///
/// The manager tracks the last value it placed on the clipboard so it can:
/// - clear secret material after an expiry window
/// - clear secret material when the app backgrounds
/// - avoid wiping unrelated clipboard content that the user copied later
class ClipboardManager {
  static Timer? _clearTimer;
  static DateTime? _expiresAt;
  static String? _managedText;
  static ClipboardDataType? _managedType;
  static int _sessionId = 0;
  static VoidCallback? _onCleared;

  static const Duration _defaultClearDelay = Duration(seconds: 30);

  /// Copy text with managed expiry behavior.
  static Future<void> copyWithAutoClear(
    String text, {
    Duration clearAfter = _defaultClearDelay,
    ClipboardDataType dataType = ClipboardDataType.text,
    VoidCallback? onCleared,
  }) async {
    final sessionId = _beginSession(
      text: text,
      dataType: dataType,
      clearAfter: clearAfter,
      onCleared: onCleared,
    );

    await Clipboard.setData(ClipboardData(text: text));

    _clearTimer = Timer(clearAfter, () {
      unawaited(_clearManagedClipboard(sessionId));
    });
  }

  /// Copy seed or key material with a short expiry and background clearing.
  static Future<void> copySensitive(
    String text, {
    Duration? clearAfter,
    ClipboardDataType dataType = ClipboardDataType.spendingKey,
    VoidCallback? onCleared,
  }) {
    return copyWithAutoClear(
      text,
      clearAfter: clearAfter ?? dataType.clearDelay,
      dataType: dataType,
      onCleared: onCleared,
    );
  }

  static Future<void> copySeed(
    String text, {
    Duration? clearAfter,
    VoidCallback? onCleared,
  }) {
    return copySensitive(
      text,
      clearAfter: clearAfter ?? ClipboardDataType.seed.clearDelay,
      dataType: ClipboardDataType.seed,
      onCleared: onCleared,
    );
  }

  static Future<void> copyViewingKey(
    String text, {
    Duration? clearAfter,
    VoidCallback? onCleared,
  }) {
    return copySensitive(
      text,
      clearAfter: clearAfter ?? ClipboardDataType.viewingKey.clearDelay,
      dataType: ClipboardDataType.viewingKey,
      onCleared: onCleared,
    );
  }

  /// Copy an address with a longer expiry window.
  static Future<void> copyAddress(
    String address, {
    VoidCallback? onCleared,
  }) {
    return copyWithAutoClear(
      address,
      clearAfter: ClipboardDataType.address.clearDelay,
      dataType: ClipboardDataType.address,
      onCleared: onCleared,
    );
  }

  /// Clear the currently managed clipboard value if it is still present.
  static Future<void> clearNow() async {
    _clearTimer?.cancel();
    await _clearManagedClipboard(_sessionId);
  }

  /// Drop the timer/session without modifying the current clipboard contents.
  static void cancelAutoClear() {
    _clearTimer?.cancel();
    _clearTimer = null;
    _expiresAt = null;
    _managedText = null;
    _managedType = null;
    _onCleared = null;
  }

  /// Clear managed secret material when the app moves out of the foreground.
  static Future<void> handleAppLifecycleState(AppLifecycleState state) async {
    final managedType = _managedType;
    if (managedType == null || !managedType.clearOnBackground) {
      return;
    }

    switch (state) {
      case AppLifecycleState.resumed:
        return;
      case AppLifecycleState.inactive:
      case AppLifecycleState.paused:
      case AppLifecycleState.hidden:
      case AppLifecycleState.detached:
        await clearNow();
        return;
    }
  }

  /// Remaining managed lifetime for the current clipboard session.
  static Duration? get remainingTime {
    final expiresAt = _expiresAt;
    if (expiresAt == null) {
      return null;
    }
    final remaining = expiresAt.difference(DateTime.now());
    return remaining.isNegative ? Duration.zero : remaining;
  }

  static int _beginSession({
    required String text,
    required ClipboardDataType dataType,
    required Duration clearAfter,
    VoidCallback? onCleared,
  }) {
    _clearTimer?.cancel();
    _sessionId += 1;
    _managedText = text;
    _managedType = dataType;
    _expiresAt = DateTime.now().add(clearAfter);
    _onCleared = onCleared;
    return _sessionId;
  }

  static Future<void> _clearManagedClipboard(int sessionId) async {
    if (sessionId != _sessionId) {
      return;
    }

    final managedText = _managedText;
    final callback = _onCleared;

    try {
      if (managedText != null) {
        final clipboardData = await Clipboard.getData(Clipboard.kTextPlain);
        if (clipboardData?.text == managedText) {
          await Clipboard.setData(const ClipboardData(text: ''));
        }
      }
    } finally {
      if (sessionId == _sessionId) {
        cancelAutoClear();
        callback?.call();
      }
    }
  }
}

enum ClipboardDataType {
  seed,
  spendingKey,
  viewingKey,
  address,
  txid,
  text,
}

extension ClipboardDataTypeExtension on ClipboardDataType {
  Duration get clearDelay {
    switch (this) {
      case ClipboardDataType.seed:
        return const Duration(seconds: 30);
      case ClipboardDataType.spendingKey:
        return const Duration(seconds: 15);
      case ClipboardDataType.viewingKey:
        return const Duration(seconds: 30);
      case ClipboardDataType.address:
        return const Duration(seconds: 60);
      case ClipboardDataType.txid:
      case ClipboardDataType.text:
        return const Duration(seconds: 30);
    }
  }

  bool get clearOnBackground {
    switch (this) {
      case ClipboardDataType.seed:
      case ClipboardDataType.spendingKey:
      case ClipboardDataType.viewingKey:
        return true;
      case ClipboardDataType.address:
      case ClipboardDataType.txid:
      case ClipboardDataType.text:
        return false;
    }
  }

  String get displayName {
    switch (this) {
      case ClipboardDataType.seed:
        return 'Seed Phrase';
      case ClipboardDataType.spendingKey:
        return 'Spending Key';
      case ClipboardDataType.viewingKey:
        return 'Viewing Key';
      case ClipboardDataType.address:
        return 'Address';
      case ClipboardDataType.txid:
        return 'Transaction ID';
      case ClipboardDataType.text:
        return 'Text';
    }
  }
}
