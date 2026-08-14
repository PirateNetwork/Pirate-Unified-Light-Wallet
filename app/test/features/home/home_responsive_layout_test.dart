import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/core/providers/price_providers.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/features/home/home_screen.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';

class _TestTunnelModeNotifier extends TunnelModeNotifier {
  @override
  TunnelMode build() => const TunnelMode.direct();
}

class _TestTorStatusNotifier extends TorStatusNotifier {
  @override
  TorStatusDetails build() => const TorStatusDetails(status: 'ready');
}

class _TestTransportConfigNotifier extends TransportConfigNotifier {
  @override
  TransportConfig build() => const TransportConfig(
    mode: 'direct',
    dnsProvider: 'cloudflare_doh',
    socks5Config: {},
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

Widget _testApp() {
  final syncedStatus = SyncStatus(
    localHeight: BigInt.from(4100000),
    targetHeight: BigInt.from(4100000),
    percent: 100,
    eta: null,
    stage: SyncStage.verify,
    lastCheckpoint: null,
    blocksPerSecond: 0,
    notesDecrypted: BigInt.zero,
    lastBatchMs: BigInt.zero,
  );

  return ProviderScope(
    overrides: [
      activeWalletMetaProvider.overrideWithValue(
        WalletMeta(
          id: 'wallet-1',
          name: 'My Pirate Wallet',
          createdAt: 0,
          watchOnly: false,
          birthdayHeight: 3500000,
          networkType: 'mainnet',
        ),
      ),
      balanceStreamProvider.overrideWith(
        (ref) => Stream.value(
          Balance(
            total: BigInt.from(100000000),
            spendable: BigInt.from(100000000),
            pending: BigInt.zero,
          ),
        ),
      ),
      syncProgressStreamProvider.overrideWith(
        (ref) => Stream.value(syncedStatus),
      ),
      syncStatusProvider.overrideWith((ref) async => syncedStatus),
      transactionsProvider.overrideWith((ref) async => const []),
      arrrPriceQuoteProvider.overrideWith((ref) => Stream.value(null)),
      decoySyncHeightProvider.overrideWith((ref) async => 0),
      tunnelModeProvider.overrideWith(_TestTunnelModeNotifier.new),
      torStatusProvider.overrideWith(_TestTorStatusNotifier.new),
      transportConfigProvider.overrideWith(_TestTransportConfigNotifier.new),
      lightdEndpointConfigProvider.overrideWith(
        (ref) async =>
            const LightdEndpointConfig(url: 'http://64.23.167.130:9067'),
      ),
    ],
    child: MaterialApp(
      theme: PTheme.dark(),
      home: const Scaffold(body: HomeScreen(useScaffold: false)),
    ),
  );
}

void main() {
  testWidgets('lets the dashboard header scroll away in phone landscape', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(844, 390);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(_testApp());
    await tester.pump(const Duration(milliseconds: 100));

    final header = tester.widget<SliverPersistentHeader>(
      find.byKey(HomeScreen.headerKey),
    );
    expect(header.pinned, isFalse);

    await tester.drag(find.byType(CustomScrollView), const Offset(0, -500));
    await tester.pumpAndSettle();

    final recentActivity = tester.widget<Text>(
      find.byKey(HomeScreen.recentActivityTitleKey),
    );
    expect(recentActivity.data, 'Recent activity');
    expect(recentActivity.maxLines, isNull);
    expect(recentActivity.overflow, isNull);
    expect(tester.takeException(), isNull);
    debugDefaultTargetPlatformOverride = null;
  });
}
