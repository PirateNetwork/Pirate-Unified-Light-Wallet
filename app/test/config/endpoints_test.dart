import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/config/endpoints.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';

void main() {
  group('lightwalletd presets', () {
    test('uses the subtree-capable automatic mainnet pool by default', () {
      expect(
        LightdEndpoint.defaultEndpoint,
        LightdEndpoint.autoMainnetClearnet,
      );
      expect(LightdEndpoint.defaultEndpoint.automaticFailover, isTrue);
      expect(LightdEndpoint.defaultEndpoint.host, kCryptoForge2LightdHost);
      expect(LightdEndpoint.mainnet, LightdEndpoint.officialMainnet);
      expect(kDefaultLightdUrl, 'https://lightd1.pirate.black:443');
      expect(kDefaultUseTls, isTrue);
    });

    test('offers curated clearnet servers without retired dev endpoints', () {
      final presets = LightdEndpoint.presetsForTransport(
        'direct',
        includeTestnet: false,
      );

      expect(presets, <LightdEndpoint>[
        LightdEndpoint.autoMainnetClearnet,
        LightdEndpoint.cryptoForge1Mainnet,
        LightdEndpoint.cryptoForge2Mainnet,
        LightdEndpoint.mathNodesMainnet,
        LightdEndpoint.officialMainnet,
        LightdEndpoint.qortalMainnet,
        LightdEndpoint.qortal2Mainnet,
        LightdEndpoint.qortal3Mainnet,
      ]);
      expect(
        presets.every((endpoint) => endpoint.route == LightdRoute.clearnet),
        isTrue,
      );
      expect(
        presets.any((endpoint) => endpoint.host == '64.23.167.130'),
        isFalse,
      );
      expect(
        presets.any((endpoint) => endpoint.host == 'lightd.pirate.black'),
        isFalse,
      );
    });

    test('prioritizes onion presets before Tor-routed clearnet servers', () {
      final presets = LightdEndpoint.presetsForTransport(
        'tor',
        includeTestnet: false,
      );

      expect(presets, <LightdEndpoint>[
        LightdEndpoint.autoMainnetTor,
        LightdEndpoint.mainnetTor1,
        LightdEndpoint.mainnetTor2,
        LightdEndpoint.cryptoForge1Mainnet,
        LightdEndpoint.cryptoForge2Mainnet,
        LightdEndpoint.mathNodesMainnet,
        LightdEndpoint.officialMainnet,
        LightdEndpoint.qortalMainnet,
        LightdEndpoint.qortal2Mainnet,
        LightdEndpoint.qortal3Mainnet,
      ]);
      expect(
        presets.every((endpoint) => endpoint.supportsTransport('tor')),
        isTrue,
      );
      expect(
        presets
            .take(3)
            .every((endpoint) => !endpoint.supportsTransport('direct')),
        isTrue,
      );
      expect(
        presets
            .skip(3)
            .every((endpoint) => endpoint.supportsTransport('direct')),
        isTrue,
      );
      expect(presets, isNot(contains(LightdEndpoint.autoMainnetClearnet)));
    });

    test('shows only official I2P presets in I2P mode', () {
      final presets = LightdEndpoint.presetsForTransport(
        'i2p',
        includeTestnet: false,
      );

      expect(presets, <LightdEndpoint>[LightdEndpoint.mainnetI2p1]);
      expect(
        presets.every((endpoint) => endpoint.supportsTransport('i2p')),
        isTrue,
      );
    });

    test('includes every Ironwood testnet route', () {
      expect(
        LightdEndpoint.presetsForTransport('direct').where(
          (endpoint) => endpoint.network == LightdNetwork.ironwoodTestnet,
        ),
        <LightdEndpoint>[
          LightdEndpoint.autoIronwoodTestnetClearnet,
          LightdEndpoint.ironwoodTestnet1,
          LightdEndpoint.ironwoodTestnet2,
        ],
      );
      expect(
        LightdEndpoint.presetsForTransport('tor').where(
          (endpoint) => endpoint.network == LightdNetwork.ironwoodTestnet,
        ),
        <LightdEndpoint>[
          LightdEndpoint.autoIronwoodTestnetTor,
          LightdEndpoint.ironwoodTestnetTor1,
          LightdEndpoint.ironwoodTestnetTor2,
          LightdEndpoint.ironwoodTestnet1,
          LightdEndpoint.ironwoodTestnet2,
        ],
      );
      expect(
        LightdEndpoint.presetsForTransport('i2p').where(
          (endpoint) => endpoint.network == LightdNetwork.ironwoodTestnet,
        ),
        <LightdEndpoint>[
          LightdEndpoint.autoIronwoodTestnetI2p,
          LightdEndpoint.ironwoodTestnetI2p1,
          LightdEndpoint.ironwoodTestnetI2p2,
        ],
      );
    });

    test('automatic pools contain only the same network and route', () {
      for (final automatic in <LightdEndpoint>[
        LightdEndpoint.autoMainnetClearnet,
        LightdEndpoint.autoMainnetTor,
        LightdEndpoint.autoIronwoodTestnetClearnet,
        LightdEndpoint.autoIronwoodTestnetTor,
        LightdEndpoint.autoIronwoodTestnetI2p,
      ]) {
        final candidates = LightdEndpoint.failoverCandidates(automatic);
        expect(candidates, isNotEmpty, reason: automatic.id);
        expect(
          candidates.every(
            (candidate) =>
                candidate.network == automatic.network &&
                candidate.route == automatic.route &&
                candidate.useTls == automatic.useTls &&
                !candidate.automaticFailover,
          ),
          isTrue,
          reason: automatic.id,
        );
        expect(
          candidates.any((candidate) => candidate.url == automatic.url),
          isFalse,
          reason: automatic.id,
        );
      }
    });

    test('manual and custom selections never acquire a pool implicitly', () {
      const custom = LightdEndpoint(
        host: 'example.com',
        port: 443,
        useTls: true,
      );

      expect(LightdEndpoint.failoverCandidates(custom), isEmpty);
      expect(
        LightdEndpoint.failoverCandidates(LightdEndpoint.officialMainnet),
        isEmpty,
      );
      expect(
        LightdEndpoint.failoverCandidates(LightdEndpoint.ironwoodTestnet1),
        isEmpty,
      );
    });

    test('automatic selection follows transport without changing networks', () {
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'i2p',
          current: LightdEndpoint.autoMainnetClearnet,
        ),
        LightdEndpoint.mainnetI2p1,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'tor',
          current: LightdEndpoint.autoIronwoodTestnetI2p,
        ),
        LightdEndpoint.autoIronwoodTestnetTor,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'direct',
          current: LightdEndpoint.autoMainnetClearnet,
        ),
        isNull,
      );

      final legacyAuto = LightdEndpoint.tryParse(
        'https://lightd1.pirate.black:443',
        automaticFailover: true,
      );
      expect(
        LightdEndpoint.currentAutomaticPreset(legacyAuto!),
        LightdEndpoint.autoMainnetClearnet,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'direct',
          current: legacyAuto,
        ),
        LightdEndpoint.autoMainnetClearnet,
      );
    });

    test('manual choices persist while compatible with the transport', () {
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'direct',
          current: LightdEndpoint.mathNodesMainnet,
        ),
        isNull,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'tor',
          current: LightdEndpoint.mainnetTor1,
        ),
        isNull,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'tor',
          current: LightdEndpoint.mathNodesMainnet,
        ),
        isNull,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'direct',
          current: LightdEndpoint.mainnetTor1,
          storedNonI2p: LightdEndpoint.mathNodesMainnet,
        ),
        LightdEndpoint.mathNodesMainnet,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'i2p',
          current: LightdEndpoint.mathNodesMainnet,
          configuredI2p: LightdEndpoint.autoMainnetI2p,
        ),
        LightdEndpoint.autoMainnetI2p,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'direct',
          current: LightdEndpoint.autoMainnetI2p,
          storedNonI2p: LightdEndpoint.mathNodesMainnet,
        ),
        LightdEndpoint.mathNodesMainnet,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'tor',
          current: LightdEndpoint.autoMainnetI2p,
          storedNonI2p: LightdEndpoint.mathNodesMainnet,
        ),
        LightdEndpoint.mathNodesMainnet,
      );
    });

    test('retired presets migrate to the matching automatic route', () {
      final retiredMainnet = LightdEndpoint.tryParse(
        'http://64.23.167.130:9067',
      );
      final retiredTestnet = LightdEndpoint.tryParse(
        'http://64.23.167.130:8067',
      );
      final retiredI2p = LightdEndpoint.tryParse(
        'http://rud5qc4s4tsjzuhzygzdweoorhofbgobo7zuo7qeor25oyqonitq.b32.i2p:9067',
      );
      final misconfiguredI2p = LightdEndpoint.tryParse(
        'http://47go5e2vfmm2o5qdl7zr7rzf57hxjt6z4453ugvgyfkl3bbobwmq.b32.i2p:9067',
      );

      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'direct',
          current: retiredMainnet,
        ),
        LightdEndpoint.autoMainnetClearnet,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'tor',
          current: retiredTestnet,
        ),
        LightdEndpoint.autoIronwoodTestnetTor,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'i2p',
          current: retiredI2p,
        ),
        LightdEndpoint.mainnetI2p1,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'i2p',
          current: misconfiguredI2p,
        ),
        LightdEndpoint.mainnetI2p1,
      );
    });
  });

  group('endpoint parsing', () {
    test('distinguishes Auto from a manual server at its primary URL', () {
      final manual = LightdEndpoint.tryParse(
        'https://lightwalletd2.cryptoforge.cc:443/',
      );
      final automatic = LightdEndpoint.tryParse(
        'https://lightwalletd2.cryptoforge.cc:443/',
        automaticFailover: true,
      );

      expect(manual, LightdEndpoint.cryptoForge2Mainnet);
      expect(manual!.automaticFailover, isFalse);
      expect(automatic, LightdEndpoint.autoMainnetClearnet);
      expect(automatic!.automaticFailover, isTrue);
    });

    test('uses standard TLS and lightwalletd ports when omitted', () {
      expect(LightdEndpoint.tryParse('https://example.com')!.port, 443);
      expect(LightdEndpoint.tryParse('http://example.com')!.port, 9067);
    });

    test('recognizes official hidden-service routes', () {
      expect(
        LightdEndpoint.tryParse(kDefaultI2pLightdUrl)!.route,
        LightdRoute.i2p,
      );
      expect(
        LightdEndpoint.tryParse(LightdEndpoint.mainnetTor1.url)!.route,
        LightdRoute.tor,
      );
    });

    test('rejects endpoint paths, credentials, and unsupported schemes', () {
      expect(LightdEndpoint.tryParse('https://example.com:443/path'), isNull);
      expect(LightdEndpoint.tryParse('https://user@example.com:443'), isNull);
      expect(LightdEndpoint.tryParse('ftp://example.com:443'), isNull);
    });
  });

  test('stored I2P preferences migrate to the official endpoint', () {
    final empty = TransportConfig.fromJson(const <String, dynamic>{
      'mode': 'i2p',
      'i2p_endpoint': '',
    });
    final retired = TransportConfig.fromJson(const <String, dynamic>{
      'mode': 'i2p',
      'i2p_endpoint': 'http://rud5qc4s4tsjzuhzygzdweoorhofbgobo7zuo7qeor25oyqonitq.b32.i2p:9067',
    });

    expect(empty.i2pEndpoint, kDefaultI2pLightdUrl);
    expect(retired.i2pEndpoint, kDefaultI2pLightdUrl);
  });
}
