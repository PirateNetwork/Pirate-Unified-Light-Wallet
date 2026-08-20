import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/providers/connection_status_provider.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';
import 'package:pirate_wallet/features/settings/screens/privacy_shield_screen.dart';

class _TorTransportNotifier extends TransportConfigNotifier {
  @override
  TransportConfig build() => const TransportConfig(
    mode: 'tor',
    dnsProvider: 'cloudflare_doh',
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

class _ReadyTorStatusNotifier extends TorStatusNotifier {
  @override
  TorStatusDetails build() => const TorStatusDetails(status: 'ready');
}

void main() {
  testWidgets('keeps every transport choice readable at phone width', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(360, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          transportConfigProvider.overrideWith(_TorTransportNotifier.new),
          torStatusProvider.overrideWith(_ReadyTorStatusNotifier.new),
          connectionStatusLevelProvider.overrideWithValue(
            ConnectionStatusLevel.secure,
          ),
        ],
        child: const MaterialApp(home: PrivacyShieldScreen()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Tor'), findsOneWidget);
    expect(find.text('I2P'), findsOneWidget);
    expect(find.text('SOCKS5'), findsOneWidget);
    expect(find.text('Direct'), findsOneWidget);
    final exception = tester.takeException();
    expect(
      exception,
      isNull,
      reason: exception is FlutterError
          ? exception.toStringDeep()
          : '$exception',
    );
  });

  testWidgets('gives Tor status a balanced desktop hierarchy', (tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          transportConfigProvider.overrideWith(_TorTransportNotifier.new),
          torStatusProvider.overrideWith(_ReadyTorStatusNotifier.new),
          connectionStatusLevelProvider.overrideWithValue(
            ConnectionStatusLevel.secure,
          ),
        ],
        child: const MaterialApp(home: PrivacyShieldScreen()),
      ),
    );
    await tester.pumpAndSettle();

    final titleRect = tester.getRect(find.text('Tor Status'));
    final descriptionRect = tester.getRect(
      find.text(
        'Tor provides the strongest privacy by routing traffic through multiple relays, making it very difficult to trace.',
      ),
    );
    final statusRect = tester.getRect(find.text('Ready'));
    final actionRect = tester.getRect(find.text('Switch exit node'));
    final routeRect = tester.getRect(
      find.text('Attempting: Direct (no fallback bridges)'),
    );

    expect(statusRect.left, greaterThan(titleRect.right));
    expect(routeRect.left, greaterThan(descriptionRect.left));
    expect((statusRect.center.dy - actionRect.center.dy).abs(), lessThan(8));
    expect(tester.takeException(), isNull);
    debugDefaultTargetPlatformOverride = null;
  });
}
