// Lightwalletd endpoint configuration.

import 'package:flutter/foundation.dart';

import '../core/i18n/arb_text_localizer.dart';

const String kOfficialLightdHost = 'lightd1.pirate.black';
const String kPirateBlackLightdHost = 'lightd.pirate.black';
const int kOfficialLightdPort = 443;
const String kDefaultLightdHost = kOfficialLightdHost;
const int kDefaultLightdPort = kOfficialLightdPort;
const String kDefaultLightd = '$kDefaultLightdHost:$kDefaultLightdPort';
const String kDefaultLightdUrl = 'https://$kDefaultLightd';
const String kMathNodesLightdHost = 'pirate.mathnodes.com';
const int kMathNodesLightdPort = 443;
const String kQortalLightdHost = 'arrr.qortal.link';
const String kQortal2LightdHost = 'arrr2.qortal.link';
const String kQortal3LightdHost = 'arrr3.qortal.link';
const String kCryptoForge1LightdHost = 'lightwalletd1.cryptoforge.cc';
const String kCryptoForge2LightdHost = 'lightwalletd2.cryptoforge.cc';

const int kMainnetHiddenLightdPort = 9067;
const String kMainnetTor1LightdHost =
    '4kbfoltkqir44ab62l6dhkovugdrdevxzjtp6duv6gga3ixoe6kwkcqd.onion';
const String kMainnetTor2LightdHost =
    'ibdhmxvqg3imgf67el6y2zxakuf37h3dyug4ujpa6qb7zvrz7sacmnqd.onion';
const String kMainnetI2p1LightdHost =
    '5vjlbxmzx4gjfuwcot2qtfjdnxodzpe4jsw3ckx7i4maltz7j5qa.b32.i2p';
const String kMainnetI2p2LightdHost =
    '47go5e2vfmm2o5qdl7zr7rzf57hxjt6z4453ugvgyfkl3bbobwmq.b32.i2p';

const String kIronwoodTestnet1LightdHost = 'testlightwalletd1.cryptoforge.cc';
const String kIronwoodTestnet2LightdHost = 'testlightwalletd2.cryptoforge.cc';
const int kIronwoodTestnetTlsPort = 443;
const int kIronwoodTestnetHiddenPort = 8067;
const String kIronwoodTestnetI2p1LightdHost =
    '6rwymqddf6dxaphftoy5n3wfgpgwut2upf2lnk6shimjkum2z6uq.b32.i2p';
const String kIronwoodTestnetI2p2LightdHost =
    'g4vk6mdenflhm5j2c4kiujwkox7ygyftdfhwai6clgye4br2ujlq.b32.i2p';
const String kIronwoodTestnetTor1LightdHost =
    'lzciy5lpujcqz42vtbr523ceik6rkzlvwtknxfnpyxcskpmx3swkfryd.onion';
const String kIronwoodTestnetTor2LightdHost =
    'iwfhfhwyg6gfm3mqpe5clnwi5oh652hsd2aq4hiael7m7syl4nkyxiqd.onion';

const String kDefaultI2pLightdUrl =
    'http://$kMainnetI2p1LightdHost:$kMainnetHiddenLightdPort';

const String _retiredMainnetDevHost = '64.23.167.130';
const String _retiredMainnetTorHost =
    'lx34l6evvk7vynbulx6brxqyzzes4balb3owhteb4jyqpdoosbfc3oid.onion';
const String _retiredMainnetI2pHost =
    'rud5qc4s4tsjzuhzygzdweoorhofbgobo7zuo7qeor25oyqonitq.b32.i2p';

const bool kDefaultUseTls = true;
const String kDefaultTlsPin = '';

enum LightdNetwork { mainnet, ironwoodTestnet }

enum LightdRoute { clearnet, tor, i2p }

/// A lightwalletd endpoint and the routing constraints for using it safely.
@immutable
class LightdEndpoint {
  final String id;
  final String host;
  final int port;
  final bool useTls;
  final String? tlsPin;
  final String? label;
  final LightdNetwork? network;
  final LightdRoute route;
  final bool automaticFailover;

  const LightdEndpoint({
    required this.host,
    required this.port,
    this.id = 'custom',
    this.useTls = false,
    this.tlsPin,
    this.label,
    this.network,
    this.route = LightdRoute.clearnet,
    this.automaticFailover = false,
  });

