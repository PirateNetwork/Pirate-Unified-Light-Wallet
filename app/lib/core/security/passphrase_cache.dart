// Biometric-wrapped passphrase storage for unlock flows.

import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import 'keystore_channel.dart';

class PassphraseCache {
  static const _storage = FlutterSecureStorage();
  static const _key = 'app_passphrase_wrapped_v1';

  static Future<void> store(String passphrase) async {
    final sealed = await KeystoreChannel.sealMasterKey(utf8.encode(passphrase));
    final encoded = base64Encode(sealed);
    await _storage.write(key: _key, value: encoded);
  }

  static Future<String?> read() async {
    final encoded = await _storage.read(key: _key);
    if (encoded == null || encoded.isEmpty) {
      return null;
    }
    final sealed = base64Decode(encoded);
    final unsealed = await KeystoreChannel.unsealMasterKey(sealed);
    return utf8.decode(unsealed, allowMalformed: false);
  }

  static Future<bool> exists() async {
    final encoded = await _storage.read(key: _key);
    return encoded != null && encoded.isNotEmpty;
  }

  static Future<void> clear() async {
    await _storage.delete(key: _key);
  }
}
