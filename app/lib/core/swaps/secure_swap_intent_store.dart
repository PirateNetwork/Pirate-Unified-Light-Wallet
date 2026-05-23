import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import 'swap_intent_store.dart';
import 'swap_models.dart';

class SecureStorageSwapIntentStore extends SwapIntentStore {
  SecureStorageSwapIntentStore({FlutterSecureStorage? storage})
    : _storage = storage ?? const FlutterSecureStorage();

  static const _keyPrefix = 'pirate_swap_intents_v1';

  final FlutterSecureStorage _storage;

  @override
  Future<List<SwapIntent>> load(String walletId) async {
    final raw = await _storage.read(key: _key(walletId));
    if (raw == null || raw.trim().isEmpty) return <SwapIntent>[];

    try {
      final decoded = jsonDecode(raw) as List;
      return decoded
          .map(
            (entry) =>
                SwapIntent.fromJson(Map<String, dynamic>.from(entry as Map)),
          )
          .toList();
    } catch (_) {
      return <SwapIntent>[];
    }
  }

  @override
  Future<void> save(String walletId, List<SwapIntent> intents) async {
    final encoded = jsonEncode(
      intents.map((intent) => intent.toJson()).toList(),
    );
    await _storage.write(key: _key(walletId), value: encoded);
  }

  String _key(String walletId) => '$_keyPrefix.$walletId';
}
