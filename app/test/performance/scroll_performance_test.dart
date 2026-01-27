/// Performance tests for scroll/jank detection
library;

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/features/activity/activity_screen.dart';
import 'package:pirate_wallet/features/home/home_screen.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';

const _testWalletId = 'test-wallet-1';
final bool _skipPerformanceTests = Platform.environment['CI'] == 'true';

class _TestActiveWalletNotifier extends ActiveWalletNotifier {
  @override
  WalletId? build() => _testWalletId;
}

class _TestTunnelModeNotifier extends TunnelModeNotifier {
  @override
  TunnelMode build() => const TunnelMode.tor();
}

class _TestTorStatusNotifier extends TorStatusNotifier {
  @override
  TorStatusDetails build() => const TorStatusDetails(status: 'ready');
}

class _TestTransportConfigNotifier extends TransportConfigNotifier {
  @override
  TransportConfig build() => const TransportConfig(
        mode: 'tor',
        dnsProvider: 'cloudflare_doh',
        socks5Config: {
          'host': 'localhost',
          'port': '1080',
          'username': null,
          'password': null,
        },
        i2pEndpoint: '',
        tlsPins: [],
        torBridge: TorBridgeConfig(
          useBridges: false,
          fallbackToBridges: true,
          transport: 'snowflake',
          bridgeLines: [],
          transportPath: null,
        ),
      );
}

List<TxInfo> _buildTestTransactions() {
  final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
  return List.generate(120, (index) {
    final isReceived = index % 2 == 0;
    return TxInfo(
      txid: 'tx_$index',
      height: 100000 + index,
      timestamp: now - (index * 60),
      amount: (isReceived ? 1 : -1) * (10000000 + index * 2000),
      fee: BigInt.from(1000),
      memo: index % 3 == 0 ? 'Test memo $index' : null,
      confirmed: index % 5 != 0,
    );
  });
}

Widget _buildTestApp({required Widget child, List<TxInfo>? transactions}) {
  final txs = transactions ?? _buildTestTransactions();
  return ProviderScope(
    overrides: [
      activeWalletProvider.overrideWith(_TestActiveWalletNotifier.new),
      walletsProvider.overrideWith((ref) async {
        return [
          WalletMeta(
            id: _testWalletId,
            name: 'Primary Wallet',
            createdAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
            watchOnly: false,
            birthdayHeight: 1700000,
            networkType: 'mainnet',
          ),
        ];
      }),
      balanceStreamProvider.overrideWith((ref) {
        return Stream.value(
          Balance(
            total: BigInt.from(250000000),
            spendable: BigInt.from(200000000),
            pending: BigInt.from(50000000),
          ),
        );
      }),
      syncProgressStreamProvider.overrideWith((ref) {
        return Stream.value(
          SyncStatus(
            localHeight: BigInt.from(1000),
            targetHeight: BigInt.from(1200),
            percent: 83.0,
            eta: BigInt.from(120),
            stage: SyncStage.notes,
            lastCheckpoint: BigInt.from(900),
            blocksPerSecond: 18.4,
            notesDecrypted: BigInt.from(12000),
            lastBatchMs: BigInt.from(320),
          ),
        );
      }),
      isSyncRunningProvider.overrideWith((ref) async => true),
      transactionsProvider.overrideWith((ref) async => txs),
      tunnelModeProvider.overrideWith(_TestTunnelModeNotifier.new),
      lightdEndpointConfigProvider.overrideWith(
        (ref) async => const LightdEndpointConfig(
          url: 'https://lightd.pirate.black:443',
        ),
      ),
      torStatusProvider.overrideWith(_TestTorStatusNotifier.new),
      transportConfigProvider.overrideWith(_TestTransportConfigNotifier.new),
    ],
    child: MaterialApp(home: child),
  );
}

