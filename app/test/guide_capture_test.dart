import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart'
    hide AddressInfo, NodeTestResult;
import 'package:pirate_wallet/core/providers/connection_status_provider.dart';
import 'package:pirate_wallet/core/providers/price_providers.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/core/services/address_rotation_service.dart';
import 'package:pirate_wallet/core/security/decoy_data.dart';
import 'package:pirate_wallet/core/swaps/swap_providers.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/features/app_shell/app_shell.dart';
import 'package:pirate_wallet/features/activity/activity_screen.dart';
import 'package:pirate_wallet/features/activity/transaction_detail_screen.dart';
import 'package:pirate_wallet/features/home/home_screen.dart';
import 'package:pirate_wallet/features/keys/import_spending_key_screen.dart';
import 'package:pirate_wallet/features/keys/keys_screen.dart';
import 'package:pirate_wallet/features/onboarding/screens/backup_warning_screen.dart';
import 'package:pirate_wallet/features/onboarding/screens/birthday_picker_screen.dart';
import 'package:pirate_wallet/features/onboarding/screens/create_or_import_screen.dart';
import 'package:pirate_wallet/features/onboarding/screens/ivk_import_screen.dart';
import 'package:pirate_wallet/features/onboarding/screens/seed_confirm_screen.dart';
import 'package:pirate_wallet/features/onboarding/screens/seed_display_screen.dart';
import 'package:pirate_wallet/features/onboarding/screens/seed_import_screen.dart';
import 'package:pirate_wallet/features/onboarding/screens/welcome_screen.dart';
import 'package:pirate_wallet/features/onboarding/onboarding_flow.dart';
import 'package:pirate_wallet/features/pay/pay_screen.dart';
import 'package:pirate_wallet/features/receive/receive_screen.dart';
import 'package:pirate_wallet/features/receive/receive_viewmodel.dart';
import 'package:pirate_wallet/features/send/send_screen.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';
import 'package:pirate_wallet/features/settings/screens/birthday_height_screen.dart';
import 'package:pirate_wallet/features/settings/screens/node_settings_screen.dart';
import 'package:pirate_wallet/features/settings/screens/outbound_api_screen.dart';
import 'package:pirate_wallet/features/settings/screens/privacy_shield_screen.dart';
import 'package:pirate_wallet/features/settings/settings_screen.dart';

const _captureBoundaryKey = ValueKey('guide-capture-boundary');

class _ActiveWallet extends ActiveWalletNotifier {
  @override
  String? build() => 'guide-wallet';
}

class _NormalMode extends DecoyModeNotifier {
  @override
  bool build() => false;
}

class _GuideOnboardingController extends OnboardingController {
  _GuideOnboardingController(this.initialState);

  final OnboardingState initialState;

  @override
  OnboardingState build() => initialState;
}

class _GuideReceiveViewModel extends ReceiveViewModel {
  @override
  ReceiveState build() => ReceiveState(
    currentAddress: 'zs1stashi9x4y0ku3z7g5m2r8e6p0q4w7t9n3c5v8b2x6a0s4d7f9h3j5k8l2p6q0w4e7r9t',
    addressHistory: [
      AddressInfo(
        addressId: 2,
        address:
            'zs1savings6u4x8m2p9r5w3t7y0q4e8k2n6c9v3b7a5d1f8h4j0l6s2g9z5x3m7p',
        label: 'Savings',
        createdAt: DateTime(2026, 8, 28),
        diversifierIndex: 2,
        balance: BigInt.from(72500000000),
      ),
      AddressInfo(
        addressId: 1,
        address:
            'zs1payments3m8x5q2w9e6r4t7y0u1i5o8p3a6s9d2f4g7h0j5k8l1z6x9c2v4b7n',
        label: 'Payments',
        createdAt: DateTime(2026, 8, 20),
        diversifierIndex: 1,
        wasUsedForReceive: true,
      ),
    ],
    diversifierIndex: 3,
  );
}

class _DirectTunnelMode extends TunnelModeNotifier {
  @override
  TunnelMode build() => const TunnelMode.direct();
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
  id: 'guide-wallet',
  name: 'My ARRR Wallet 1',
  createdAt: 1787961600,
  watchOnly: false,
  birthdayHeight: 3500000,
  networkType: 'mainnet',
);

const _guideMnemonic =
    'abandon abandon abandon abandon abandon abandon abandon abandon '
    'abandon abandon abandon abandon abandon abandon abandon abandon '
    'abandon abandon abandon abandon abandon abandon abandon art';

