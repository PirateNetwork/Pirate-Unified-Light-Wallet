import 'dart:async';
import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import '../ffi/ffi_bridge.dart';
import '../ffi/generated/models.dart' show WalletMeta;

class BirthdayUpdateService {
  static const String _storageKey = 'pending_wallet_birthday_v1';
  static const FlutterSecureStorage _storage = FlutterSecureStorage();
  static final Set<String> _inFlight = <String>{};

  static Future<void> markPending(WalletId walletId, int fallbackHeight) async {
    final pending = await _readPending();
    pending[walletId] = fallbackHeight;
    await _writePending(pending);
  }

  static Future<void> clearPending(WalletId walletId) async {
    final pending = await _readPending();
    if (!pending.containsKey(walletId)) {
      return;
    }
    pending.remove(walletId);
    await _writePending(pending);
  }

  static Future<void> resumePendingUpdates({void Function()? onWalletsUpdated}) async {
    final pending = await _readPending();
    if (pending.isEmpty) {
      return;
    }

    List<WalletMeta> wallets;
    try {
      wallets = await FfiBridge.listWallets();
    } catch (_) {
      return;
    }

    final walletById = <String, WalletMeta>{
      for (final wallet in wallets) wallet.id: wallet,
    };

    final retained = <String, int>{};
    for (final entry in pending.entries) {
      final wallet = walletById[entry.key];
      if (wallet == null) {
        continue;
      }
      if (wallet.birthdayHeight != entry.value) {
        continue;
      }
      retained[entry.key] = entry.value;
      unawaited(updateWhenAvailable(
        entry.key,
        entry.value,
        onWalletsUpdated: onWalletsUpdated,
      ));
    }

    await _writePending(retained);
  }

  static Future<void> updateWhenAvailable(
    WalletId walletId,
    int fallbackHeight, {
    void Function()? onWalletsUpdated,
  }) async {
    if (_inFlight.contains(walletId)) {
      return;
    }
    _inFlight.add(walletId);
    var attempt = 0;

    try {
      while (true) {
        final height =
            await fetchLatestBirthdayHeight(walletId: walletId);
        if (height != null) {
          final currentHeight = await _getWalletBirthdayHeight(walletId);
          if (currentHeight == null || currentHeight != fallbackHeight) {
            await clearPending(walletId);
            return;
          }
          var setOk = false;
          try {
            await FfiBridge.setWalletBirthdayHeight(walletId, height);
            setOk = true;
          } catch (_) {}

          if (setOk) {
            try {
              await FfiBridge.rescan(walletId, height);
            } catch (_) {}
            onWalletsUpdated?.call();
            await clearPending(walletId);
            return;
          }
        }

        attempt += 1;
        final delaySeconds = attempt < 3 ? 10 : (attempt < 10 ? 30 : 60);
        await Future<void>.delayed(Duration(seconds: delaySeconds));
      }
    } finally {
      _inFlight.remove(walletId);
    }
  }

  static Future<int?> fetchLatestBirthdayHeight({WalletId? walletId}) async {
    String url = FfiBridge.defaultLightdUrl;
    String? tlsPin;

    if (walletId != null) {
      try {
        final config = await FfiBridge.getLightdEndpointConfig(walletId);
        url = config.url;
        tlsPin = config.tlsPin;
      } catch (_) {}
    }

    final result = await FfiBridge.testNode(url: url, tlsPin: tlsPin);
    final latest = result.latestBlockHeight;
    if (!result.success || latest == null) {
      return null;
    }
    final nextHeight = latest - 10;
    return nextHeight > 0 ? nextHeight : 1;
  }

  static Future<int?> _getWalletBirthdayHeight(WalletId walletId) async {
    try {
      final wallets = await FfiBridge.listWallets();
      for (final wallet in wallets) {
        if (wallet.id == walletId) {
          return wallet.birthdayHeight;
        }
      }
    } catch (_) {}
    return null;
  }

  static Future<Map<String, int>> _readPending() async {
    final raw = await _storage.read(key: _storageKey);
    if (raw == null || raw.isEmpty) {
      return <String, int>{};
    }
    try {
      final decoded = jsonDecode(raw);
      if (decoded is Map) {
        final pending = <String, int>{};
        for (final entry in decoded.entries) {
          final key = entry.key;
          final value = entry.value;
          if (key is! String) {
            continue;
          }
          final parsed = value is int ? value : int.tryParse(value.toString());
          if (parsed == null) {
            continue;
          }
          pending[key] = parsed;
        }
        return pending;
      }
    } catch (_) {}
    return <String, int>{};
  }

  static Future<void> _writePending(Map<String, int> pending) async {
    if (pending.isEmpty) {
      await _storage.delete(key: _storageKey);
      return;
    }
    await _storage.write(key: _storageKey, value: jsonEncode(pending));
  }
}
