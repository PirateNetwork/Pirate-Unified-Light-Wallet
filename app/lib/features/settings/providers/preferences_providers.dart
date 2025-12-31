import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import '../../../core/security/biometric_auth.dart';
import '../../../core/security/keystore_channel.dart';
import '../../../core/ffi/ffi_bridge.dart';

enum AppThemeMode {
  system,
  light,
  dark,
}

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

enum CurrencyPreference {
  arrr,
  usd,
  eur,
  gbp,
  btc,
}

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

  @override
  bool build() {
    _storage = const FlutterSecureStorage();
    _load();
    return false;
  }

  Future<void> setEnabled({required bool enabled}) async {
    final previous = state;
    state = enabled;
    await _storage.write(key: _storageKey, value: enabled.toString());
    if (Platform.isAndroid || Platform.isIOS) {
      try {
        await KeystoreChannel.setBiometricsEnabled(enabled);
        await FfiBridge.resealDbKeysForBiometrics();
      } catch (e) {
        state = previous;
        await _storage.write(key: _storageKey, value: previous.toString());
        try {
          await KeystoreChannel.setBiometricsEnabled(previous);
        } catch (_) {}
        rethrow;
      }
    }
  }

  Future<void> _load() async {
    final raw = await _storage.read(key: _storageKey);
    if (raw == null || raw.isEmpty) {
      state = false;
      return;
    }
    state = raw.toLowerCase() == 'true';
  }
}

final appThemeModeProvider =
    NotifierProvider<ThemeModeNotifier, AppThemeMode>(ThemeModeNotifier.new);

final currencyPreferenceProvider =
    NotifierProvider<CurrencyPreferenceNotifier, CurrencyPreference>(CurrencyPreferenceNotifier.new);

final biometricsEnabledProvider =
    NotifierProvider<BiometricsPreferenceNotifier, bool>(BiometricsPreferenceNotifier.new);

final biometricAvailabilityProvider = FutureProvider<bool>((ref) async {
  return BiometricAuth.isAvailable();
});