final _guideTransactions = [
  TxInfo(
    txid: 'e89060e026faec5713f9fbdcb80647fc9c13815ebcdafff052d8c225545bddff',
    height: 4111812,
    timestamp: DateTime(2026, 8, 30).millisecondsSinceEpoch ~/ 1000,
    amount: 2200000000000,
    fee: BigInt.zero,
    memo: 'Treasure Chest migration',
    confirmed: true,
    expired: false,
  ),
  TxInfo(
    txid: '75f2860f6bc123de772e795ec561adb44492c87e73f2a76d204955c046f9efad',
    height: 4111020,
    timestamp: DateTime(2026, 8, 27).millisecondsSinceEpoch ~/ 1000,
    amount: -12500000000,
    fee: BigInt.from(10000),
    memo: 'Invoice 1042',
    confirmed: true,
    expired: false,
  ),
];

class _GuideActivityHistory extends ActivityHistoryNotifier {
  @override
  Future<ActivityHistoryState> build() async => ActivityHistoryState(
    transactions: List.unmodifiable(_guideTransactions),
    nextCursor: null,
  );
}

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

Widget _walletApp(
  Widget child, {
  bool includeReceiveState = false,
  bool includeAppVersion = false,
}) {
  return ProviderScope(
    overrides: [
      activeWalletProvider.overrideWith(_ActiveWallet.new),
      decoyModeProvider.overrideWith(_NormalMode.new),
      activeWalletMetaProvider.overrideWithValue(_walletMeta),
      walletsProvider.overrideWith((ref) async => const [_walletMeta]),
      balanceStreamProvider.overrideWith(
        (ref) => Stream.value(
          Balance(
            total: BigInt.from(2247523456789),
            spendable: BigInt.from(2247523456789),
            pending: BigInt.zero,
          ),
        ),
      ),
      syncProgressStreamProvider.overrideWith(
        (ref) => Stream.value(_syncedStatus),
      ),
      syncStatusProvider.overrideWith((ref) async => _syncedStatus),
      transactionsProvider.overrideWith((ref) async => _guideTransactions),
      activityHistoryProvider.overrideWith(_GuideActivityHistory.new),
      transactionStreamProvider.overrideWith((ref) => const Stream.empty()),
      transactionWatcherProvider.overrideWith((ref) {}),
      syncCompletionWatcherProvider.overrideWith((ref) {}),
      autoRotationWatcherProvider.overrideWith((ref) {}),
      syncCompletionRotationWatcherProvider.overrideWith((ref) {}),
      walletInitRotationWatcherProvider.overrideWith((ref) {}),
      kdfSwapWarmupProvider.overrideWith((ref) {}),
      appThemeModeProvider.overrideWith(_DarkThemeMode.new),
      arrrPriceQuoteProvider.overrideWith((ref) => Stream.value(null)),
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
      if (includeReceiveState)
        receiveViewModelProvider.overrideWith(_GuideReceiveViewModel.new),
      if (includeAppVersion)
        appVersionProvider.overrideWith((ref) async => 'v1.1.9'),
    ],
    child: MaterialApp(
      debugShowCheckedModeBanner: false,
      theme: PTheme.dark(),
      home: RepaintBoundary(key: _captureBoundaryKey, child: child),
    ),
  );
}

Widget _redactedReceiveScreen() {
  return LayoutBuilder(
    builder: (context, constraints) {
      final isDesktop = constraints.maxWidth >= 800;

      Widget blurRegion({
        required double left,
        required double top,
        required double width,
        required double height,
        required double radius,
      }) {
        return Positioned(
          left: left,
          top: top,
          width: width,
          height: height,
          child: IgnorePointer(
            child: ClipRRect(
              borderRadius: BorderRadius.circular(8),
              child: BackdropFilter(
                filter: ui.ImageFilter.blur(sigmaX: radius, sigmaY: radius),
                child: ColoredBox(color: Colors.black.withValues(alpha: 0.18)),
              ),
            ),
          ),
        );
      }

      return Stack(
        fit: StackFit.expand,
        children: [
          const ReceiveScreen(),
          blurRegion(
            left: isDesktop ? 586 : 141,
            top: 235,
            width: 108,
            height: 108,
            radius: 14,
          ),
          blurRegion(
            left: isDesktop ? 377 : 77,
            top: isDesktop ? 458 : 463,
            width: isDesktop ? 526 : 236,
            height: isDesktop ? 34 : 62,
            radius: 12,
          ),
        ],
      );
    },
  );
}