  static final LightdEndpoint autoMainnetClearnet = LightdEndpoint(
    id: 'auto-mainnet-clearnet',
    host: kOfficialLightdHost,
    port: kOfficialLightdPort,
    useTls: true,
    network: LightdNetwork.mainnet,
    automaticFailover: true,
  );

  static final LightdEndpoint officialMainnet = LightdEndpoint(
    id: 'pirate-official',
    host: kOfficialLightdHost,
    port: kOfficialLightdPort,
    useTls: true,
    network: LightdNetwork.mainnet,
  );

  static final LightdEndpoint mathNodesMainnet = LightdEndpoint(
    id: 'mathnodes',
    host: kMathNodesLightdHost,
    port: kMathNodesLightdPort,
    useTls: true,
    network: LightdNetwork.mainnet,
  );

  static final LightdEndpoint pirateBlackMainnet = LightdEndpoint(
    id: 'pirate-black',
    host: kPirateBlackLightdHost,
    port: 443,
    useTls: true,
    network: LightdNetwork.mainnet,
  );

  static final LightdEndpoint qortalMainnet = LightdEndpoint(
    id: 'qortal',
    host: kQortalLightdHost,
    port: 443,
    useTls: true,
    network: LightdNetwork.mainnet,
  );

  static final LightdEndpoint qortal2Mainnet = LightdEndpoint(
    id: 'qortal-2',
    host: kQortal2LightdHost,
    port: 443,
    useTls: true,
    network: LightdNetwork.mainnet,
  );

  static final LightdEndpoint qortal3Mainnet = LightdEndpoint(
    id: 'qortal-3',
    host: kQortal3LightdHost,
    port: 443,
    useTls: true,
    network: LightdNetwork.mainnet,
  );

  static final LightdEndpoint cryptoForge1Mainnet = LightdEndpoint(
    id: 'cryptoforge-1',
    host: kCryptoForge1LightdHost,
    port: 443,
    useTls: true,
    network: LightdNetwork.mainnet,
  );

  static final LightdEndpoint cryptoForge2Mainnet = LightdEndpoint(
    id: 'cryptoforge-2',
    host: kCryptoForge2LightdHost,
    port: 443,
    useTls: true,
    network: LightdNetwork.mainnet,
  );

  static final LightdEndpoint autoMainnetTor = LightdEndpoint(
    id: 'auto-mainnet-tor',
    host: kMainnetTor1LightdHost,
    port: kMainnetHiddenLightdPort,
    network: LightdNetwork.mainnet,
    route: LightdRoute.tor,
    automaticFailover: true,
  );

  static final LightdEndpoint mainnetTor1 = LightdEndpoint(
    id: 'mainnet-tor-1',
    host: kMainnetTor1LightdHost,
    port: kMainnetHiddenLightdPort,
    network: LightdNetwork.mainnet,
    route: LightdRoute.tor,
  );

  static final LightdEndpoint mainnetTor2 = LightdEndpoint(
    id: 'mainnet-tor-2',
    host: kMainnetTor2LightdHost,
    port: kMainnetHiddenLightdPort,
    network: LightdNetwork.mainnet,
    route: LightdRoute.tor,
  );

  static final LightdEndpoint autoMainnetI2p = LightdEndpoint(
    id: 'auto-mainnet-i2p',
    host: kMainnetI2p1LightdHost,
    port: kMainnetHiddenLightdPort,
    network: LightdNetwork.mainnet,
    route: LightdRoute.i2p,
    automaticFailover: true,
  );

  static final LightdEndpoint mainnetI2p1 = LightdEndpoint(
    id: 'mainnet-i2p-1',
    host: kMainnetI2p1LightdHost,
    port: kMainnetHiddenLightdPort,
    network: LightdNetwork.mainnet,
    route: LightdRoute.i2p,
  );

  static final LightdEndpoint mainnetI2p2 = LightdEndpoint(
    id: 'mainnet-i2p-2',
    host: kMainnetI2p2LightdHost,
    port: kMainnetHiddenLightdPort,
    network: LightdNetwork.mainnet,
    route: LightdRoute.i2p,
  );

  static final LightdEndpoint autoIronwoodTestnetClearnet = LightdEndpoint(
    id: 'auto-ironwood-clearnet',
    host: kIronwoodTestnet1LightdHost,
    port: kIronwoodTestnetTlsPort,
    useTls: true,
    network: LightdNetwork.ironwoodTestnet,
    automaticFailover: true,
  );

