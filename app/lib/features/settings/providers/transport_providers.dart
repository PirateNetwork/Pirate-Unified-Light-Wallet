import 'dart:async';
import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/ffi/generated/models.dart' show TunnelMode;
import '../../../core/providers/wallet_providers.dart';

/// Tor status details for UI.
class TorStatusNotifier extends Notifier<TorStatusDetails> {
  Timer? _timer;

  @override
  TorStatusDetails build() {
    _startPolling();
    ref.onDispose(() => _timer?.cancel());
    return const TorStatusDetails(status: 'not_started');
  }

  void _startPolling() {
    _timer?.cancel();
    _timer = Timer.periodic(const Duration(seconds: 3), (_) async {
      await _refresh();
    });
    // Kick off an immediate fetch.
    unawaited(_refresh());
  }

  Future<void> _refresh() async {
    try {
      final status = await FfiBridge.getTorStatusDetails();
      if (!ref.mounted) return;
      state = status;
    } catch (_) {
      if (!ref.mounted) return;
      state = const TorStatusDetails(status: 'error');
    }
  }
}

final torStatusProvider = NotifierProvider<TorStatusNotifier, TorStatusDetails>(TorStatusNotifier.new);

/// Transport configuration persistence
class TorBridgeConfig {
  final bool useBridges;
  final bool fallbackToBridges;
  final String transport;
  final List<String> bridgeLines;
  final String? transportPath;

  const TorBridgeConfig({
    required this.useBridges,
    required this.fallbackToBridges,
    required this.transport,
    required this.bridgeLines,
    required this.transportPath,
  });

  TorBridgeConfig copyWith({
    bool? useBridges,
    bool? fallbackToBridges,
    String? transport,
    List<String>? bridgeLines,
    String? transportPath,
  }) {
    return TorBridgeConfig(
      useBridges: useBridges ?? this.useBridges,
      fallbackToBridges: fallbackToBridges ?? this.fallbackToBridges,
      transport: transport ?? this.transport,
      bridgeLines: bridgeLines ?? this.bridgeLines,
      transportPath: transportPath ?? this.transportPath,
    );
  }

  Map<String, dynamic> toJson() => {
    'use_bridges': useBridges,
    'fallback_to_bridges': fallbackToBridges,
    'transport': transport,
    'bridge_lines': bridgeLines,
    'transport_path': transportPath,
  };

  factory TorBridgeConfig.fromJson(Map<String, dynamic> json) {
    final rawLines = json['bridge_lines'];
    final bridgeLines = rawLines is List
        ? rawLines.map((line) => line.toString()).toList()
        : rawLines is String
            ? rawLines
                .split(RegExp(r'\r?\n'))
                .map((line) => line.trim())
                .where((line) => line.isNotEmpty)
                .toList()
            : <String>[];
    return TorBridgeConfig(
      useBridges: json['use_bridges'] as bool? ?? false,
      fallbackToBridges: json['fallback_to_bridges'] as bool? ?? true,
      transport: json['transport'] as String? ?? 'snowflake',
      bridgeLines: bridgeLines,
      transportPath: json['transport_path'] as String?,
    );
  }
}

class TransportConfig {
  final String mode;
  final String dnsProvider;
  final Map<String, String?> socks5Config;
  final String i2pEndpoint;
  final List<Map<String, String>> tlsPins;
  final TorBridgeConfig torBridge;

  const TransportConfig({
    required this.mode,
    required this.dnsProvider,
    required this.socks5Config,
    required this.i2pEndpoint,
    required this.tlsPins,
    required this.torBridge,
  });

  Map<String, dynamic> toJson() => {
    'mode': mode,
    'dns_provider': dnsProvider,
    'socks5': socks5Config,
    'i2p_endpoint': i2pEndpoint,
    'tls_pins': tlsPins,
    'tor_bridge': torBridge.toJson(),
  };

