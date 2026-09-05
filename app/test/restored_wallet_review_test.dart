import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart';
import 'package:pirate_wallet/core/ffi/generated/frb_generated.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart'
    hide AddressInfo, NodeTestResult;
import 'package:pirate_wallet/core/providers/connection_status_provider.dart';
import 'package:pirate_wallet/core/providers/price_providers.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/core/services/address_rotation_service.dart';
import 'package:pirate_wallet/core/swaps/swap_providers.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/features/keys/keys_screen.dart';
import 'package:pirate_wallet/features/receive/receive_screen.dart';
import 'package:pirate_wallet/features/receive/widgets/address_history_list.dart';
import 'package:pirate_wallet/features/send/send_screen.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';
import 'package:pirate_wallet/ui/atoms/p_button.dart';

import 'support/restored_wallet_api.dart';

const _captureBoundaryKey = ValueKey('restored-wallet-capture-boundary');

class _ActiveWallet extends ActiveWalletNotifier {
  @override
  String? build() => 'restored-review-wallet';
}

class _NormalMode extends DecoyModeNotifier {
  @override
  bool build() => false;
}

class _DirectTunnelMode extends TunnelModeNotifier {
  @override
  TunnelMode build() => const TunnelMode.tor();
}

class _ReadyTorStatus extends TorStatusNotifier {
  @override
  TorStatusDetails build() => const TorStatusDetails(status: 'ready');
}

class _DarkThemeMode extends ThemeModeNotifier {
  @override
  AppThemeMode build() => AppThemeMode.dark;
}

class _TorTransport extends TransportConfigNotifier {
  @override
  TransportConfig build() => const TransportConfig(
    mode: 'tor',
    dnsProvider: 'system',
    socks5Config: <String, String?>{},
    i2pEndpoint: 'http://5vjlbxmzx4gjfuwcot2qtfjdnxodzpe4jsw3ckx7i4maltz7j5qa.b32.i2p:9067',
    tlsPins: <Map<String, String>>[],
    torBridge: TorBridgeConfig(
      useBridges: false,
      fallbackToBridges: false,
      transport: 'snowflake',
      bridgeLines: <String>[],
      transportPath: null,
    ),
  );
}

const _walletMeta = WalletMeta(
  id: 'restored-review-wallet',
  name: 'My ARRR Wallet',
  createdAt: 1787961600,
  watchOnly: false,
  birthdayHeight: 3500000,
  networkType: 'mainnet',
);

final _syncedStatus = SyncStatus(
  localHeight: BigInt.from(4111871),
  targetHeight: BigInt.from(4111871),
  percent: 100,
  eta: null,
  stage: SyncStage.verify,
  lastCheckpoint: null,
  blocksPerSecond: 0,
  notesDecrypted: BigInt.from(8),
  lastBatchMs: BigInt.zero,
);

