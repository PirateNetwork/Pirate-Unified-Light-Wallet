// ignore_for_file: avoid_print

import 'dart:async';

/// Small helper for the post-wallet-creation sync bootstrap path.
///
/// Keeping the delayed auto-sync launch out of the main bridge file reduces
/// the amount of lifecycle coordination logic mixed into the FFI adapter.
class WalletLifecycleSyncHelper {
  static Future<void> startCompactSyncAfterCreate({
    required String walletId,
    required Future<void> Function(String walletId) startSync,
    Duration delay = const Duration(milliseconds: 500),
  }) async {
    try {
      await Future<void>.delayed(delay);
      await startSync(walletId);
    } catch (e) {
      // Log error but don't throw - sync can be manually started later.
      print('Auto-sync start failed: $e');
    }
  }
}
