import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/features/settings/providers/endpoint_health_provider.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';
import 'package:pirate_wallet/features/settings/screens/node_settings_screen.dart';
import 'package:pirate_wallet/ui/atoms/p_input.dart';

class _TestTransportConfigNotifier extends TransportConfigNotifier {
  @override
  TransportConfig build() => const TransportConfig(
    mode: 'direct',
    dnsProvider: 'cloudflare_doh',
    socks5Config: <String, String?>{},
    i2pEndpoint: '',
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

class _TestEndpointHealthNotifier extends EndpointHealthNotifier {
  @override
  EndpointHealthState build() => const EndpointHealthState.idle();
}

class _TestI2pTransportConfigNotifier extends TransportConfigNotifier {
  @override
  TransportConfig build() => const TransportConfig(
    mode: 'i2p',
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
  testWidgets('keeps the TLS pin control compact and vertically aligned', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          lightdEndpointConfigProvider.overrideWith(
            (ref) async =>
                const LightdEndpointConfig(url: 'https://lightd.example:443'),
          ),
          transportConfigProvider.overrideWith(
            _TestTransportConfigNotifier.new,
          ),
          endpointHealthProvider.overrideWith(_TestEndpointHealthNotifier.new),
        ],
        child: const MaterialApp(home: NodeSettingsScreen()),
      ),
    );
    await tester.pumpAndSettle();

    final pinFinder = find.byKey(const ValueKey('tls-pin-input'));
    await tester.scrollUntilVisible(
      pinFinder,
      240,
      scrollable: find.byType(Scrollable).first,
    );
    await tester.pumpAndSettle();

    final pinInput = tester.widget<PInput>(pinFinder);
    expect(pinInput.maxLines, 1);
    expect(pinInput.helperText, 'Leave empty to skip certificate pinning');
    expect(pinInput.hint, isNull);
    expect(pinInput.monospace, isTrue);
    expect(pinInput.autocorrect, isFalse);
    expect(pinInput.enableSuggestions, isFalse);
    expect(pinInput.keyboardType, TextInputType.visiblePassword);
    expect(pinInput.textInputAction, TextInputAction.done);

    final textFieldFinder = find.descendant(
      of: pinFinder,
      matching: find.byType(TextField),
    );
    final textField = tester.widget<TextField>(textFieldFinder);
    expect(textField.textAlignVertical, TextAlignVertical.center);
    expect(tester.getSize(textFieldFinder).height, lessThanOrEqualTo(64));

    final labelFinder = find.descendant(
      of: pinFinder,
      matching: find.text('SPKI Pin (base64)'),
    );
    final helperFinder = find.descendant(
      of: pinFinder,
      matching: find.text('Leave empty to skip certificate pinning'),
    );
    final leftEdge = tester.getTopLeft(textFieldFinder).dx;
    expect(tester.getTopLeft(labelFinder).dx, leftEdge);
    expect(tester.getTopLeft(helperFinder).dx, leftEdge);
  });

  testWidgets('keeps endpoint fields usable in phone landscape', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(844, 390);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          lightdEndpointConfigProvider.overrideWith(
            (ref) async =>
                const LightdEndpointConfig(url: 'https://lightd.example:443'),
          ),
          transportConfigProvider.overrideWith(
            _TestTransportConfigNotifier.new,
          ),
          endpointHealthProvider.overrideWith(_TestEndpointHealthNotifier.new),
        ],
        child: const MaterialApp(home: NodeSettingsScreen()),
      ),
    );
    await tester.pumpAndSettle();

    final endpoint = find.text('Endpoint (host:port)');
    expect(endpoint, findsOneWidget);
    expect(find.text('Choose your lightwalletd endpoint'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('shows all clearnet presets without hidden-service overflow', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          lightdEndpointConfigProvider.overrideWith(
            (ref) async =>
                const LightdEndpointConfig(url: 'http://64.23.167.130:9067'),
          ),
          transportConfigProvider.overrideWith(
            _TestTransportConfigNotifier.new,
          ),
          endpointHealthProvider.overrideWith(_TestEndpointHealthNotifier.new),
        ],
        child: const MaterialApp(home: NodeSettingsScreen()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('64.23.167.130:9067'), findsWidgets);
    expect(find.text('lightd1.pirate.black:443'), findsOneWidget);
    expect(find.text('pirate.mathnodes.com:443'), findsOneWidget);
    expect(find.text('Dev server Mainnet (no TLS)'), findsOneWidget);
    expect(find.text('Pirate Chain Mainnet'), findsOneWidget);
    expect(find.text('Mathnodes Mainnet'), findsOneWidget);
    expect(find.textContaining('.onion'), findsNothing);
    expect(find.textContaining('.b32.i2p'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('shows only the reachable hidden service in I2P mode', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          lightdEndpointConfigProvider.overrideWith(
            (ref) async => const LightdEndpointConfig(
              url: 'http://rud5qc4s4tsjzuhzygzdweoorhofbgobo7zuo7qeor25oyqonitq.b32.i2p:9067',
            ),
          ),
          transportConfigProvider.overrideWith(
            _TestI2pTransportConfigNotifier.new,
          ),
          endpointHealthProvider.overrideWith(_TestEndpointHealthNotifier.new),
        ],
        child: const MaterialApp(home: NodeSettingsScreen()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.textContaining('.b32.i2p'), findsWidgets);
    expect(find.text('Dev server Mainnet (no TLS)'), findsNothing);
    expect(find.text('Pirate Chain Mainnet'), findsNothing);
    expect(find.text('Mathnodes Mainnet'), findsNothing);
    expect(find.textContaining('.onion'), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