  factory TransportConfig.fromJson(Map<String, dynamic> json) {
    final torBridgeJson = json['tor_bridge'] as Map<String, dynamic>? ?? {};
    return TransportConfig(
      mode: json['mode'] as String? ?? 'tor',
      dnsProvider: json['dns_provider'] as String? ?? 'cloudflare_doh',
      socks5Config: Map<String, String?>.from(json['socks5'] as Map? ?? {}),
      i2pEndpoint: json['i2p_endpoint'] as String? ?? '',
      tlsPins: List<Map<String, String>>.from(
        (json['tls_pins'] as List?)?.map((pin) => Map<String, String>.from(pin as Map)) ?? [],
      ),
      torBridge: TorBridgeConfig.fromJson(torBridgeJson),
    );
  }

  TransportConfig copyWith({
    String? mode,
    String? dnsProvider,
    Map<String, String?>? socks5Config,
    String? i2pEndpoint,
    List<Map<String, String>>? tlsPins,
    TorBridgeConfig? torBridge,
  }) {
    return TransportConfig(
      mode: mode ?? this.mode,
      dnsProvider: dnsProvider ?? this.dnsProvider,
      socks5Config: socks5Config ?? this.socks5Config,
      i2pEndpoint: i2pEndpoint ?? this.i2pEndpoint,
      tlsPins: tlsPins ?? this.tlsPins,
      torBridge: torBridge ?? this.torBridge,
    );
  }
}

/// Save/load transport configuration
class TransportConfigNotifier extends Notifier<TransportConfig> {
  late final FlutterSecureStorage _storage;
  static const String _storageKey = 'transport_config_v1';
  static const String _storageNonI2pEndpointKey = 'transport_non_i2p_endpoint_v1';
  static const String _storageNonI2pTlsPinKey = 'transport_non_i2p_tls_pin_v1';

  @override
  TransportConfig build() {
    _storage = const FlutterSecureStorage();
    _load();
    return const TransportConfig(
      mode: 'tor',
      dnsProvider: 'cloudflare_doh',
      socks5Config: {
        'host': 'localhost',
        'port': '1080',
        'username': null,
        'password': null,
      },
      i2pEndpoint: '',
      tlsPins: [],
      torBridge: TorBridgeConfig(
        useBridges: false,
        fallbackToBridges: true,
        transport: 'snowflake',
        bridgeLines: [],
        transportPath: null,
      ),
    );
  }

  Future<void> setMode(String mode) async {
    state = state.copyWith(mode: mode);
    await _applyTunnel(state);
    await _persist();
  }

  Future<void> setDnsProvider(String provider) async {
    state = state.copyWith(dnsProvider: provider);
    await _persist();
  }

  Future<void> setSocks5Config(Map<String, String?> config) async {
    state = state.copyWith(socks5Config: config);
    if (state.mode == 'socks5') {
      await _applyTunnel(state);
    }
    await _persist();
  }

  Future<void> setI2pEndpoint(String endpoint) async {
    state = state.copyWith(i2pEndpoint: endpoint.trim());
    await _persist();
    if (state.mode == 'i2p') {
      await _applyI2pEndpoint(state.i2pEndpoint);
    }
  }

  Future<void> setTlsPins(List<Map<String, String>> pins) async {
    state = state.copyWith(tlsPins: pins);
    await _persist();
  }

  Future<void> refresh() => _load();

  Future<void> _persist() async {
    final encoded = jsonEncode(state.toJson());
    await _storage.write(key: _storageKey, value: encoded);
  }

  Future<void> _load() async {
    final raw = await _storage.read(key: _storageKey);
    if (raw != null && raw.isNotEmpty) {
      try {
        final decoded = jsonDecode(raw) as Map<String, dynamic>;
        state = TransportConfig.fromJson(decoded);
      } catch (_) {
        state = const TransportConfig(
          mode: 'tor',
          dnsProvider: 'cloudflare_doh',
          socks5Config: {
            'host': 'localhost',
            'port': '1080',
            'username': null,
            'password': null,
          },
          i2pEndpoint: '',
          tlsPins: [],
          torBridge: TorBridgeConfig(
            useBridges: false,
            fallbackToBridges: true,
            transport: 'snowflake',
            bridgeLines: [],
            transportPath: null,
          ),
        );
      }
    }
    await _applyTunnel(state);
  }

  Future<void> setTorBridgeConfig(TorBridgeConfig config, {bool apply = true}) async {
    state = state.copyWith(torBridge: config);
    await _persist();
    if (apply && state.mode == 'tor') {
      await _applyTunnel(state);
    }
  }

