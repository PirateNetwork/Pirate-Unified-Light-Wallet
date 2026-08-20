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

  testWidgets(
    'shows Auto and curated clearnet presets without retired servers',
    (tester) async {
      tester.view.physicalSize = const Size(390, 844);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);

      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            lightdEndpointConfigProvider.overrideWith(
              (ref) async => const LightdEndpointConfig(
                url: 'https://lightd1.pirate.black:443',
                automaticFailover: true,
              ),
            ),
            transportConfigProvider.overrideWith(
              _TestTransportConfigNotifier.new,
            ),
            endpointHealthProvider.overrideWith(
              _TestEndpointHealthNotifier.new,
            ),
          ],
          child: const MaterialApp(home: NodeSettingsScreen()),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.textContaining('64.23.167.130'), findsNothing);
      expect(find.text('Automatic server selection'), findsOneWidget);
      expect(find.text('Auto (Mainnet)'), findsOneWidget);
      expect(find.text('Pirate Chain Mainnet'), findsOneWidget);
      expect(find.text('Pirate Black Mainnet'), findsOneWidget);
      expect(find.text('Qortal 1 Mainnet'), findsOneWidget);
      expect(find.text('Qortal 2 Mainnet'), findsOneWidget);
      expect(find.text('Qortal 3 Mainnet'), findsOneWidget);
      expect(find.text('CryptoForge 1 Mainnet'), findsOneWidget);
      expect(find.text('CryptoForge 2 Mainnet'), findsOneWidget);
      expect(find.text('Mathnodes Mainnet'), findsOneWidget);
      expect(find.text('Auto (Ironwood testnet)'), findsOneWidget);
      expect(find.text('CryptoForge 1 Ironwood testnet'), findsOneWidget);
      expect(find.text('CryptoForge 2 Ironwood testnet'), findsOneWidget);
      expect(find.textContaining('.onion'), findsNothing);
      expect(find.textContaining('.b32.i2p'), findsNothing);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('shows only the official I2P pools in I2P mode', (tester) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          lightdEndpointConfigProvider.overrideWith(
            (ref) async => const LightdEndpointConfig(
              url: 'http://5vjlbxmzx4gjfuwcot2qtfjdnxodzpe4jsw3ckx7i4maltz7j5qa.b32.i2p:9067',
              automaticFailover: true,
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
    expect(find.text('Auto (Mainnet)'), findsOneWidget);
    expect(find.text('Auto (Ironwood testnet)'), findsOneWidget);
    expect(find.text('Mathnodes Mainnet'), findsNothing);
    expect(find.textContaining('.onion'), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
