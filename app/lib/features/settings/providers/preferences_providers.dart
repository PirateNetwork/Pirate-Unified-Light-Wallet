import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:path_provider/path_provider.dart';

import '../../../core/security/biometric_auth.dart';
import '../../../core/security/keystore_channel.dart';
import '../../../core/security/passphrase_cache.dart';
import '../../../core/ffi/ffi_bridge.dart';

enum AppThemeMode { system, light, dark }

extension AppThemeModeX on AppThemeMode {
  ThemeMode get themeMode {
    switch (this) {
      case AppThemeMode.light:
        return ThemeMode.light;
      case AppThemeMode.dark:
        return ThemeMode.dark;
      case AppThemeMode.system:
        return ThemeMode.system;
    }
  }

  String get label {
    switch (this) {
      case AppThemeMode.light:
        return 'Light';
      case AppThemeMode.dark:
        return 'Dark';
      case AppThemeMode.system:
        return 'System';
    }
  }
}

enum CurrencyPreference { arrr, usd, eur, gbp, btc }

extension CurrencyPreferenceX on CurrencyPreference {
  String get code {
    switch (this) {
      case CurrencyPreference.usd:
        return 'USD';
      case CurrencyPreference.eur:
        return 'EUR';
      case CurrencyPreference.gbp:
        return 'GBP';
      case CurrencyPreference.btc:
        return 'BTC';
      case CurrencyPreference.arrr:
        return 'ARRR';
    }
  }

  String get label {
    switch (this) {
      case CurrencyPreference.usd:
        return 'US Dollar';
      case CurrencyPreference.eur:
        return 'Euro';
      case CurrencyPreference.gbp:
        return 'British Pound';
      case CurrencyPreference.btc:
        return 'Bitcoin';
      case CurrencyPreference.arrr:
        return 'Pirate Chain';
    }
  }
}

class ThemeModeNotifier extends Notifier<AppThemeMode> {
  late final FlutterSecureStorage _storage;
  static const String _storageKey = 'ui_theme_mode_v1';

  @override
  AppThemeMode build() {
    _storage = const FlutterSecureStorage();
    _load();
    return AppThemeMode.system;
  }

  Future<void> setThemeMode(AppThemeMode mode) async {
    state = mode;
    await _storage.write(key: _storageKey, value: mode.name);
  }

  Future<void> _load() async {
    final raw = await _storage.read(key: _storageKey);
    if (raw == null || raw.isEmpty) {
      state = AppThemeMode.system;
      return;
    }
    state = AppThemeMode.values.firstWhere(
      (mode) => mode.name == raw,
      orElse: () => AppThemeMode.system,
    );
  }
}

class CurrencyPreferenceNotifier extends Notifier<CurrencyPreference> {
  late final FlutterSecureStorage _storage;
  static const String _storageKey = 'ui_currency_pref_v1';

  @override
  CurrencyPreference build() {
    _storage = const FlutterSecureStorage();
    _load();
    return CurrencyPreference.arrr;
  }

  Future<void> setCurrency(CurrencyPreference preference) async {
    state = preference;
    await _storage.write(key: _storageKey, value: preference.name);
  }

  Future<void> _load() async {
    final raw = await _storage.read(key: _storageKey);
    if (raw == null || raw.isEmpty) {
      state = CurrencyPreference.arrr;
      return;
    }
    state = CurrencyPreference.values.firstWhere(
      (pref) => pref.name == raw,
      orElse: () => CurrencyPreference.arrr,
    );
  }
}

class BiometricsPreferenceNotifier extends Notifier<bool> {
  late final FlutterSecureStorage _storage;
  static const String _storageKey = 'ui_biometrics_enabled_v1';
  static const String _fallbackFileName = 'ui_biometrics_enabled_v1.txt';

  @override
  bool build() {
    _storage = const FlutterSecureStorage();
    _load();
    return false;
  }

  Future<void> setEnabled({required bool enabled}) async {
    final previous = state;
    if (enabled == previous) {
      return;
    }
    state = enabled;

    var persisted = false;
    try {
      await _storage.write(key: _storageKey, value: enabled.toString());
      persisted = true;
    } catch (e) {
      // This is a non-sensitive UI preference. If secure storage persistence
      // fails on a device, fall back to file-backed persistence.
      debugPrint('Failed to persist biometrics preference: $e');
    }
    if (await _writeFallbackValue(enabled)) {
      persisted = true;
    }
    if (!persisted) {
      state = previous;
      throw Exception('Failed to persist biometrics preference');
    }

    String? cachedPassphraseForReseal;

    try {
      if (Platform.isAndroid || Platform.isIOS) {
        // If we already have a cached passphrase, preserve it across keystore
        // mode changes (biometric <-> non-biometric).
        if (enabled && await PassphraseCache.exists()) {
          cachedPassphraseForReseal = await PassphraseCache.read();
          if (cachedPassphraseForReseal == null ||
              cachedPassphraseForReseal.isEmpty) {
            throw Exception(
              'Cached passphrase is unavailable for biometric reseal.',
            );
          }
        }

        if (!enabled) {
          await PassphraseCache.clear();
        }

        await KeystoreChannel.setBiometricsEnabled(enabled: enabled);

        if (enabled && cachedPassphraseForReseal != null) {
          await PassphraseCache.store(cachedPassphraseForReseal);
        }

        await FfiBridge.resealDbKeysForBiometrics();
      } else if (!enabled) {
        await PassphraseCache.clear();
      }
    } catch (e) {
      state = previous;
      await _persistBestEffort(previous);

      if (Platform.isAndroid || Platform.isIOS) {
        try {
          await KeystoreChannel.setBiometricsEnabled(enabled: previous);
        } catch (rollbackError) {
          debugPrint(
            'Failed to rollback mobile biometric keystore mode: $rollbackError',
          );
        }
        if (cachedPassphraseForReseal != null) {
          try {
            await PassphraseCache.store(cachedPassphraseForReseal);
          } catch (rollbackError) {
            debugPrint(
              'Failed to restore cached passphrase after biometric rollback: $rollbackError',
            );
          }
        }
      }

      rethrow;
    }
  }