  static final LightdEndpoint ironwoodTestnet1 = LightdEndpoint(
    id: 'ironwood-testnet-1',
    host: kIronwoodTestnet1LightdHost,
    port: kIronwoodTestnetTlsPort,
    useTls: true,
    network: LightdNetwork.ironwoodTestnet,
  );

  static final LightdEndpoint ironwoodTestnet2 = LightdEndpoint(
    id: 'ironwood-testnet-2',
    host: kIronwoodTestnet2LightdHost,
    port: kIronwoodTestnetTlsPort,
    useTls: true,
    network: LightdNetwork.ironwoodTestnet,
  );

  static final LightdEndpoint autoIronwoodTestnetTor = LightdEndpoint(
    id: 'auto-ironwood-tor',
    host: kIronwoodTestnetTor1LightdHost,
    port: kIronwoodTestnetHiddenPort,
    network: LightdNetwork.ironwoodTestnet,
    route: LightdRoute.tor,
    automaticFailover: true,
  );

  static final LightdEndpoint ironwoodTestnetTor1 = LightdEndpoint(
    id: 'ironwood-testnet-tor-1',
    host: kIronwoodTestnetTor1LightdHost,
    port: kIronwoodTestnetHiddenPort,
    network: LightdNetwork.ironwoodTestnet,
    route: LightdRoute.tor,
  );

  static final LightdEndpoint ironwoodTestnetTor2 = LightdEndpoint(
    id: 'ironwood-testnet-tor-2',
    host: kIronwoodTestnetTor2LightdHost,
    port: kIronwoodTestnetHiddenPort,
    network: LightdNetwork.ironwoodTestnet,
    route: LightdRoute.tor,
  );

  static final LightdEndpoint autoIronwoodTestnetI2p = LightdEndpoint(
    id: 'auto-ironwood-i2p',
    host: kIronwoodTestnetI2p1LightdHost,
    port: kIronwoodTestnetHiddenPort,
    network: LightdNetwork.ironwoodTestnet,
    route: LightdRoute.i2p,
    automaticFailover: true,
  );

  static final LightdEndpoint ironwoodTestnetI2p1 = LightdEndpoint(
    id: 'ironwood-testnet-i2p-1',
    host: kIronwoodTestnetI2p1LightdHost,
    port: kIronwoodTestnetHiddenPort,
    network: LightdNetwork.ironwoodTestnet,
    route: LightdRoute.i2p,
  );

  static final LightdEndpoint ironwoodTestnetI2p2 = LightdEndpoint(
    id: 'ironwood-testnet-i2p-2',
    host: kIronwoodTestnetI2p2LightdHost,
    port: kIronwoodTestnetHiddenPort,
    network: LightdNetwork.ironwoodTestnet,
    route: LightdRoute.i2p,
  );

  static final LightdEndpoint defaultEndpoint = autoMainnetClearnet;
  static final LightdEndpoint mainnet = officialMainnet;

  static final List<LightdEndpoint> mainnetClearnetPresets =
      List<LightdEndpoint>.unmodifiable([
        autoMainnetClearnet,
        officialMainnet,
        pirateBlackMainnet,
        cryptoForge1Mainnet,
        cryptoForge2Mainnet,
        qortalMainnet,
        qortal2Mainnet,
        qortal3Mainnet,
        mathNodesMainnet,
      ]);

  static final List<LightdEndpoint> mainnetTorPresets =
      List<LightdEndpoint>.unmodifiable([
        autoMainnetTor,
        mainnetTor1,
        mainnetTor2,
      ]);

  static final List<LightdEndpoint> mainnetI2pPresets =
      List<LightdEndpoint>.unmodifiable([
        autoMainnetI2p,
        mainnetI2p1,
        mainnetI2p2,
      ]);

  static final List<LightdEndpoint> ironwoodClearnetPresets =
      List<LightdEndpoint>.unmodifiable([
        autoIronwoodTestnetClearnet,
        ironwoodTestnet1,
        ironwoodTestnet2,
      ]);

  static final List<LightdEndpoint> ironwoodTorPresets =
      List<LightdEndpoint>.unmodifiable([
        autoIronwoodTestnetTor,
        ironwoodTestnetTor1,
        ironwoodTestnetTor2,
      ]);

