/// Secure storage for duress passphrase hash.

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

class DuressPassphraseStore {
  static const _storage = FlutterSecureStorage();
  static const _key = 'duress_passphrase_hash_v1';

  static Future<void> store(String hash) async {
    await _storage.write(key: _key, value: hash);
  }

  static Future<String?> read() async {
    final value = await _storage.read(key: _key);
    if (value == null || value.isEmpty) {
      return null;
    }
    return value;
  }

  static Future<bool> exists() async {
    final value = await _storage.read(key: _key);
    return value != null && value.isNotEmpty;
  }

  static Future<void> clear() async {
    await _storage.delete(key: _key);
  }
}
