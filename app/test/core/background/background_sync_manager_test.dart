import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/background/background_sync_manager.dart';

void main() {
  group('BackgroundSyncResult', () {
    test('fromMap applies safe defaults for partial platform payloads', () {
      final result = BackgroundSyncResult.fromMap(const {
        'errors': <dynamic>['sync stalled'],
      });

      expect(result.mode, 'compact');
      expect(result.blocksSynced, 0);
      expect(result.durationSecs, 0);
      expect(result.newTransactions, 0);
      expect(result.newBalance, isNull);
      expect(result.tunnelUsed, 'tor');
      expect(result.errors, ['sync stalled']);
      expect(result.hasErrors, isTrue);
      expect(result.hasNewTransactions, isFalse);
    });

    test('fromMap preserves explicit values from native payloads', () {
      final result = BackgroundSyncResult.fromMap(const {
        'mode': 'deep',
        'blocks_synced': 512,
        'duration_secs': 44,
        'new_transactions': 3,
        'new_balance': 987654321,
        'tunnel_used': 'socks5',
        'errors': <dynamic>[],
      });

      expect(result.mode, 'deep');
      expect(result.blocksSynced, 512);
      expect(result.durationSecs, 44);
      expect(result.newTransactions, 3);
      expect(result.newBalance, 987654321);
      expect(result.tunnelUsed, 'socks5');
      expect(result.errors, isEmpty);
      expect(result.hasErrors, isFalse);
      expect(result.hasNewTransactions, isTrue);
    });
  });

  group('SyncStatus', () {
    test('fromMap parses timestamps and flags from the platform shape', () {
      final compactAt = DateTime.fromMillisecondsSinceEpoch(1_700_000_000_000);
      final deepAt = DateTime.fromMillisecondsSinceEpoch(1_700_003_600_000);

      final status = SyncStatus.fromMap({
        'last_compact_sync': compactAt.millisecondsSinceEpoch,
        'last_deep_sync': deepAt.millisecondsSinceEpoch,
        'tunnel_mode': 'i2p',
        'is_running': true,
        'current_mode': 'deep',
      });

      expect(status.lastCompactSync, compactAt);
      expect(status.lastDeepSync, deepAt);
      expect(status.tunnelMode, 'i2p');
      expect(status.isRunning, isTrue);
      expect(status.currentMode, 'deep');
    });

    test(
      'fromMap falls back to conservative defaults when fields are missing',
      () {
        final status = SyncStatus.fromMap(<String, dynamic>{});

        expect(status.lastCompactSync, isNull);
        expect(status.lastDeepSync, isNull);
        expect(status.tunnelMode, 'tor');
        expect(status.isRunning, isFalse);
        expect(status.currentMode, isNull);
      },
    );
  });

  group('TunnelModeExtension', () {
    test('fromName normalizes supported values and unknown inputs', () {
      expect(TunnelModeExtension.fromName('i2p'), TunnelMode.i2p);
      expect(TunnelModeExtension.fromName('socks5'), TunnelMode.socks5);
      expect(TunnelModeExtension.fromName('direct'), TunnelMode.direct);
      expect(TunnelModeExtension.fromName('unknown'), TunnelMode.tor);
      expect(TunnelModeExtension.fromName(''), TunnelMode.tor);
    });
  });
}
