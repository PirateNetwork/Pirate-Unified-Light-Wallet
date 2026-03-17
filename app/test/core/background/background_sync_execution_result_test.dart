import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/background/background_sync_execution_result.dart';
import 'package:pirate_wallet/core/background/background_sync_manager.dart';

void main() {
  test('typed execution result converts to platform map consistently', () {
    const result = BackgroundSyncExecutionResult(
      walletId: 'wallet_a',
      mode: 'compact',
      blocksSynced: 42,
      startHeight: 1000,
      endHeight: 1042,
      durationSecs: 9,
      newTransactions: 1,
      newBalance: 123456,
      tunnelUsed: 'tor',
      errors: ['warning'],
    );

    expect(result.toPlatformMap(), {
      'wallet_id': 'wallet_a',
      'walletId': 'wallet_a',
      'mode': 'compact',
      'blocks_synced': 42,
      'start_height': 1000,
      'end_height': 1042,
      'duration_secs': 9,
      'new_transactions': 1,
      'new_balance': 123456,
      'tunnel_used': 'tor',
      'errors': ['warning'],
    });
  });

  test('ui background sync result derives from typed execution result', () {
    const execution = BackgroundSyncExecutionResult(
      mode: 'deep',
      blocksSynced: 250,
      startHeight: 5000,
      endHeight: 5250,
      durationSecs: 30,
      newTransactions: 2,
      newBalance: 777,
      tunnelUsed: 'socks5',
    );

    final uiResult = BackgroundSyncResult.fromExecution(execution);

    expect(uiResult.mode, 'deep');
    expect(uiResult.blocksSynced, 250);
    expect(uiResult.durationSecs, 30);
    expect(uiResult.newTransactions, 2);
    expect(uiResult.newBalance, 777);
    expect(uiResult.tunnelUsed, 'socks5');
    expect(uiResult.errors, isEmpty);
  });
}
