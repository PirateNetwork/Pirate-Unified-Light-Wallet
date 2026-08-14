import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/core/providers/connection_status_provider.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/design/tokens/spacing.dart';
import 'package:pirate_wallet/features/app_shell/desktop_status_bar.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';
import 'package:pirate_wallet/ui/atoms/p_button.dart';
import 'package:pirate_wallet/ui/atoms/theme_toggle_button.dart';

final _syncStatus = SyncStatus(
  localHeight: BigInt.from(500),
  targetHeight: BigInt.from(1000),
  percent: 50,
  stage: SyncStage.notes,
  blocksPerSecond: 1200,
  notesDecrypted: BigInt.zero,
  lastBatchMs: BigInt.zero,
);

class _TestTransportConfigNotifier extends TransportConfigNotifier {
  @override
  TransportConfig build() => const TransportConfig(
    mode: 'tor',
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

class _TestThemeModeNotifier extends ThemeModeNotifier {
  @override
  AppThemeMode build() => AppThemeMode.dark;
}

Widget _testApp({
  required VoidCallback onSettingsTap,
  required VoidCallback onConnectionTap,
  double textScale = 1,
}) {
  return ProviderScope(
    overrides: [
      connectionStatusLevelProvider.overrideWithValue(
        ConnectionStatusLevel.secure,
      ),
      syncProgressStreamProvider.overrideWith(
        (ref) => Stream.value(_syncStatus),
      ),
      transportConfigProvider.overrideWith(_TestTransportConfigNotifier.new),
      appThemeModeProvider.overrideWith(_TestThemeModeNotifier.new),
    ],
    child: MaterialApp(
      theme: PTheme.dark(),
      home: MediaQuery(
        data: MediaQueryData(textScaler: TextScaler.linear(textScale)),
        child: Scaffold(
          body: Align(
            alignment: Alignment.bottomCenter,
            child: DesktopStatusBar(
              settingsSelected: true,
              onSettingsTap: onSettingsTap,
              onConnectionTap: onConnectionTap,
            ),
          ),
        ),
      ),
    ),
  );
}

void main() {
  testWidgets('shows live desktop state without crowding the header', (
    tester,
  ) async {
    var settingsTaps = 0;
    var connectionTaps = 0;
    tester.view.physicalSize = const Size(1000, 600);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      _testApp(
        onSettingsTap: () => settingsTaps++,
        onConnectionTap: () => connectionTaps++,
      ),
    );
    await tester.pump();

    expect(
      tester.getSize(find.byKey(DesktopStatusBar.barKey)).height,
      PSpacing.desktopStatusBarHeight,
    );
    expect(find.text('Connected - Secure'), findsOneWidget);
    expect(find.text('Tor'), findsOneWidget);
    expect(find.text('Scanning notes'), findsOneWidget);
    expect(find.text('50%'), findsOneWidget);
    expect(
      tester.widget<ThemeToggleButton>(find.byType(ThemeToggleButton)).size,
      PIconButtonSize.compact,
    );

    await tester.tap(find.byKey(DesktopStatusBar.settingsKey));
    await tester.tap(find.byKey(DesktopStatusBar.connectionKey));
    expect(settingsTaps, 1);
    expect(connectionTaps, 1);
  });

  testWidgets('keeps footer telemetry contained at large text sizes', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(960, 600);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      _testApp(onSettingsTap: () {}, onConnectionTap: () {}, textScale: 2),
    );
    await tester.pump();

    expect(tester.takeException(), isNull);
    expect(find.byKey(DesktopStatusBar.progressKey), findsOneWidget);
  });
}