  static final List<LightdEndpoint> ironwoodI2pPresets =
      List<LightdEndpoint>.unmodifiable([
        autoIronwoodTestnetI2p,
        ironwoodTestnetI2p1,
        ironwoodTestnetI2p2,
      ]);

  static final List<LightdEndpoint> mainnetPresets =
      List<LightdEndpoint>.unmodifiable([
        ...mainnetClearnetPresets,
        ...mainnetTorPresets,
        ...mainnetI2pPresets,
      ]);

  static final List<LightdEndpoint> allPresets =
      List<LightdEndpoint>.unmodifiable([
        ...mainnetPresets,
        ...ironwoodClearnetPresets,
        ...ironwoodTorPresets,
        ...ironwoodI2pPresets,
      ]);

  /// Presets that can be reached through the selected transport.
  static List<LightdEndpoint> presetsForTransport(
    String mode, {
    bool includeTestnet = true,
  }) {
    final normalizedMode = mode.toLowerCase();
    final (mainnetEndpoints, testnetEndpoints) = switch (normalizedMode) {
      'i2p' => (mainnetI2pPresets, ironwoodI2pPresets),
      'tor' => (mainnetTorPresets, ironwoodTorPresets),
      _ => (mainnetClearnetPresets, ironwoodClearnetPresets),
    };
    return List<LightdEndpoint>.unmodifiable([
      ...mainnetEndpoints,
      if (includeTestnet) ...testnetEndpoints,
    ]);
  }

  static LightdEndpoint automaticEndpointFor(
    LightdNetwork network,
    String mode,
  ) {
    return switch ((network, mode.toLowerCase())) {
      (LightdNetwork.mainnet, 'tor') => autoMainnetTor,
      (LightdNetwork.mainnet, 'i2p') => autoMainnetI2p,
      (LightdNetwork.mainnet, _) => autoMainnetClearnet,
      (LightdNetwork.ironwoodTestnet, 'tor') => autoIronwoodTestnetTor,
      (LightdNetwork.ironwoodTestnet, 'i2p') => autoIronwoodTestnetI2p,
      (LightdNetwork.ironwoodTestnet, _) => autoIronwoodTestnetClearnet,
    };
  }

  /// Same-network, same-route members supplied to the validated Rust pool.
  static List<LightdEndpoint> failoverCandidates(LightdEndpoint current) {
    if (!current.automaticFailover || current.network == null) {
      return const <LightdEndpoint>[];
    }
    final ordered = switch ((current.network!, current.route)) {
      (LightdNetwork.mainnet, LightdRoute.clearnet) => mainnetClearnetPresets,
      (LightdNetwork.mainnet, LightdRoute.tor) => mainnetTorPresets,
      (LightdNetwork.mainnet, LightdRoute.i2p) => mainnetI2pPresets,
      (LightdNetwork.ironwoodTestnet, LightdRoute.clearnet) =>
        ironwoodClearnetPresets,
      (LightdNetwork.ironwoodTestnet, LightdRoute.tor) => ironwoodTorPresets,
      (LightdNetwork.ironwoodTestnet, LightdRoute.i2p) => ironwoodI2pPresets,
    };
    return List<LightdEndpoint>.unmodifiable(
      ordered.where(
        (candidate) =>
            !candidate.automaticFailover && candidate.url != current.url,
      ),
    );
  }

  List<String> get failoverUrls =>
      failoverCandidates(this)
          .map((endpoint) => endpoint.url)
          .toList(growable: false);

  bool supportsTransport(String mode) {
    return switch (mode.toLowerCase()) {
      'i2p' => route == LightdRoute.i2p,
      'tor' => route != LightdRoute.i2p,
      _ => route == LightdRoute.clearnet,
    };
  }

