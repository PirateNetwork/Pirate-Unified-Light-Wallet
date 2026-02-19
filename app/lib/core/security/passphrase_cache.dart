// Biometric-wrapped passphrase storage for unlock flows.

import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:path_provider/path_provider.dart';

import 'keystore_channel.dart';

class PassphraseCache {
  static const _storage = FlutterSecureStorage();
  static const _key = 'app_passphrase_wrapped_v1';
  static const _fallbackFileName = 'app_passphrase_wrapped_v1.txt';

  static Future<void> store(String passphrase) async {
    final sealed = await KeystoreChannel.sealMasterKey(utf8.encode(passphrase));
    final encoded = base64Encode(sealed);
    if (Platform.isMacOS) {
      await _writeFallbackMarker(encoded);
      // Best-effort compatibility path for already provisioned builds.
      try {
        await _storage.write(key: _key, value: encoded);
      } catch (_) {}
      return;
    }
    await _storage.write(key: _key, value: encoded);
  }

  static Future<String?> read() async {
    final encoded = await _readEncodedMarker();
    if (encoded == null || encoded.isEmpty) {
      return null;
    }
    final sealed = base64Decode(encoded);
    final unsealed = await KeystoreChannel.unsealMasterKey(sealed);
    return utf8.decode(unsealed, allowMalformed: false);
  }

  static Future<bool> exists() async {
    final encoded = await _readEncodedMarker();
    return encoded != null && encoded.isNotEmpty;
  }

  static Future<void> clear() async {
    if (Platform.isMacOS) {
      await _deleteFallbackMarker();
      try {
        await _storage.delete(key: _key);
      } catch (_) {}
      return;
    }
    await _storage.delete(key: _key);
  }

  static Future<String?> _readEncodedMarker() async {
    if (!Platform.isMacOS) {
      return _storage.read(key: _key);
    }

    final fallback = await _readFallbackMarker();
    if (fallback != null && fallback.isNotEmpty) {
      return fallback;
    }

    try {
      final secureStorageValue = await _storage.read(key: _key);
      if (secureStorageValue != null && secureStorageValue.isNotEmpty) {
        await _writeFallbackMarker(secureStorageValue);
      }
      return secureStorageValue;
    } catch (e) {
      debugPrint('PassphraseCache secure-storage marker read failed: $e');
      return null;
    }
  }

  static Future<File> _fallbackMarkerFile({
    required bool ensureParentDir,
  }) async {
    final supportDir = await getApplicationSupportDirectory();
    final securityDir = Directory('${supportDir.path}/security');
    if (ensureParentDir && !securityDir.existsSync()) {
      securityDir.createSync(recursive: true);
    }
    return File('${securityDir.path}/$_fallbackFileName');
  }

  static Future<String?> _readFallbackMarker() async {
    try {
      final file = await _fallbackMarkerFile(ensureParentDir: false);
      if (!file.existsSync()) {
        return null;
      }
      final value = await file.readAsString();
      return value.trim().isEmpty ? null : value.trim();
    } catch (e) {
      debugPrint('PassphraseCache fallback marker read failed: $e');
      return null;
    }
  }

  static Future<void> _writeFallbackMarker(String encoded) async {
    final file = await _fallbackMarkerFile(ensureParentDir: true);
    await file.writeAsString(encoded, flush: true);
  }

  static Future<void> _deleteFallbackMarker() async {
    try {
      final file = await _fallbackMarkerFile(ensureParentDir: false);
      if (file.existsSync()) {
        await file.delete();
      }
    } catch (e) {
      debugPrint('PassphraseCache fallback marker delete failed: $e');
    }
  }
}