  Future<void> _load() async {
    try {
      final raw = await _storage.read(key: _storageKey);
      final parsed = _parseBool(raw);
      if (parsed != null) {
        state = parsed;
        return;
      }
    } catch (e) {
      debugPrint(
        'Failed to read biometrics preference from secure storage: $e',
      );
    }

    final fallback = await _readFallbackValue();
    if (fallback != null) {
      state = fallback;
    } else {
      state = false;
    }

    if (state) {
      final available = await BiometricAuth.isAvailable();
      if (!available) {
        debugPrint(
          'Biometrics preference was enabled but no biometrics are currently available. Resetting to disabled.',
        );
        state = false;
        await _persistBestEffort(false);
        return;
      }

      try {
        final hasWrappedPassphrase = await PassphraseCache.exists();
        if (!hasWrappedPassphrase) {
          debugPrint(
            'Biometrics preference was enabled but no wrapped passphrase cache exists. Resetting to disabled.',
          );
          state = false;
          await _persistBestEffort(false);
        }
      } catch (e) {
        debugPrint(
          'Failed to verify wrapped passphrase cache for biometrics: $e',
        );
        state = false;
        await _persistBestEffort(false);
      }
    }
  }

  Future<bool> readPersistedValue() async {
    bool enabled = false;
    try {
      final raw = await _storage.read(key: _storageKey);
      final parsed = _parseBool(raw);
      if (parsed != null) {
        enabled = parsed;
      }
    } catch (e) {
      debugPrint(
        'Failed to read biometrics preference from secure storage: $e',
      );
    }

    if (!enabled) {
      final fallback = await _readFallbackValue();
      if (fallback != null) {
        enabled = fallback;
      }
    }

    if (!enabled) {
      return false;
    }

    final available = await BiometricAuth.isAvailable();
    if (!available) {
      await _persistBestEffort(false);
      return false;
    }

    try {
      final hasWrappedPassphrase = await PassphraseCache.exists();
      if (!hasWrappedPassphrase) {
        await _persistBestEffort(false);
        return false;
      }
    } catch (e) {
      debugPrint(
        'Failed to verify wrapped passphrase cache for persisted biometrics preference: $e',
      );
      await _persistBestEffort(false);
      return false;
    }

    return true;
  }

  bool? _parseBool(String? raw) {
    if (raw == null || raw.isEmpty) {
      return null;
    }
    final normalized = raw.trim().toLowerCase();
    if (normalized == 'true') return true;
    if (normalized == 'false') return false;
    return null;
  }

  Future<File> _fallbackFile({required bool ensureParentDir}) async {
    final supportDir = await getApplicationSupportDirectory();
    final prefsDir = Directory('${supportDir.path}/prefs');
    if (ensureParentDir && !prefsDir.existsSync()) {
      prefsDir.createSync(recursive: true);
    }
    return File('${prefsDir.path}/$_fallbackFileName');
  }

  Future<bool?> _readFallbackValue() async {
    try {
      final file = await _fallbackFile(ensureParentDir: false);
      if (!file.existsSync()) {
        return null;
      }
      return _parseBool(file.readAsStringSync());
    } catch (e) {
      debugPrint('Failed to read biometrics fallback preference: $e');
      return null;
    }
  }

  Future<bool> _writeFallbackValue(bool enabled) async {
    try {
      final file = await _fallbackFile(ensureParentDir: true);
      await file.writeAsString(enabled ? 'true' : 'false', flush: true);
      return true;
    } catch (e) {
      debugPrint('Failed to persist biometrics fallback preference: $e');
      return false;
    }
  }

  Future<void> _persistBestEffort(bool enabled) async {
    try {
      await _storage.write(key: _storageKey, value: enabled.toString());
    } catch (e) {
      debugPrint('Failed to rollback biometrics secure-storage value: $e');
    }
    final fallbackOk = await _writeFallbackValue(enabled);
    if (!fallbackOk) {
      debugPrint('Failed to rollback biometrics fallback value.');
    }
  }
}

final appThemeModeProvider = NotifierProvider<ThemeModeNotifier, AppThemeMode>(
  ThemeModeNotifier.new,
);

final currencyPreferenceProvider =
    NotifierProvider<CurrencyPreferenceNotifier, CurrencyPreference>(
      CurrencyPreferenceNotifier.new,
    );

final biometricsEnabledProvider =
    NotifierProvider<BiometricsPreferenceNotifier, bool>(
      BiometricsPreferenceNotifier.new,
    );

final biometricAvailabilityProvider = FutureProvider<bool>((ref) async {
  return BiometricAuth.isAvailable();
});