  Future<void> _applyTunnel(TransportConfig config) async {
    try {
      await _pauseActiveSync();
      final mode = config.mode.toLowerCase();
      final tunnelNotifier = ref.read(tunnelModeProvider.notifier);
      if (mode != 'i2p') {
        await _restoreNonI2pEndpointIfNeeded();
      }
      if (mode == 'socks5') {
        final url = _buildSocks5Url(config.socks5Config);
        await tunnelNotifier.setSocks5(url);
        return;
      }
      if (mode == 'i2p') {
        await _storeNonI2pEndpointIfNeeded();
        await _applyI2pEndpoint(config.i2pEndpoint);
        await tunnelNotifier.setI2p();
        return;
      }
      if (mode == 'direct') {
        await tunnelNotifier.setDirect();
        return;
      }
      await FfiBridge.setTorBridgeSettings(
        useBridges: config.torBridge.useBridges,
        fallbackToBridges: config.torBridge.fallbackToBridges,
        transport: config.torBridge.transport,
        bridgeLines: config.torBridge.bridgeLines,
        transportPath: config.torBridge.transportPath,
      );
      await tunnelNotifier.setTor();
    } catch (_) {
      // Ignore errors when the FFI layer isn't ready yet.
    }
  }

  Future<void> _storeNonI2pEndpointIfNeeded() async {
    try {
      final currentTunnel = await FfiBridge.getTunnel();
      if (currentTunnel.name == 'i2p') {
        return;
      }
      final endpointConfig = await ref.read(lightdEndpointConfigProvider.future);
      await _storage.write(
        key: _storageNonI2pEndpointKey,
        value: endpointConfig.url,
      );
      await _storage.write(
        key: _storageNonI2pTlsPinKey,
        value: endpointConfig.tlsPin ?? '',
      );
    } catch (_) {}
  }

  Future<void> _restoreNonI2pEndpointIfNeeded() async {
    try {
      final currentTunnel = await FfiBridge.getTunnel();
      if (currentTunnel.name != 'i2p') {
        return;
      }
      final endpoint =
          (await _storage.read(key: _storageNonI2pEndpointKey))?.trim();
      final tlsPin =
          (await _storage.read(key: _storageNonI2pTlsPinKey))?.trim();
      final restoreUrl = (endpoint == null || endpoint.isEmpty)
          ? FfiBridge.defaultLightdUrl
          : endpoint;
      await ref.read(setLightdEndpointProvider)(
        url: restoreUrl,
        tlsPin: (tlsPin == null || tlsPin.isEmpty) ? null : tlsPin,
      );
    } catch (_) {}
  }

  Future<void> _applyI2pEndpoint(String endpoint) async {
    final trimmed = endpoint.trim();
    if (trimmed.isEmpty) {
      return;
    }
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) {
      return;
    }
    await ref.read(setLightdEndpointProvider)(
      url: trimmed,
      tlsPin: null,
    );
  }

  Future<void> _pauseActiveSync() async {
    final walletId = ref.read(activeWalletProvider);
    if (walletId == null) return;
    try {
      await FfiBridge.cancelSync(walletId);
    } catch (_) {}
    ref.invalidate(syncStatusProvider);
    ref.invalidate(syncProgressStreamProvider);
    ref.invalidate(isSyncRunningProvider);
  }

  String _buildSocks5Url(Map<String, String?> config) {
    final host = (config['host'] ?? '').trim();
    final port = (config['port'] ?? '1080').trim();
    final username = config['username'];
    final password = config['password'];
    if (host.isEmpty) {
      return 'socks5://localhost:1080';
    }

    final hasUser = username != null && username.isNotEmpty;
    final hasPass = password != null && password.isNotEmpty;
    final auth = hasUser
        ? '${Uri.encodeComponent(username)}${hasPass ? ':${Uri.encodeComponent(password!)}' : ''}@'
        : '';
    final portPart = port.isNotEmpty ? ':$port' : '';
    return 'socks5://$auth$host$portPart';
  }
}

final transportConfigProvider = NotifierProvider<TransportConfigNotifier, TransportConfig>(TransportConfigNotifier.new);
