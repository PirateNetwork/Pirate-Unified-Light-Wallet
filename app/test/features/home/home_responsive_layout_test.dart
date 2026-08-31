import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/core/providers/price_providers.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/design/tokens/spacing.dart';
import 'package:pirate_wallet/features/home/home_screen.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';
import 'package:pirate_wallet/ui/molecules/transaction_row_v2.dart';
import 'package:pirate_wallet/ui/organisms/p_sliver_header.dart';

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

Widget _testApp({
  BigInt? total,
  BigInt? pending,
  List<TxInfo> transactions = const [],
  Key? key,
}) {
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
    key: key,
    overrides: [
      activeWalletMetaProvider.overrideWithValue(
        WalletMeta(
          id: 'wallet-1',
          name: 'My Stashi Wallet',
          createdAt: 0,
          watchOnly: false,
          birthdayHeight: 3500000,
          networkType: 'mainnet',
        ),
      ),
      balanceStreamProvider.overrideWith(
        (ref) => Stream.value(
          Balance(
            total: total ?? BigInt.from(100000000),
            spendable: BigInt.from(100000000),
            pending: pending ?? BigInt.zero,
          ),
        ),
      ),
      syncProgressStreamProvider.overrideWith(
        (ref) => Stream.value(syncedStatus),
      ),
      syncStatusProvider.overrideWith((ref) async => syncedStatus),
      transactionsProvider.overrideWith((ref) async => transactions),
      arrrPriceQuoteProvider.overrideWith((ref) => Stream.value(null)),
      decoySyncHeightProvider.overrideWith((ref) async => 0),
      tunnelModeProvider.overrideWith(_TestTunnelModeNotifier.new),
      torStatusProvider.overrideWith(_TestTorStatusNotifier.new),
      transportConfigProvider.overrideWith(_TestTransportConfigNotifier.new),
      lightdEndpointConfigProvider.overrideWith(
        (ref) async =>
            const LightdEndpointConfig(url: 'https://lightd1.pirate.black:443'),
      ),
    ],
    child: MaterialApp(
      theme: PTheme.dark(),
      home: const Scaffold(body: HomeScreen(useScaffold: false)),
    ),
  );
}

void main() {
  testWidgets('reserves balance helper height only when it is needed', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1280, 900);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(_testApp(key: const ValueKey('settled-balance')));
    await tester.pump(const Duration(milliseconds: 100));
    final settledHeader = tester.widget<SliverPersistentHeader>(
      find.byKey(HomeScreen.headerKey),
    );
    final settledExtent =
        (settledHeader.delegate as PSliverHeaderDelegate).maxExtent;

    await tester.pumpWidget(
      _testApp(
        pending: BigInt.from(50000000),
        key: const ValueKey('pending-balance'),
      ),
    );
    await tester.pump(const Duration(milliseconds: 100));
    final pendingHeader = tester.widget<SliverPersistentHeader>(
      find.byKey(HomeScreen.headerKey),
    );
    final pendingExtent =
        (pendingHeader.delegate as PSliverHeaderDelegate).maxExtent;

    expect(pendingExtent - settledExtent, 36);
    expect(tester.takeException(), isNull);
    debugDefaultTargetPlatformOverride = null;
  });

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

  testWidgets('uses a shorter dashboard header on laptop viewports', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1097, 706);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(_testApp());
    await tester.pump(const Duration(milliseconds: 100));

    final header = tester.widget<SliverPersistentHeader>(
      find.byKey(HomeScreen.headerKey),
    );
    final extent = (header.delegate as PSliverHeaderDelegate).maxExtent;

    expect(extent, lessThanOrEqualTo(252));
    expect(extent, lessThan(284));
    expect(tester.takeException(), isNull);
    debugDefaultTargetPlatformOverride = null;
  });

  testWidgets('keeps the pinned phone header opaque while content scrolls', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(390, 844);
    addTearDown(tester.view.reset);

    await tester.pumpWidget(_testApp());
    await tester.pump(const Duration(milliseconds: 100));
    await tester.drag(find.byType(CustomScrollView), const Offset(0, -700));
    await tester.pumpAndSettle();

    final surface = tester.widget<DecoratedBox>(
      find.byKey(HomeScreen.headerSurfaceKey),
    );
    final decoration = surface.decoration as BoxDecoration;
    expect(decoration.color?.a, 1.0);
    expect(tester.takeException(), isNull);
    debugDefaultTargetPlatformOverride = null;
  });

  testWidgets('keeps long recent amounts clear and separates mobile cards', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(390, 844);
    addTearDown(tester.view.reset);

    final timestamp = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final transactions = [
      TxInfo(
        txid: 'large-receive',
        height: 4099999,
        timestamp: timestamp,
        amount: 7799999970000,
        fee: BigInt.zero,
        memo: null,
        confirmed: true,
        expired: false,
      ),
      TxInfo(
        txid: 'large-send',
        height: 4099998,
        timestamp: timestamp - 60,
        amount: -1234567890000,
        fee: BigInt.from(10000),
        memo: null,
        confirmed: true,
        expired: false,
      ),
    ];

    await tester.pumpWidget(_testApp(transactions: transactions));
    await tester.pump(const Duration(milliseconds: 100));
    await tester.drag(find.byType(CustomScrollView), const Offset(0, -900));
    await tester.pumpAndSettle();

    final rows = find.byType(TransactionRowV2);
    expect(rows, findsNWidgets(2));
    expect(find.text('+77999.9997 ARRR'), findsOneWidget);
    expect(find.text('-12345.6789 ARRR'), findsOneWidget);

    final first = tester.getRect(rows.at(0));
    final second = tester.getRect(rows.at(1));
    expect(second.top - first.bottom, PSpacing.sm);
    expect(tester.takeException(), isNull);
    debugDefaultTargetPlatformOverride = null;
  });
}
