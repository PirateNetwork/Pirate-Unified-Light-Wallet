import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/providers/wallet_providers.dart';

/// Tor status (not_started/bootstrapping/ready/error)
class TorStatusNotifier extends Notifier<String> {
  @override
  String build() => 'not_started';
}

final torStatusProvider = NotifierProvider<TorStatusNotifier, String>(TorStatusNotifier.new);

/// Transport configuration persistence
class TransportConfig {
  final String mode;
  final String dnsProvider;
  final Map<String, String?> socks5Config;
  final List<Map<String, String>> tlsPins;

  const TransportConfig({
    required this.mode,
    required this.dnsProvider,
    required this.socks5Config,
    required this.tlsPins,
  });

  Map<String, dynamic> toJson() => {
    'mode': mode,
    'dns_provider': dnsProvider,
    'socks5': socks5Config,
    'tls_pins': tlsPins,
  };

  factory TransportConfig.fromJson(Map<String, dynamic> json) {
    return TransportConfig(
      mode: json['mode'] as String? ?? 'direct',
      dnsProvider: json['dns_provider'] as String? ?? 'cloudflare_doh',
      socks5Config: Map<String, String?>.from(json['socks5'] as Map? ?? {}),
      tlsPins: List<Map<String, String>>.from(
        (json['tls_pins'] as List?)?.map((pin) => Map<String, String>.from(pin as Map)) ?? [],
      ),
    );
  }

  TransportConfig copyWith({
    String? mode,
    String? dnsProvider,
    Map<String, String?>? socks5Config,
    List<Map<String, String>>? tlsPins,
  }) {
    return TransportConfig(
      mode: mode ?? this.mode,
      dnsProvider: dnsProvider ?? this.dnsProvider,
      socks5Config: socks5Config ?? this.socks5Config,
      tlsPins: tlsPins ?? this.tlsPins,
    );
  }
}

/// Save/load transport configuration
class TransportConfigNotifier extends Notifier<TransportConfig> {
  late final FlutterSecureStorage _storage;
  static const String _storageKey = 'transport_config_v1';

  @override
  TransportConfig build() {
    _storage = const FlutterSecureStorage();
    _load();
    return const TransportConfig(
      mode: 'direct',
      dnsProvider: 'cloudflare_doh',
      socks5Config: {
        'host': 'localhost',
        'port': '1080',
        'username': null,
        'password': null,
      },
      tlsPins: [],
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
          mode: 'direct',
          dnsProvider: 'cloudflare_doh',
          socks5Config: {
            'host': 'localhost',
            'port': '1080',
            'username': null,
            'password': null,
          },
          tlsPins: [],
        );
      }
    }
    await _applyTunnel(state);
  }

  Future<void> _applyTunnel(TransportConfig config) async {
    try {
      final mode = config.mode.toLowerCase();
      final tunnelNotifier = ref.read(tunnelModeProvider.notifier);
      if (mode == 'socks5') {
        final url = _buildSocks5Url(config.socks5Config);
        await tunnelNotifier.setSocks5(url);
        return;
      }
      if (mode == 'direct') {
        await tunnelNotifier.setDirect();
        return;
      }
      await tunnelNotifier.setTor();
    } catch (_) {
      // Ignore errors when the FFI layer isn't ready yet.
    }
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

