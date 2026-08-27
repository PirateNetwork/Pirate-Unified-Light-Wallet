import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:path_provider/path_provider.dart';

typedef DebugPreferenceRead = Future<String?> Function(String key);
typedef DebugPreferenceWrite = Future<void> Function(
  String key,
  String value,
);
typedef DebugPreferenceDirectory = Future<Directory> Function();

/// Persists the non-secret debug logging preference without making macOS
/// Keychain availability a prerequisite for collecting diagnostics.
class DebugLogPreferenceStore {
  DebugLogPreferenceStore({
    required DebugPreferenceRead secureRead,
    required DebugPreferenceWrite secureWrite,
    required DebugPreferenceDirectory supportDirectory,
    required bool preferFile,
  }) : _secureRead = secureRead,
       _secureWrite = secureWrite,
       _supportDirectory = supportDirectory,
       _preferFile = preferFile;

  factory DebugLogPreferenceStore.platform() {
    const storage = FlutterSecureStorage();
    return DebugLogPreferenceStore(
      secureRead: (key) => storage.read(key: key),
      secureWrite: (key, value) => storage.write(key: key, value: value),
      supportDirectory: getApplicationSupportDirectory,
      preferFile: Platform.isMacOS,
    );
  }

  static const fallbackFileName = 'ui_debug_logging_enabled_v1.txt';

  final DebugPreferenceRead _secureRead;
  final DebugPreferenceWrite _secureWrite;
  final DebugPreferenceDirectory _supportDirectory;
  final bool _preferFile;

  Future<bool> read({required String key}) async {
    if (_preferFile) {
      final fallback = await _readFallback();
      if (fallback != null) {
        return _parse(fallback);
      }
    }

    try {
      final secure = await _secureRead(key);
      if (secure != null && secure.isNotEmpty) {
        await _writeFallbackBestEffort(secure);
        return _parse(secure);
      }
    } catch (error) {
      debugPrint('Debug preference secure-storage read failed: $error');
    }

    if (!_preferFile) {
      final fallback = await _readFallback();
      if (fallback != null) {
        return _parse(fallback);
      }
    }
    return false;
  }

  Future<void> write({required String key, required bool enabled}) async {
    final value = enabled.toString();
    if (_preferFile) {
      // On macOS this file is the durable source of truth. Debug logging is a
      // diagnostic preference, not a secret, and must remain usable when the
      // Keychain reports an entitlement or availability failure.
      await _writeFallback(value);
      try {
        await _secureWrite(key, value);
      } catch (error) {
        debugPrint('Debug preference secure-storage write failed: $error');
      }
      return;
    }

    Object? secureError;
    try {
      await _secureWrite(key, value);
      await _writeFallbackBestEffort(value);
      return;
    } catch (error) {
      secureError = error;
      debugPrint('Debug preference secure-storage write failed: $error');
    }

    try {
      await _writeFallback(value);
    } catch (fallbackError) {
      throw FileSystemException(
        'Could not persist debug logging preference '
        '(secure storage: $secureError; file: $fallbackError)',
      );
    }
  }

  bool _parse(String value) => value.trim().toLowerCase() == 'true';

  Future<File> _fallbackFile({required bool ensureParent}) async {
    final support = await _supportDirectory();
    final preferences = Directory(
      '${support.path}${Platform.pathSeparator}preferences',
    );
    if (ensureParent && !preferences.existsSync()) {
      await preferences.create(recursive: true);
    }
    return File(
      '${preferences.path}${Platform.pathSeparator}$fallbackFileName',
    );
  }

  Future<String?> _readFallback() async {
    try {
      final file = await _fallbackFile(ensureParent: false);
      if (!file.existsSync()) {
        return null;
      }
      final value = (await file.readAsString()).trim();
      return value.isEmpty ? null : value;
    } catch (error) {
      debugPrint('Debug preference fallback read failed: $error');
      return null;
    }
  }

  Future<void> _writeFallback(String value) async {
    final file = await _fallbackFile(ensureParent: true);
    await file.writeAsString(value, flush: true);
  }

  Future<void> _writeFallbackBestEffort(String value) async {
    try {
      await _writeFallback(value);
    } catch (error) {
      debugPrint('Debug preference fallback write failed: $error');
    }
  }
}