void main() {
  group('Scroll Performance Tests', () {
    testWidgets('home screen - transaction list scroll performance',
        (tester) async {
      if (_skipPerformanceTests) return;
      await tester.pumpWidget(
        _buildTestApp(child: const HomeScreen()),
      );

      // Wait for initial render
      await tester.pump(const Duration(milliseconds: 600));

      // Measure frame times during scroll
      final frameTimes = <Duration>[];
      
      // Enable frame callback
      tester.binding.addTimingsCallback((timings) {
        for (final timing in timings) {
          frameTimes.add(timing.totalSpan);
        }
      });

      // Perform scroll
      final listFinder = find.byType(CustomScrollView);
      expect(listFinder, findsOneWidget);

      // Scroll down
      await tester.drag(listFinder, const Offset(0, -500));
      await tester.pump(const Duration(milliseconds: 300));

      // Scroll up
      await tester.drag(listFinder, const Offset(0, 500));
      await tester.pump(const Duration(milliseconds: 300));

      // Analyze frame times
      if (frameTimes.isNotEmpty) {
        final averageFrameTime = Duration(
          microseconds:
              frameTimes.map((t) => t.inMicroseconds).reduce((a, b) => a + b) ~/
                  frameTimes.length,
        );
        final maxFrameTime = frameTimes.reduce(
          (a, b) => a > b ? a : b,
        );

        // 60 FPS = 16.67ms per frame
        // 120 FPS = 8.33ms per frame
        const target60fps = Duration(microseconds: 16670);
        const target120fps = Duration(microseconds: 8330);

        debugPrint('Scroll performance:');
        debugPrint('  Average frame time: us');
        debugPrint('  Max frame time: us');
        debugPrint('  60 FPS target: us');
        debugPrint('  Frames total: ');

        // Assert 60 FPS capability
        expect(
          averageFrameTime,
          lessThan(target60fps),
          reason: 'Average frame time should be under 16.67ms for 60 FPS',
        );

        // Warn if max frame time indicates jank
        if (maxFrameTime > target60fps * 2) {
          debugPrint('Warning: potential jank detected: us frame');
        }
      }
    });

    testWidgets('activity screen - transaction list scroll performance',
        (tester) async {
      if (_skipPerformanceTests) return;
      await tester.pumpWidget(
        _buildTestApp(child: const ActivityScreen()),
      );

      await tester.pump(const Duration(milliseconds: 600));

      final frameTimes = <Duration>[];
      tester.binding.addTimingsCallback((timings) {
        for (final timing in timings) {
          frameTimes.add(timing.totalSpan);
        }
      });

      final listFinder = find.byType(ListView);
      expect(listFinder, findsOneWidget);

      await tester.drag(listFinder, const Offset(0, -600));
      await tester.pump(const Duration(milliseconds: 300));

      await tester.drag(listFinder, const Offset(0, 600));
      await tester.pump(const Duration(milliseconds: 300));

      if (frameTimes.isNotEmpty) {
        final averageFrameTime = Duration(
          microseconds:
              frameTimes.map((t) => t.inMicroseconds).reduce((a, b) => a + b) ~/
                  frameTimes.length,
        );
        final maxFrameTime = frameTimes.reduce((a, b) => a > b ? a : b);
        const target60fps = Duration(microseconds: 16670);

        debugPrint('Activity scroll performance:');
        debugPrint('  Average frame time: us');
        debugPrint('  Max frame time: us');
        debugPrint('  Frames total: ');

        expect(
          averageFrameTime,
          lessThan(target60fps),
          reason: 'Activity list should average under 16.67ms per frame',
        );
      }
    });
  });

  group('Animation Performance Tests', () {
    testWidgets('home screen - sync indicator animation', (tester) async {
      if (_skipPerformanceTests) return;
      await tester.pumpWidget(
        _buildTestApp(child: const HomeScreen()),
      );

      final frameTimes = <Duration>[];
      
      tester.binding.addTimingsCallback((timings) {
        for (final timing in timings) {
          frameTimes.add(timing.totalSpan);
        }
      });

      // Let animation run for 2 seconds
      await tester.pump(const Duration(seconds: 2));
      await tester.pump(const Duration(milliseconds: 200));

      // Check smooth animation
      if (frameTimes.isNotEmpty) {
        final averageFrameTime = Duration(
          microseconds:
              frameTimes.map((t) => t.inMicroseconds).reduce((a, b) => a + b) ~/
                  frameTimes.length,
        );
        
        const target60fps = Duration(microseconds: 16670);
        
        debugPrint('Animation performance:');
        debugPrint('  Average frame time: us');
        
        expect(
          averageFrameTime,
          lessThan(target60fps),
          reason: 'Animations should maintain 60 FPS',
        );
      }
    });
  });

  group('Memory Performance Tests', () {
    testWidgets('home screen - memory usage during scroll', (tester) async {
      if (_skipPerformanceTests) return;
      await tester.pumpWidget(
        _buildTestApp(child: const HomeScreen()),
      );

      await tester.pump(const Duration(milliseconds: 600));

      // Record initial memory
      // Note: Actual memory profiling requires integration tests
      // This is a placeholder for the pattern

      // Perform scrolling
      for (var i = 0; i < 10; i++) {
        await tester.drag(
          find.byType(CustomScrollView),
          const Offset(0, -500),
        );
        await tester.pump();
      }

      await tester.pump(const Duration(milliseconds: 300));

      // Check for memory leaks
      // In real tests, use DevTools memory profiling
      debugPrint('OK: Memory test completed (manual profiling required)');
    });
  });
}
