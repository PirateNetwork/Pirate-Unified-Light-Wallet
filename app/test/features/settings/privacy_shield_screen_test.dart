import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/providers/connection_status_provider.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';
import 'package:pirate_wallet/features/settings/screens/privacy_shield_screen.dart';

class _TorTransportNotifier extends TransportConfigNotifier {
  @override
  TransportConfig build() => const TransportConfig(
    mode: 'tor',
    dnsProvider: 'cloudflare_doh',
    socks5Config: <String, String?>{},
    i2pEndpoint: 'http://rud5qc4s4tsjzuhzygzdweoorhofbgobo7zuo7qeor25oyqonitq.b32.i2p:9067',
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
}