Widget _app(Widget child) {
  return ProviderScope(
    child: MaterialApp(
      debugShowCheckedModeBanner: false,
      theme: PTheme.dark(),
      home: RepaintBoundary(key: _captureBoundaryKey, child: child),
    ),
  );
}

Widget _onboardingApp(Widget child, OnboardingState state) {
  return ProviderScope(
    overrides: [
      onboardingControllerProvider.overrideWith(
        () => _GuideOnboardingController(state),
      ),
      walletsProvider.overrideWith((ref) async => const <WalletMeta>[]),
    ],
    child: MaterialApp(
      debugShowCheckedModeBanner: false,
      theme: PTheme.dark(),
      home: RepaintBoundary(key: _captureBoundaryKey, child: child),
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
  bool captureOverlay = false,
}) async {
  debugDefaultTargetPlatformOverride = platform;
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
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
    await expectLater(
      captureOverlay
          ? find.byType(Overlay).first
          : find.byKey(_captureBoundaryKey),
      matchesGoldenFile(Uri.file(path)),
    );
  }

  expect(tester.takeException(), isNull);
  await tester.pumpWidget(const SizedBox.shrink());
  await tester.pump(const Duration(milliseconds: 1));
  debugDefaultTargetPlatformOverride = null;
  tester.view.reset();
}