Widget _walletApp(Widget child, {bool scanning = false}) {
  final status = scanning
      ? SyncStatus(
          localHeight: BigInt.from(100),
          targetHeight: BigInt.from(4111871),
          percent: 1,
          stage: SyncStage.notes,
          blocksPerSecond: 100,
          notesDecrypted: BigInt.zero,
          lastBatchMs: BigInt.zero,
        )
      : _syncedStatus;
  return ProviderScope(
    overrides: [
      activeWalletProvider.overrideWith(_ActiveWallet.new),
      decoyModeProvider.overrideWith(_NormalMode.new),
      activeWalletMetaProvider.overrideWithValue(_walletMeta),
      walletsProvider.overrideWith((ref) async => const [_walletMeta]),
      balanceStreamProvider.overrideWith(
        (ref) => Stream.value(
          Balance(
            total: BigInt.from(15000000000),
            spendable: BigInt.from(15000000000),
            pending: BigInt.zero,
          ),
        ),
      ),
      syncProgressStreamProvider.overrideWith((ref) => Stream.value(status)),
      syncStatusProvider.overrideWith((ref) async => status),
      transactionsProvider.overrideWith((ref) async => <TxInfo>[]),
      transactionStreamProvider.overrideWith((ref) => const Stream.empty()),
      transactionWatcherProvider.overrideWith((ref) {}),
      syncCompletionWatcherProvider.overrideWith((ref) {}),
      autoRotationWatcherProvider.overrideWith((ref) {}),
      syncCompletionRotationWatcherProvider.overrideWith((ref) {}),
      walletInitRotationWatcherProvider.overrideWith((ref) {}),
      kdfSwapWarmupProvider.overrideWith((ref) {}),
      appThemeModeProvider.overrideWith(_DarkThemeMode.new),
      arrrPriceQuoteProvider.overrideWith(
        (ref) => Stream.value(
          ArrrPriceQuote(
            currency: CurrencyPreference.usd,
            pricePerArrr: 0.25,
            fetchedAt: DateTime.now(),
            source: ArrrPriceSource.coingecko,
          ),
        ),
      ),
      decoySyncHeightProvider.overrideWith((ref) async => 0),
      tunnelModeProvider.overrideWith(_DirectTunnelMode.new),
      torStatusProvider.overrideWith(_ReadyTorStatus.new),
      transportConfigProvider.overrideWith(_TorTransport.new),
      connectionStatusLevelProvider.overrideWithValue(
        ConnectionStatusLevel.secure,
      ),
      lightdEndpointConfigProvider.overrideWith(
        (ref) async =>
            const LightdEndpointConfig(url: 'https://lightd1.pirate.black:443'),
      ),
    ],
    child: MaterialApp(
      debugShowCheckedModeBanner: false,
      theme: PTheme.dark(),
      builder: (context, page) =>
          RepaintBoundary(key: _captureBoundaryKey, child: page),
      home: child,
    ),
  );
}

Future<void> _capture(
  WidgetTester tester, {
  required Size size,
  required String filename,
  required Widget widget,
  TargetPlatform platform = TargetPlatform.android,
  Future<void> Function(WidgetTester tester)? interact,
}) async {
  debugDefaultTargetPlatformOverride = platform;
  tester.view.physicalSize = size * 3;
  tester.view.devicePixelRatio = 3;
  await tester.pumpWidget(widget);
  await tester.pump(const Duration(milliseconds: 900));
  // Let short data-arrival transitions finish before taking a still image.
  await tester.pump(const Duration(milliseconds: 250));
  if (interact != null) {
    await interact(tester);
    await tester.pumpAndSettle();
  }

  final outputDirectory = Platform.environment['PIRATE_UI_CAPTURE_DIR'];
  if (outputDirectory != null && outputDirectory.isNotEmpty) {
    final path = '$outputDirectory${Platform.pathSeparator}$filename';
    final boundary = tester.renderObject<RenderRepaintBoundary>(
      find.byKey(_captureBoundaryKey),
    );
    await tester.runAsync(() async {
      final image = await boundary.toImage(pixelRatio: 3);
      final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
      await File(path).writeAsBytes(bytes!.buffer.asUint8List());
      image.dispose();
    });
  }

  expect(tester.takeException(), isNull);
  await tester.pumpWidget(const SizedBox.shrink());
  await tester.pump(const Duration(milliseconds: 1));
  debugDefaultTargetPlatformOverride = null;
  tester.view.reset();
}

