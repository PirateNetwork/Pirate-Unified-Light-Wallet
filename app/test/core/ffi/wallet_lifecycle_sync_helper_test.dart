import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/wallet_lifecycle_sync_helper.dart';

void main() {
  test(
    'startCompactSyncAfterCreate forwards the wallet id on the next event turn',
    () async {
      final calls = <String>[];

      final future = WalletLifecycleSyncHelper.startCompactSyncAfterCreate(
        walletId: 'wallet_123',
        startSync: (walletId) async {
          calls.add(walletId);
        },
        delay: Duration.zero,
      );

      expect(calls, isEmpty);

      await future;

      expect(calls, ['wallet_123']);
    },
  );

  test('startCompactSyncAfterCreate swallows startSync failures', () async {
    final future = WalletLifecycleSyncHelper.startCompactSyncAfterCreate(
      walletId: 'wallet_456',
      startSync: (_) async {
        throw StateError('boom');
      },
      delay: Duration.zero,
    );

    await expectLater(future, completes);
  });
}
