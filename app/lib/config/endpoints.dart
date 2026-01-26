// Lightwalletd endpoint configuration
//
// Default endpoint and TLS settings for Pirate Chain lightwalletd servers.

import 'package:flutter/foundation.dart';

/// Default lightwalletd server host (official mainnet)
const String kDefaultLightdHost = '64.23.167.130';

/// Default lightwalletd server port
const int kDefaultLightdPort = 9067;

/// Full default endpoint URL
const String kDefaultLightd = '$kDefaultLightdHost:$kDefaultLightdPort';

/// Orchard-capable mainnet endpoint (provided)
const String kOrchardMainnetHost = '64.23.167.130';
const int kOrchardMainnetPort = 9067;

/// Orchard-capable testnet endpoint (provided)
const String kOrchardTestnetHost = '64.23.167.130';
const int kOrchardTestnetPort = 8067;

/// Whether TLS is enabled by default
/// Set to false since lightwalletd servers don't have valid certificates
const bool kDefaultUseTls = false;

/// Lightwalletd endpoint configuration
@immutable
class LightdEndpoint {
  /// Server host
  final String host;

  /// Server port
  final int port;

  /// Whether TLS is enabled
  final bool useTls;

  /// Optional TLS certificate pin (SPKI hash, base64-encoded)
  /// When set, the app will verify the server's certificate against this pin.
  final String? tlsPin;

  /// User-provided label for this endpoint
  final String? label;

  const LightdEndpoint({
    required this.host,
    required this.port,
    this.useTls = kDefaultUseTls,
    this.tlsPin,
    this.label,
  });

  /// Default Pirate Chain endpoint (Orchard-ready mainnet)
  /// This is the working endpoint that replaces the old lightd1.piratechain.com
  static const LightdEndpoint defaultEndpoint = LightdEndpoint(
    host: kOrchardMainnetHost,
    port: kOrchardMainnetPort,
    useTls: kDefaultUseTls,
    label: 'Pirate Chain Official',
  );

  /// Orchard-capable preset endpoints for quick selection
  static const LightdEndpoint orchardMainnet = LightdEndpoint(
    host: kOrchardMainnetHost,
    port: kOrchardMainnetPort,
    useTls: kDefaultUseTls,
    label: 'Pirate Chain Official',
  );

  static const LightdEndpoint orchardTestnet = LightdEndpoint(
    host: kOrchardTestnetHost,
    port: kOrchardTestnetPort,
    useTls: kDefaultUseTls,
    label: 'Orchard Testnet',
  );

  /// Suggested endpoints presented in the node picker UI
  /// Note: Removed old "Pirate Chain Official" (lightd1.piratechain.com) as it doesn't work
  static const List<LightdEndpoint> suggested = <LightdEndpoint>[
    orchardMainnet,  // This is now the default and labeled "Pirate Chain Official"
    orchardTestnet,
  ];

  /// Full URL for gRPC connection
  String get url {
    final scheme = useTls ? 'https' : 'http';
    return '$scheme://$host:$port';
  }

  /// Display string (host:port)
  String get displayString => '$host:$port';

  /// Parse endpoint from URL string
  /// Accepts formats: "host:port", "https://host:port", "http://host:port"
  static LightdEndpoint? tryParse(String input, {String? tlsPin, String? label}) {
    var normalized = input.trim();
    var useTls = kDefaultUseTls; // Default to false (no TLS) since servers don't have certificates

    // Handle scheme prefix
    if (normalized.startsWith('https://')) {
      normalized = normalized.substring(8);
      useTls = true;
    } else if (normalized.startsWith('http://')) {
      normalized = normalized.substring(7);
      useTls = false;
    }

    // Remove trailing slash
    if (normalized.endsWith('/')) {
      normalized = normalized.substring(0, normalized.length - 1);
    }

    // Parse host:port
    final parts = normalized.split(':');
    if (parts.isEmpty || parts.length > 2) {
      return null;
    }

    final host = parts[0];
    if (host.isEmpty) {
      return null;
    }

    // Validate host (basic check)
    if (!_isValidHost(host)) {
      return null;
    }

    var port = kDefaultLightdPort;
    if (parts.length == 2) {
      final parsedPort = int.tryParse(parts[1]);
      if (parsedPort == null || parsedPort < 1 || parsedPort > 65535) {
        return null;
      }
      port = parsedPort;
    }

    return LightdEndpoint(
      host: host,
      port: port,
      useTls: useTls,
      tlsPin: tlsPin,
      label: label,
    );
  }

  /// Validate host string (basic validation)
  static bool _isValidHost(String host) {
    // Allow domain names and IP addresses
    // Domain: letters, numbers, dots, hyphens
    final domainRegex = RegExp(r'^[a-zA-Z0-9]([a-zA-Z0-9\-\.]*[a-zA-Z0-9])?$');
    // IPv4: 4 octets
    final ipv4Regex = RegExp(r'^(\d{1,3}\.){3}\d{1,3}$');
    // IPv6: simplified check
    final ipv6Regex = RegExp(r'^\[?[a-fA-F0-9:]+\]?$');

    return domainRegex.hasMatch(host) ||
        ipv4Regex.hasMatch(host) ||
        ipv6Regex.hasMatch(host);
  }

  /// Validate TLS pin format (base64-encoded SHA-256 hash)
  static bool isValidTlsPin(String pin) {
    // SPKI pin is base64-encoded SHA-256 (44 chars with padding)
    if (pin.length < 40 || pin.length > 48) {
      return false;
    }
    // Check for valid base64 characters
    final base64Regex = RegExp(r'^[A-Za-z0-9+/]+=*$');
    return base64Regex.hasMatch(pin);
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

  /// Convert to JSON for storage
  Map<String, dynamic> toJson() => {
        'host': host,
        'port': port,
        'useTls': useTls,
        if (tlsPin != null) 'tlsPin': tlsPin,
        if (label != null) 'label': label,
      };

  /// Create from JSON
  factory LightdEndpoint.fromJson(Map<String, dynamic> json) {
    return LightdEndpoint(
      host: json['host'] as String,
      port: json['port'] as int,
      useTls: json['useTls'] as bool? ?? true,
      tlsPin: json['tlsPin'] as String?,
      label: json['label'] as String?,
    );
  }
}

/// Secure storage key for persisted endpoint
const String kEndpointStorageKey = 'lightd_endpoint';