void main() {
  setUpAll(() async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
          const MethodChannel('plugins.it_nomads.com/flutter_secure_storage'),
          (call) async => null,
        );
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
          const MethodChannel('plugins.flutter.io/local_auth'),
          (call) async {
            if (call.method == 'getAvailableBiometrics') {
              return <String>[];
            }
            return false;
          },
        );

    final sora = FontLoader('Sora')
      ..addFont(rootBundle.load('assets/fonts/Sora/Sora.ttf'));
    await sora.load();
    final monospace = FontLoader(
      'JetBrainsMono',
    )..addFont(rootBundle.load('assets/fonts/JetBrainsMono/JetBrainsMono.ttf'));
    await monospace.load();

    final materialIconsPath =
        Platform.environment['PIRATE_MATERIAL_ICONS_FONT'];
    if (materialIconsPath != null && File(materialIconsPath).existsSync()) {
      final materialIcons = FontLoader('MaterialIcons')
        ..addFont(
          File(materialIconsPath).readAsBytes().then(ByteData.sublistView),
        );
      await materialIcons.load();
    }
  });

  final api = RestoredWalletApi();
  setUpAll(() {
    RustLib.initMock(api: api);
    api.addresses[2] = 'zs1samplerestoredaccount2addressnotforpayments';
    api.addresses[3] = 'zs1samplerestoredaccount3addressnotforpayments';
  });
  testWidgets('restored keys are visible on the management page', (
    tester,
  ) async {
    api.visibleKeyCount = 1;
    await _capture(
      tester,
      size: const Size(430, 932),
      filename: '01-keys.png',
      widget: _walletApp(const KeyManagementScreen()),
      interact: (tester) async {
        expect(find.text('Seed account 1'), findsNothing);
        api.visibleKeyCount = 3;
        final readsBefore = api.keyListReads;
        await tester.pump(const Duration(seconds: 5));
        await tester.pumpAndSettle();
        expect(api.keyListReads, greaterThan(readsBefore));
        await tester.pump(const Duration(milliseconds: 100));
        await tester.pumpAndSettle();
        expect(find.text('Seed account 1'), findsOneWidget);
        expect(find.text('Seed account 2'), findsOneWidget);
      },
    );
  });
  testWidgets('receive filters restored seed account addresses', (
    tester,
  ) async {
    await _capture(
      tester,
      size: const Size(430, 932),
      filename: '02-receive-filter.png',
      widget: _walletApp(const ReceiveScreen()),
      interact: (tester) async {
        await tester.drag(
          find.byType(CustomScrollView).first,
          const Offset(0, -670),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.byType(DropdownButtonFormField<int>));
        await tester.pumpAndSettle();
        await tester.tap(find.text('Seed account 1').last);
        await tester.pumpAndSettle();
        expect(
          tester
              .widget<DropdownButtonFormField<int>>(
                find.byType(DropdownButtonFormField<int>),
              )
              .initialValue,
          2,
        );
        await tester.drag(
          find.byType(CustomScrollView).first,
          const Offset(0, -600),
        );
        await tester.pumpAndSettle();
        expect(
          tester
              .widget<AddressHistorySliver>(find.byType(AddressHistorySliver))
              .addresses
              .map((a) => a.keyId),
          [2],
        );
        expect(find.byKey(const ValueKey('address-history-2')), findsOneWidget);
        expect(find.byKey(const ValueKey('address-history-3')), findsNothing);
      },
    );
  });
  testWidgets('account recovery explains the scan and disables additions', (
    tester,
  ) async {
    await _capture(
      tester,
      size: const Size(430, 932),
      filename: '04-account-recovery.png',
      widget: _walletApp(const KeyManagementScreen(), scanning: true),
      interact: (tester) async {
        await tester.ensureVisible(
          find.byKey(KeyManagementScreen.seedAccountsCardKey),
        );
        await tester.pumpAndSettle();
        expect(
          find.text(
            'Scanning restored accounts. You can add more when the scan finishes.',
          ),
          findsOneWidget,
        );
        final buttons = tester.widgetList<PButton>(find.byType(PButton));
        expect(buttons.where((b) => b.onPressed != null), isEmpty);
      },
    );
  });
  testWidgets(
    'send includes restored accounts in automatic and manual sources',
    (tester) async {
      await _capture(
        tester,
        size: const Size(820, 1100),
        filename: '03-send-sources.png',
        widget: _walletApp(const SendScreen()),
        interact: (tester) async {
          await tester.tap(find.text('Spend from').first);
          await tester.pumpAndSettle();
          expect(find.text('Auto (all keys)'), findsWidgets);
          expect(find.text('Seed account 1'), findsWidgets);
          expect(find.text('Seed account 2'), findsWidgets);
        },
      );
    },
  );
}