  /// Chooses a compatible endpoint when the transport changes.
  ///
  /// A null result means the current endpoint already works through the new
  /// transport and should be left untouched.
  static LightdEndpoint? replacementForTransport({
    required String mode,
    required LightdEndpoint? current,
    LightdEndpoint? storedNonI2p,
    LightdEndpoint? configuredI2p,
  }) {
    final normalizedMode = mode.toLowerCase();
    final retiredNetwork = _retiredPresetNetwork(current);
    final currentNetwork =
        retiredNetwork ?? current?.network ?? LightdNetwork.mainnet;
    if (retiredNetwork != null) {
      return automaticEndpointFor(currentNetwork, normalizedMode);
    }
    if (normalizedMode != 'i2p' &&
        current?.route == LightdRoute.i2p &&
        storedNonI2p?.supportsTransport(normalizedMode) == true &&
        storedNonI2p?.network == currentNetwork) {
      return storedNonI2p;
    }
    if (current?.automaticFailover == true) {
      final target = automaticEndpointFor(currentNetwork, normalizedMode);
      return current == target ? null : target;
    }
    if (current?.supportsTransport(normalizedMode) == true) return null;

    if (normalizedMode == 'i2p') {
      return configuredI2p?.route == LightdRoute.i2p &&
              configuredI2p?.network == currentNetwork
          ? configuredI2p
          : automaticEndpointFor(currentNetwork, normalizedMode);
    }

    if (storedNonI2p?.supportsTransport(normalizedMode) == true &&
        storedNonI2p?.network == currentNetwork) {
      return storedNonI2p;
    }
    return automaticEndpointFor(currentNetwork, normalizedMode);
  }

  static LightdNetwork? _retiredPresetNetwork(LightdEndpoint? endpoint) {
    if (endpoint == null) return null;
    final host = endpoint.host.toLowerCase();
    if (host == _retiredMainnetDevHost) {
      return endpoint.port == 8067
          ? LightdNetwork.ironwoodTestnet
          : LightdNetwork.mainnet;
    }
    if (host == _retiredMainnetTorHost || host == _retiredMainnetI2pHost) {
      return LightdNetwork.mainnet;
    }
    return null;
  }

  String get url {
    final scheme = useTls ? 'https' : 'http';
    return '$scheme://$displayString';
  }

  String get displayString {
    final displayHost = host.contains(':') ? '[$host]' : host;
    return '$displayHost:$port';
  }

  /// Localized preset label, or the user-supplied label for custom servers.
  String get displayLabel => switch (id) {
    'auto-mainnet-clearnet' ||
    'auto-mainnet-tor' ||
    'auto-mainnet-i2p' => 'Auto (Mainnet)'.tr,
    'auto-ironwood-clearnet' ||
    'auto-ironwood-tor' ||
    'auto-ironwood-i2p' => 'Auto (Ironwood testnet)'.tr,
    'pirate-official' => 'Pirate Chain Mainnet'.tr,
    'pirate-black' => 'Pirate Black Mainnet'.tr,
    'mathnodes' => 'Mathnodes Mainnet'.tr,
    'qortal' => 'Qortal 1 Mainnet'.tr,
    'qortal-2' => 'Qortal 2 Mainnet'.tr,
    'qortal-3' => 'Qortal 3 Mainnet'.tr,
    'cryptoforge-1' => 'CryptoForge 1 Mainnet'.tr,
    'cryptoforge-2' => 'CryptoForge 2 Mainnet'.tr,
    'mainnet-tor-1' => 'Pirate Tor 1 Mainnet'.tr,
    'mainnet-tor-2' => 'Pirate Tor 2 Mainnet'.tr,
    'mainnet-i2p-1' => 'Pirate I2P 1 Mainnet'.tr,
    'mainnet-i2p-2' => 'Pirate I2P 2 Mainnet'.tr,
    'ironwood-testnet-1' => 'CryptoForge 1 Ironwood testnet'.tr,
    'ironwood-testnet-2' => 'CryptoForge 2 Ironwood testnet'.tr,
    'ironwood-testnet-tor-1' => 'Pirate Tor 1 Ironwood testnet'.tr,
    'ironwood-testnet-tor-2' => 'Pirate Tor 2 Ironwood testnet'.tr,
    'ironwood-testnet-i2p-1' => 'Pirate I2P 1 Ironwood testnet'.tr,
    'ironwood-testnet-i2p-2' => 'Pirate I2P 2 Ironwood testnet'.tr,
    _ => label ?? host,
  };

  String get displaySubtitle => automaticFailover
      ? 'Chooses healthy servers automatically'.tr
      : displayString;

  static LightdEndpoint? findPreset(
    String input, {
    bool automaticFailover = false,
  }) {
    final parsed = tryParse(input, automaticFailover: automaticFailover);
    if (parsed == null) return null;
    for (final preset in allPresets) {
      if (preset.host.toLowerCase() == parsed.host.toLowerCase() &&
          preset.port == parsed.port &&
          preset.useTls == parsed.useTls &&
          preset.automaticFailover == automaticFailover) {
        return preset;
      }
    }
    return null;
  }