Future<void> _captureWelcome(
  WidgetTester tester, {
  required Size size,
  required String filename,
  required TargetPlatform platform,
}) async {
  debugDefaultTargetPlatformOverride = platform;
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  final router = GoRouter(
    initialLocation: '/onboarding/welcome',
    routes: [
      GoRoute(
        path: '/onboarding/welcome',
        builder: (context, state) => const WelcomeScreen(),
      ),
    ],
  );
  await tester.pumpWidget(
    ProviderScope(
      child: MaterialApp.router(
        debugShowCheckedModeBanner: false,
        theme: PTheme.dark(),
        routerConfig: router,
        builder: (context, child) => RepaintBoundary(
          key: _captureBoundaryKey,
          child: child ?? const SizedBox.shrink(),
        ),
      ),
    ),
  );
  await tester.runAsync(() async {
    await precacheImage(
      const AssetImage('assets/icons/stashi-wallet-logo.png'),
      tester.element(find.byType(WelcomeScreen)),
    );
  });
  await tester.pumpAndSettle();

  final logo = find.byWidgetPredicate(
    (widget) =>
        widget is Image &&
        widget.image is AssetImage &&
        (widget.image as AssetImage).assetName ==
            'assets/icons/stashi-wallet-logo.png',
  );
  expect(logo, findsOneWidget);
  expect(tester.getSize(logo).isEmpty, isFalse);
  final logoTopLeft = tester.getTopLeft(logo);
  expect(logoTopLeft.dx, greaterThanOrEqualTo(0));
  expect(logoTopLeft.dy, greaterThanOrEqualTo(0));
  expect(logoTopLeft.dx, lessThan(size.width));
  expect(logoTopLeft.dy, lessThan(size.height));

  final outputDirectory = Platform.environment['PIRATE_UI_CAPTURE_DIR'];
  if (outputDirectory != null && outputDirectory.isNotEmpty) {
    final path = '$outputDirectory${Platform.pathSeparator}$filename';
    await expectLater(
      find.byKey(_captureBoundaryKey),
      matchesGoldenFile(Uri.file(path)),
    );
  }
  expect(tester.takeException(), isNull);
  router.dispose();
  await tester.pumpWidget(const SizedBox.shrink());
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

  tearDown(() {
    debugDefaultTargetPlatformOverride = null;
  });

  tearDownAll(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
          const MethodChannel('plugins.it_nomads.com/flutter_secure_storage'),
          null,
        );
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(
          const MethodChannel('plugins.flutter.io/local_auth'),
          null,
        );
  });

  testWidgets('captures onboarding on phone and desktop', (tester) async {
    await _captureWelcome(
      tester,
      size: const Size(1280, 900),
      filename: 'welcome-desktop.png',
      platform: TargetPlatform.windows,
    );
    await _captureWelcome(
      tester,
      size: const Size(390, 844),
      filename: 'welcome-phone.png',
      platform: TargetPlatform.android,
    );
  });

  testWidgets('captures home on phone and desktop', (tester) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'home-phone.png',
      widget: _walletApp(const Scaffold(body: HomeScreen(useScaffold: false))),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'home-desktop.png',
      widget: _walletApp(const Scaffold(body: HomeScreen(useScaffold: false))),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures wallet hub on phone and desktop', (tester) async {
    final sheet = PaySheet(
      onSend: () {},
      onReceive: () {},
      onVerify: () {},
      onSwap: () {},
    );
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'pay-phone.png',
      widget: _app(Scaffold(body: sheet)),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'pay-desktop.png',
      widget: _app(
        Scaffold(
          body: Center(child: SizedBox(width: 780, child: sheet)),
        ),
      ),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures send on phone and desktop', (tester) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'send-phone.png',
      widget: _walletApp(const SendScreen()),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'send-desktop.png',
      widget: _walletApp(const SendScreen()),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures receive on phone and desktop', (tester) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'receive-phone.png',
      widget: _walletApp(_redactedReceiveScreen(), includeReceiveState: true),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'receive-desktop.png',
      widget: _walletApp(_redactedReceiveScreen(), includeReceiveState: true),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures key management on phone and desktop', (tester) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'keys-phone.png',
      widget: _walletApp(
        KeyManagementScreen(keyLoader: (_) async => DecoyData.keyGroups()),
      ),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'keys-desktop.png',
      widget: _walletApp(
        KeyManagementScreen(keyLoader: (_) async => DecoyData.keyGroups()),
      ),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures network privacy on phone and desktop', (tester) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'network-privacy-phone.png',
      widget: _walletApp(const PrivacyShieldScreen()),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'network-privacy-desktop.png',
      widget: _walletApp(const PrivacyShieldScreen()),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures settings on phone and desktop', (tester) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'settings-phone.png',
      widget: _walletApp(const SettingsScreen(), includeAppVersion: true),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'settings-desktop.png',
      widget: _walletApp(const SettingsScreen(), includeAppVersion: true),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures wallet setup choices on phone and desktop', (
    tester,
  ) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'setup-choices-phone.png',
      widget: _app(const CreateOrImportScreen()),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'setup-choices-desktop.png',
      widget: _app(const CreateOrImportScreen()),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures seed import on phone and desktop', (tester) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'seed-import-phone.png',
      widget: _app(const SeedImportScreen()),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'seed-import-desktop.png',
      widget: _app(const SeedImportScreen()),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures backup warning on phone and desktop', (tester) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'backup-warning-phone.png',
      widget: _app(const BackupWarningScreen()),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'backup-warning-desktop.png',
      widget: _app(const BackupWarningScreen()),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures recovery phrase language on phone and desktop', (
    tester,
  ) async {
    const state = OnboardingState(
      currentStep: OnboardingStep.seedDisplay,
      mode: OnboardingMode.create,
      mnemonic: _guideMnemonic,
      mnemonicLanguage: MnemonicLanguage.english,
    );
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'seed-display-phone.png',
      widget: _onboardingApp(const SeedDisplayScreen(), state),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'seed-display-desktop.png',
      widget: _onboardingApp(const SeedDisplayScreen(), state),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures wallet naming and seed confirmation', (tester) async {
    const state = OnboardingState(
      currentStep: OnboardingStep.seedConfirm,
      mode: OnboardingMode.create,
      mnemonic: _guideMnemonic,
      mnemonicLanguage: MnemonicLanguage.english,
    );
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'seed-confirm-phone.png',
      widget: _onboardingApp(const SeedConfirmScreen(), state),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'seed-confirm-desktop.png',
      widget: _onboardingApp(const SeedConfirmScreen(), state),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures restore naming and birthday selection', (tester) async {
    const state = OnboardingState(
      currentStep: OnboardingStep.birthdayPicker,
      mode: OnboardingMode.import,
      mnemonic: _guideMnemonic,
      mnemonicLanguage: MnemonicLanguage.english,
      passphrase: 'local-only-test-passphrase',
    );
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'wallet-birthday-phone.png',
      widget: _onboardingApp(const BirthdayPickerScreen(), state),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'wallet-birthday-desktop.png',
      widget: _onboardingApp(const BirthdayPickerScreen(), state),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures compact Ubuntu laptop layouts', (tester) async {
    await _capture(
      tester,
      size: const Size(1097, 706),
      filename: 'home-laptop.png',
      widget: _walletApp(
        const AppShell(
          location: '/home',
          child: HomeScreen(useScaffold: false),
        ),
      ),
      platform: TargetPlatform.linux,
    );
    await _capture(
      tester,
      size: const Size(1097, 706),
      filename: 'wallets-laptop.png',
      widget: _walletApp(
        const AppShell(location: '/pay', child: PayScreen(useScaffold: false)),
      ),
      platform: TargetPlatform.linux,
    );
  });

  testWidgets('captures activity on phone and desktop', (tester) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'activity-phone.png',
      widget: _walletApp(const ActivityScreen()),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'activity-desktop.png',
      widget: _walletApp(const ActivityScreen()),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures transaction details on phone and desktop', (
    tester,
  ) async {
    final details = TransactionDetailScreen(
      txid: _guideTransactions.first.txid,
      transaction: _guideTransactions.first,
    );
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'transaction-details-phone.png',
      widget: _walletApp(details),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'transaction-details-desktop.png',
      widget: _walletApp(details),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures seed account help on phone and desktop', (
    tester,
  ) async {
    Future<void> openHelp(WidgetTester tester) async {
      await tester.tap(find.text('How seed accounts work'));
    }

    Widget keys() => _walletApp(
      KeyManagementScreen(keyLoader: (_) async => DecoyData.keyGroups()),
    );
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'seed-account-help-phone.png',
      widget: keys(),
      interact: openHelp,
      captureOverlay: true,
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'seed-account-help-desktop.png',
      widget: keys(),
      platform: TargetPlatform.windows,
      interact: openHelp,
      captureOverlay: true,
    );
  });

  testWidgets('captures spending-key import on phone and desktop', (
    tester,
  ) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'spending-key-import-phone.png',
      widget: _walletApp(const ImportSpendingKeyScreen()),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'spending-key-import-desktop.png',
      widget: _walletApp(const ImportSpendingKeyScreen()),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures view-only wallet fields on mobile and desktop', (
    tester,
  ) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'view-only-wallet-mobile.png',
      widget: _walletApp(const ViewingKeysImportScreen()),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'view-only-wallet-desktop.png',
      widget: _walletApp(const ViewingKeysImportScreen()),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures viewing-key import on mobile and desktop', (
    tester,
  ) async {
    Future<void> openViewingKeyImport(WidgetTester tester) async {
      await tester.tap(find.text('Viewing Key'));
    }

    Widget keys() => _walletApp(
      KeyManagementScreen(keyLoader: (_) async => DecoyData.keyGroups()),
    );
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'viewing-key-import-mobile.png',
      widget: keys(),
      interact: openViewingKeyImport,
      captureOverlay: true,
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'viewing-key-import-desktop.png',
      widget: keys(),
      platform: TargetPlatform.windows,
      interact: openViewingKeyImport,
      captureOverlay: true,
    );
  });

  testWidgets('captures node selection on phone and desktop', (tester) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'node-selection-phone.png',
      widget: _walletApp(const NodeSettingsScreen()),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'node-selection-desktop.png',
      widget: _walletApp(const NodeSettingsScreen()),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures outbound API controls on phone and desktop', (
    tester,
  ) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'outbound-apis-phone.png',
      widget: _walletApp(const OutboundApiScreen()),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'outbound-apis-desktop.png',
      widget: _walletApp(const OutboundApiScreen()),
      platform: TargetPlatform.windows,
    );
  });

  testWidgets('captures birthday height on phone and desktop', (tester) async {
    await _capture(
      tester,
      size: const Size(390, 844),
      filename: 'birthday-height-phone.png',
      widget: _walletApp(BirthdayHeightScreen(nodeTester: _guideNodeTester)),
    );
    await _capture(
      tester,
      size: const Size(1280, 900),
      filename: 'birthday-height-desktop.png',
      widget: _walletApp(BirthdayHeightScreen(nodeTester: _guideNodeTester)),
      platform: TargetPlatform.windows,
    );
  });
}

Future<NodeTestResult> _guideNodeTester({
  required String url,
  String? tlsPin,
}) async => NodeTestResult(
  success: true,
  latestBlockHeight: 4111871,
  transportMode: 'tor',
  tlsEnabled: true,
  tlsPinMatched: true,
  responseTimeMs: 86,
  serverVersion: 'lightwalletd',
  chainName: 'main',
);
