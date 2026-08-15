import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/config/endpoints.dart';
import 'package:pirate_wallet/features/settings/providers/transport_providers.dart';

void main() {
  group('lightwalletd presets', () {
    test('offers all curated clearnet mainnet endpoints', () {
      final presets = LightdEndpoint.presetsForTransport(
        'direct',
        includeTestnet: false,
      );

      expect(
        presets.map((endpoint) => endpoint.url),
        containsAll(<String>[
          'http://64.23.167.130:9067',
          'https://lightd1.pirate.black:443',
          'https://pirate.mathnodes.com:443',
        ]),
      );
      expect(
        presets.every((endpoint) => endpoint.route == LightdRoute.clearnet),
        isTrue,
      );
    });

    test('adds the hidden service only when Tor can resolve it', () {
      final torPresets = LightdEndpoint.presetsForTransport(
        'tor',
        includeTestnet: false,
      );
      final directPresets = LightdEndpoint.presetsForTransport(
        'direct',
        includeTestnet: false,
      );

      expect(torPresets, contains(LightdEndpoint.torMainnet));
      expect(directPresets, isNot(contains(LightdEndpoint.torMainnet)));
      expect(LightdEndpoint.torMainnet.supportsTransport('tor'), isTrue);
      expect(LightdEndpoint.torMainnet.supportsTransport('direct'), isFalse);
    });

    test('uses only the I2P hidden service in I2P mode', () {
      expect(LightdEndpoint.presetsForTransport('i2p'), <LightdEndpoint>[
        LightdEndpoint.i2pMainnet,
      ]);
      expect(LightdEndpoint.i2pMainnet.supportsTransport('i2p'), isTrue);
      expect(LightdEndpoint.i2pMainnet.supportsTransport('tor'), isFalse);
    });

    test('never mixes custom or testnet endpoints into failover', () {
      const custom = LightdEndpoint(
        host: 'example.com',
        port: 443,
        useTls: true,
      );

      expect(LightdEndpoint.failoverCandidates(custom, 'direct'), isEmpty);
      expect(
        LightdEndpoint.failoverCandidates(
          LightdEndpoint.ironwoodTestnet,
          'direct',
        ),
        isEmpty,
      );
      expect(
        LightdEndpoint.failoverCandidates(LightdEndpoint.unifiedMainnet, 'i2p'),
        isEmpty,
      );
    });

    test('moves into I2P and restores a compatible clearnet endpoint', () {
      final intoI2p = LightdEndpoint.replacementForTransport(
        mode: 'i2p',
        current: LightdEndpoint.officialMainnet,
        configuredI2p: LightdEndpoint.i2pMainnet,
      );
      final backToDirect = LightdEndpoint.replacementForTransport(
        mode: 'direct',
        current: LightdEndpoint.i2pMainnet,
        storedNonI2p: LightdEndpoint.officialMainnet,
      );

      expect(intoI2p, LightdEndpoint.i2pMainnet);
      expect(backToDirect, LightdEndpoint.officialMainnet);
    });

    test('never carries hidden-service routes into incompatible modes', () {
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'direct',
          current: LightdEndpoint.torMainnet,
          storedNonI2p: LightdEndpoint.torMainnet,
        ),
        LightdEndpoint.unifiedMainnet,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'tor',
          current: LightdEndpoint.i2pMainnet,
        ),
        LightdEndpoint.torMainnet,
      );
      expect(
        LightdEndpoint.replacementForTransport(
          mode: 'tor',
          current: LightdEndpoint.officialMainnet,
        ),
        isNull,
      );
    });
  });

  group('endpoint parsing', () {
    test('recognizes curated endpoint metadata', () {
      final endpoint = LightdEndpoint.tryParse(
        'https://lightd1.pirate.black:443/',
      );

      expect(endpoint, isNotNull);
      expect(endpoint!.id, 'pirate-official');
      expect(endpoint.network, LightdNetwork.mainnet);
      expect(endpoint.automaticFailover, isTrue);
    });

    test('uses standard TLS and lightwalletd ports when omitted', () {
      expect(LightdEndpoint.tryParse('https://example.com')!.port, 443);
      expect(LightdEndpoint.tryParse('http://example.com')!.port, 9067);
    });

    test('recognizes hidden-service routes', () {
      expect(
        LightdEndpoint.tryParse(kDefaultI2pLightdUrl)!.route,
        LightdRoute.i2p,
      );
      expect(
        LightdEndpoint.tryParse(LightdEndpoint.torMainnet.url)!.route,
        LightdRoute.tor,
      );
    });

    test('rejects endpoint paths, credentials, and unsupported schemes', () {
      expect(LightdEndpoint.tryParse('https://example.com:443/path'), isNull);
      expect(LightdEndpoint.tryParse('https://user@example.com:443'), isNull);
      expect(LightdEndpoint.tryParse('ftp://example.com:443'), isNull);
    });
  });

  test('empty stored I2P endpoints migrate to the curated preset', () {
    final config = TransportConfig.fromJson(const <String, dynamic>{
      'mode': 'i2p',
      'i2p_endpoint': '',
    });

    expect(config.i2pEndpoint, kDefaultI2pLightdUrl);
  });
}