  /// Parses `host:port`, `https://host:port`, and `http://host:port`.
  static LightdEndpoint? tryParse(
    String input, {
    String? tlsPin,
    String? label,
    bool automaticFailover = false,
  }) {
    final trimmed = input.trim();
    if (trimmed.isEmpty) return null;

    final hasScheme = trimmed.contains('://');
    final uri = Uri.tryParse(hasScheme ? trimmed : 'http://$trimmed');
    if (uri == null ||
        uri.host.isEmpty ||
        uri.userInfo.isNotEmpty ||
        uri.hasQuery ||
        uri.hasFragment ||
        (uri.path.isNotEmpty && uri.path != '/')) {
      return null;
    }
    if (uri.scheme != 'http' && uri.scheme != 'https') return null;

    final host = uri.host;
    if (!_isValidHost(host)) return null;
    final useTls = uri.scheme == 'https';
    final port = uri.hasPort
        ? uri.port
        : useTls
        ? 443
        : kMainnetHiddenLightdPort;
    if (port < 1 || port > 65535) return null;
    final route = _routeForHost(host);

    LightdEndpoint? matched;
    for (final preset in allPresets) {
      if (preset.host.toLowerCase() == host.toLowerCase() &&
          preset.port == port &&
          preset.useTls == useTls &&
          preset.automaticFailover == automaticFailover) {
        matched = preset;
        break;
      }
    }

    return LightdEndpoint(
      id: matched?.id ?? 'custom',
      host: host,
      port: port,
      useTls: useTls,
      tlsPin: tlsPin ?? matched?.tlsPin,
      label: label ?? matched?.label,
      network: matched?.network,
      route: matched?.route ?? route,
      automaticFailover: matched?.automaticFailover ?? automaticFailover,
    );
  }

  static LightdRoute _routeForHost(String host) {
    final normalized = host.toLowerCase();
    if (normalized.endsWith('.b32.i2p') || normalized.endsWith('.i2p')) {
      return LightdRoute.i2p;
    }
    if (normalized.endsWith('.onion')) return LightdRoute.tor;
    return LightdRoute.clearnet;
  }

  static bool _isValidHost(String host) {
    final domainRegex = RegExp(r'^[a-zA-Z0-9]([a-zA-Z0-9\-\.]*[a-zA-Z0-9])?$');
    final ipv4Regex = RegExp(r'^(\d{1,3}\.){3}\d{1,3}$');
    final ipv6Regex = RegExp(r'^[a-fA-F0-9:]+$');
    return domainRegex.hasMatch(host) ||
        ipv4Regex.hasMatch(host) ||
        ipv6Regex.hasMatch(host);
  }

  static bool isValidTlsPin(String pin) {
    if (pin.length < 40 || pin.length > 48) return false;
    return RegExp(r'^[A-Za-z0-9+/]+=*$').hasMatch(pin);
  }

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is LightdEndpoint &&
          host == other.host &&
          port == other.port &&
          useTls == other.useTls &&
          tlsPin == other.tlsPin &&
          automaticFailover == other.automaticFailover;

  @override
  int get hashCode =>
      Object.hash(host, port, useTls, tlsPin, automaticFailover);

  Map<String, dynamic> toJson() => {
    'id': id,
    'host': host,
    'port': port,
    'useTls': useTls,
    if (tlsPin != null) 'tlsPin': tlsPin,
    if (label != null) 'label': label,
    if (network != null) 'network': network!.name,
    'route': route.name,
    'automaticFailover': automaticFailover,
  };

  factory LightdEndpoint.fromJson(Map<String, dynamic> json) {
    final route = LightdRoute.values.where(
      (value) => value.name == json['route'],
    );
    final network = LightdNetwork.values.where(
      (value) => value.name == json['network'],
    );
    return LightdEndpoint(
      id: json['id'] as String? ?? 'custom',
      host: json['host'] as String,
      port: json['port'] as int,
      useTls: json['useTls'] as bool? ?? true,
      tlsPin: json['tlsPin'] as String?,
      label: json['label'] as String?,
      network: network.isEmpty ? null : network.first,
      route: route.isEmpty ? LightdRoute.clearnet : route.first,
      automaticFailover: json['automaticFailover'] as bool? ?? false,
    );
  }
}

const String kEndpointStorageKey = 'lightd_endpoint';
