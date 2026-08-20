import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/ffi/ffi_bridge.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/features/settings/providers/endpoint_health_provider.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';

const _walletId = 'health-test-wallet';

class _TestActiveWalletNotifier extends ActiveWalletNotifier {
  @override
  WalletId? build() => _walletId;
}

class _DirectTransportNotifier extends TransportConfigNotifier {
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

class _ReadyTorStatusNotifier extends TorStatusNotifier {
  @override
  TorStatusDetails build() => const TorStatusDetails(status: 'ready');
}

NodeTestResult _probeResult({
  required bool success,
  int? height,
  String chain = 'main',
}) {
  return NodeTestResult(
    success: success,
    latestBlockHeight: height,
    transportMode: 'direct',
    tlsEnabled: false,
    responseTimeMs: 25,
    chainName: chain,
    errorMessage: success ? null : 'unavailable',
  );
}

ProviderContainer _container({
  required LightdEndpointConfig config,
  required LightdEndpointProbe probe,
  required Future<void> Function({required String url, String? tlsPin})
  setEndpoint,
  LightdEndpointConfig Function()? readConfig,
}) {
  return ProviderContainer(
    overrides: [
      activeWalletProvider.overrideWith(_TestActiveWalletNotifier.new),
      transportConfigProvider.overrideWith(_DirectTransportNotifier.new),
      torStatusProvider.overrideWith(_ReadyTorStatusNotifier.new),
      lightdEndpointConfigProvider.overrideWith(
        (ref) async => readConfig?.call() ?? config,
      ),
      lightdEndpointProbeProvider.overrideWithValue(probe),
      setLightdEndpointProvider.overrideWithValue(setEndpoint),
    ],
  );
}

void main() {
  test('routine checks probe only the selected endpoint', () async {
    final probed = <String>[];
    final container = _container(
      config: const LightdEndpointConfig(url: 'http://64.23.167.130:9067'),
      probe: ({required url, tlsPin}) async {
        probed.add(url);
        return _probeResult(success: true, height: 4090000);
      },
      setEndpoint: ({required url, tlsPin}) async {},
    );
    addTearDown(container.dispose);

    container.read(endpointHealthProvider);
    await container.read(endpointHealthProvider.notifier).checkNow();

    expect(probed, <String>['http://64.23.167.130:9067']);
    expect(
      container.read(endpointHealthProvider).phase,
      EndpointHealthPhase.healthy,
    );
  });

  test('confirmed failures switch to the healthiest curated server', () async {
    final probed = <String>[];
    String? selectedUrl;
    final container = _container(
      config: const LightdEndpointConfig(
        url: 'https://lightd.pirate.black:443',
      ),
      probe: ({required url, tlsPin}) async {
        probed.add(url);
        if (url == 'https://lightd1.pirate.black:443') {
          return _probeResult(success: true, height: 4090010);
        }
        return _probeResult(success: false);
      },
      setEndpoint: ({required url, tlsPin}) async => selectedUrl = url,
    );
    addTearDown(container.dispose);

    container.read(endpointHealthProvider);
    final notifier = container.read(endpointHealthProvider.notifier);
    await notifier.checkNow();
    expect(probed, <String>['https://lightd.pirate.black:443']);
    expect(
      container.read(endpointHealthProvider).phase,
      EndpointHealthPhase.degraded,
    );

    await notifier.checkNow();

    expect(selectedUrl, 'https://lightd1.pirate.black:443');
    expect(probed, contains('https://lightwalletd1.cryptoforge.cc:443'));
    expect(probed, isNot(contains('https://pirate.mathnodes.com:443')));
    expect(
      container.read(endpointHealthProvider).switchedFrom,
      'https://lightd.pirate.black:443',
    );
    expect(container.read(endpointHealthProvider).switchedTo, selectedUrl);
  });

  test('custom endpoints remain under explicit user control', () async {
    final probed = <String>[];
    var switchCount = 0;
    final container = _container(
      config: const LightdEndpointConfig(url: 'https://private.example:443'),
      probe: ({required url, tlsPin}) async {
        probed.add(url);
        return _probeResult(success: false);
      },
      setEndpoint: ({required url, tlsPin}) async => switchCount += 1,
    );
    addTearDown(container.dispose);

    container.read(endpointHealthProvider);
    final notifier = container.read(endpointHealthProvider.notifier);
    await notifier.checkNow();
    await notifier.checkNow();

    expect(probed, <String>[
      'https://private.example:443',
      'https://private.example:443',
    ]);
    expect(switchCount, 0);
    expect(
      container.read(endpointHealthProvider).phase,
      EndpointHealthPhase.offline,
    );
  });

  test('certificate-pinned endpoints never fail over automatically', () async {
    var switchCount = 0;
    final container = _container(
      config: const LightdEndpointConfig(
        url: 'https://lightd1.pirate.black:443',
        tlsPin: 'pinned-certificate',
      ),
      probe: ({required url, tlsPin}) async => _probeResult(success: false),
      setEndpoint: ({required url, tlsPin}) async => switchCount += 1,
    );
    addTearDown(container.dispose);

    container.read(endpointHealthProvider);
    final notifier = container.read(endpointHealthProvider.notifier);
    await notifier.checkNow();
    await notifier.checkNow();

    expect(switchCount, 0);
    expect(
      container.read(endpointHealthProvider).phase,
      EndpointHealthPhase.offline,
    );
  });

  test('an in-flight probe cannot replace a newer endpoint choice', () async {
    var config = const LightdEndpointConfig(url: 'http://64.23.167.130:9067');
    var currentProbeCount = 0;
    var switchCount = 0;
    final delayedProbeStarted = Completer<void>();
    final delayedProbeResult = Completer<NodeTestResult>();
    final container = _container(
      config: config,
      readConfig: () => config,
      probe: ({required url, tlsPin}) async {
        if (url != 'http://64.23.167.130:9067') {
          return _probeResult(success: true, height: 4090010);
        }
        currentProbeCount += 1;
        if (currentProbeCount == 1) return _probeResult(success: false);
        delayedProbeStarted.complete();
        return delayedProbeResult.future;
      },
      setEndpoint: ({required url, tlsPin}) async => switchCount += 1,
    );
    addTearDown(container.dispose);

    container.read(endpointHealthProvider);
    final notifier = container.read(endpointHealthProvider.notifier);
    await notifier.checkNow();

    final staleCheck = notifier.checkNow();
    await delayedProbeStarted.future;
    config = const LightdEndpointConfig(url: 'https://private.example:443');
    container.invalidate(lightdEndpointConfigProvider);
    await container.read(lightdEndpointConfigProvider.future);
    delayedProbeResult.complete(_probeResult(success: false));
    await staleCheck;

    expect(switchCount, 0);
    expect(
      container.read(endpointHealthProvider).activeUrl,
      isNot('https://lightd1.pirate.black:443'),
    );
  });
}
