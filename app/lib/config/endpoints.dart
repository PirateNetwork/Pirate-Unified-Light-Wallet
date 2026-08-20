// Lightwalletd endpoint configuration.

import 'package:flutter/foundation.dart';

import '../core/i18n/arb_text_localizer.dart';

const String kDevLightdHost = '64.23.167.130';
const int kDevLightdPort = 9067;

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

const String kTorLightdHost =
    'lx34l6evvk7vynbulx6brxqyzzes4balb3owhteb4jyqpdoosbfc3oid.onion';
const int kTorLightdPort = 9067;
const String kTorLightd = '$kTorLightdHost:$kTorLightdPort';

const String kI2pLightdHost =
    'rud5qc4s4tsjzuhzygzdweoorhofbgobo7zuo7qeor25oyqonitq.b32.i2p';
const int kI2pLightdPort = 9067;
const String kI2pLightd = '$kI2pLightdHost:$kI2pLightdPort';
const String kDefaultI2pLightdUrl = 'http://$kI2pLightd';

const String kIronwoodTestnetHost = '64.23.167.130';
const int kIronwoodTestnetPort = 8067;

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

  static final LightdEndpoint unifiedMainnet = LightdEndpoint(
    id: 'pirate-unified',
    host: kDevLightdHost,
    port: kDevLightdPort,
    tlsPin: kDefaultTlsPin.isEmpty ? null : kDefaultTlsPin,
    network: LightdNetwork.mainnet,
  );

  static final LightdEndpoint officialMainnet = LightdEndpoint(
    id: 'pirate-official',
    host: kOfficialLightdHost,
    port: kOfficialLightdPort,
    useTls: true,
    network: LightdNetwork.mainnet,
    automaticFailover: true,
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
    automaticFailover: true,
  );

  static final LightdEndpoint qortalMainnet = LightdEndpoint(
    id: 'qortal',
    host: kQortalLightdHost,
    port: 443,
    useTls: true,
    network: LightdNetwork.mainnet,
    automaticFailover: true,
  );

  static final LightdEndpoint qortal2Mainnet = LightdEndpoint(
    id: 'qortal-2',
    host: kQortal2LightdHost,
    port: 443,
    useTls: true,
    network: LightdNetwork.mainnet,
    automaticFailover: true,
  );

  static final LightdEndpoint qortal3Mainnet = LightdEndpoint(
    id: 'qortal-3',
    host: kQortal3LightdHost,
    port: 443,
    useTls: true,
    network: LightdNetwork.mainnet,
    automaticFailover: true,
  );

  static final LightdEndpoint cryptoForge1Mainnet = LightdEndpoint(
    id: 'cryptoforge-1',
    host: kCryptoForge1LightdHost,
    port: 443,
    useTls: true,
    network: LightdNetwork.mainnet,
    automaticFailover: true,
  );

  static final LightdEndpoint cryptoForge2Mainnet = LightdEndpoint(
    id: 'cryptoforge-2',
    host: kCryptoForge2LightdHost,
    port: 443,
    useTls: true,
    network: LightdNetwork.mainnet,
    automaticFailover: true,
  );

  static final LightdEndpoint torMainnet = LightdEndpoint(
    id: 'pirate-tor',
    host: kTorLightdHost,
    port: kTorLightdPort,
    network: LightdNetwork.mainnet,
    route: LightdRoute.tor,
    automaticFailover: true,
  );

  static final LightdEndpoint i2pMainnet = LightdEndpoint(
    id: 'pirate-i2p',
    host: kI2pLightdHost,
    port: kI2pLightdPort,
    network: LightdNetwork.mainnet,
    route: LightdRoute.i2p,
  );

  static final LightdEndpoint ironwoodTestnet = LightdEndpoint(
    id: 'ironwood-testnet',
    host: kIronwoodTestnetHost,
    port: kIronwoodTestnetPort,
    network: LightdNetwork.ironwoodTestnet,
  );

  static final LightdEndpoint defaultEndpoint = officialMainnet;
  static final LightdEndpoint mainnet = officialMainnet;

  static final List<LightdEndpoint> mainnetPresets =
      List<LightdEndpoint>.unmodifiable([
        officialMainnet,
        pirateBlackMainnet,
        qortalMainnet,
        qortal2Mainnet,
        qortal3Mainnet,
        cryptoForge1Mainnet,
        cryptoForge2Mainnet,
        mathNodesMainnet,
        unifiedMainnet,
        torMainnet,
        i2pMainnet,
      ]);

  static final List<LightdEndpoint> allPresets =
      List<LightdEndpoint>.unmodifiable([...mainnetPresets, ironwoodTestnet]);

  /// Presets that can be reached through the selected transport.
  static List<LightdEndpoint> presetsForTransport(
    String mode, {
    bool includeTestnet = true,
  }) {
    final normalizedMode = mode.toLowerCase();
    final presets = switch (normalizedMode) {
      'i2p' => <LightdEndpoint>[i2pMainnet],
      'tor' => <LightdEndpoint>[
        officialMainnet,
        pirateBlackMainnet,
        qortalMainnet,
        qortal2Mainnet,
        qortal3Mainnet,
        cryptoForge1Mainnet,
        cryptoForge2Mainnet,
        mathNodesMainnet,
        unifiedMainnet,
        torMainnet,
      ],
      _ => <LightdEndpoint>[
        officialMainnet,
        pirateBlackMainnet,
        qortalMainnet,
        qortal2Mainnet,
        qortal3Mainnet,
        cryptoForge1Mainnet,
        cryptoForge2Mainnet,
        mathNodesMainnet,
        unifiedMainnet,
      ],
    };
    if (includeTestnet && normalizedMode != 'i2p') {
      presets.add(ironwoodTestnet);
    }
    return List<LightdEndpoint>.unmodifiable(presets);
  }

  /// Same-network candidates used by automatic health failover.
  static List<LightdEndpoint> failoverCandidates(
    LightdEndpoint current,
    String mode,
  ) {
    if (current.network != LightdNetwork.mainnet ||
        !current.automaticFailover ||
        mode.toLowerCase() == 'i2p') {
      return const <LightdEndpoint>[];
    }

    final ordered = mode.toLowerCase() == 'tor'
        ? <LightdEndpoint>[
            officialMainnet,
            pirateBlackMainnet,
            qortalMainnet,
            qortal2Mainnet,
            qortal3Mainnet,
            cryptoForge1Mainnet,
            cryptoForge2Mainnet,
          ]
        : <LightdEndpoint>[
            officialMainnet,
            pirateBlackMainnet,
            qortalMainnet,
            qortal2Mainnet,
            qortal3Mainnet,
            cryptoForge1Mainnet,
            cryptoForge2Mainnet,
          ];
    return List<LightdEndpoint>.unmodifiable(
      ordered.where((candidate) => candidate.id != current.id),
    );
  }

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
    if (current?.supportsTransport(normalizedMode) == true) return null;

    if (normalizedMode == 'i2p') {
      return configuredI2p?.route == LightdRoute.i2p
          ? configuredI2p
          : i2pMainnet;
    }

    if (storedNonI2p?.supportsTransport(normalizedMode) == true) {
      return storedNonI2p;
    }
    return normalizedMode == 'tor' ? torMainnet : officialMainnet;
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
    'pirate-unified' => 'Dev server Mainnet (no TLS)'.tr,
    'pirate-official' => 'Auto (Mainnet)'.tr,
    'pirate-black' => 'Pirate Black Mainnet'.tr,
    'mathnodes' => 'Mathnodes Mainnet'.tr,
    'qortal' => 'Qortal 1 Mainnet'.tr,
    'qortal-2' => 'Qortal 2 Mainnet'.tr,
    'qortal-3' => 'Qortal 3 Mainnet'.tr,
    'cryptoforge-1' => 'CryptoForge 1 Mainnet'.tr,
    'cryptoforge-2' => 'CryptoForge 2 Mainnet'.tr,
    'pirate-tor' => 'Tor'.tr,
    'pirate-i2p' => 'I2P'.tr,
    'ironwood-testnet' => 'Dev server Ironwood testnet (no TLS)'.tr,
    _ => label ?? host,
  };

  static LightdEndpoint? findPreset(String input) {
    final parsed = tryParse(input);
    if (parsed == null) return null;
    for (final preset in allPresets) {
      if (preset.host.toLowerCase() == parsed.host.toLowerCase() &&
          preset.port == parsed.port &&
          preset.useTls == parsed.useTls) {
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
        : kDevLightdPort;
    if (port < 1 || port > 65535) return null;
    final route = _routeForHost(host);

    LightdEndpoint? matched;
    for (final preset in allPresets) {
      if (preset.host.toLowerCase() == host.toLowerCase() &&
          preset.port == port &&
          preset.useTls == useTls) {
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
      automaticFailover: matched?.automaticFailover ?? false,
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
          tlsPin == other.tlsPin;

  @override
  int get hashCode => Object.hash(host, port, useTls, tlsPin);

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
