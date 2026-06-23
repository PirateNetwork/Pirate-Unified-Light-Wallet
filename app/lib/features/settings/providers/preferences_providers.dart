import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:path_provider/path_provider.dart';

import '../../../core/security/biometric_auth.dart';
import '../../../core/security/keystore_channel.dart';
import '../../../core/security/passphrase_cache.dart';
import '../../../core/ffi/generated/models.dart';
import '../../../core/ffi/ffi_bridge.dart';
import '../../../core/i18n/arb_text_localizer.dart';
import '../../../core/logging/debug_log_controller.dart';

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
        return 'Light'.tr;
      case AppThemeMode.dark:
        return 'Dark'.tr;
      case AppThemeMode.system:
        return 'System'.tr;
    }
  }
}

enum AppLocalePreference {
  english,
  indonesian;

  Locale get locale {
    switch (this) {
      case AppLocalePreference.english:
        return const Locale('en');
      case AppLocalePreference.indonesian:
        return const Locale('id');
    }
  }

  String get label {
    switch (this) {
      case AppLocalePreference.english:
        return 'English';
      case AppLocalePreference.indonesian:
        return 'Bahasa Indonesia';
    }
  }
}

enum SwapInterfacePreference {
  simple,
  advanced;

  String get label {
    switch (this) {
      case SwapInterfacePreference.simple:
        return 'Simple swap'.tr;
      case SwapInterfacePreference.advanced:
        return 'Advanced trading'.tr;
    }
  }

  String get description {
    switch (this) {
      case SwapInterfacePreference.simple:
        return 'Guided swap with simple steps'.tr;
      case SwapInterfacePreference.advanced:
        return 'Order book, limits, and slippage controls'.tr;
    }
  }
}

enum CurrencyPreference {
  usd,
  eur,
  gbp,
  btc,
  cad,
  aud,
  jpy,
  chf,
  cny,
  inr,
  brl,
  krw,
  rub,
  uah,
  mxn,
  ars,
  aed,
  bhd,
  kwd,
  sar,
  tryCurrency,
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
      case CurrencyPreference.cad:
        return 'CAD';
      case CurrencyPreference.aud:
        return 'AUD';
      case CurrencyPreference.jpy:
        return 'JPY';
      case CurrencyPreference.chf:
        return 'CHF';
      case CurrencyPreference.cny:
        return 'CNY';
      case CurrencyPreference.inr:
        return 'INR';
      case CurrencyPreference.brl:
        return 'BRL';
      case CurrencyPreference.krw:
        return 'KRW';
      case CurrencyPreference.rub:
        return 'RUB';
      case CurrencyPreference.uah:
        return 'UAH';
      case CurrencyPreference.mxn:
        return 'MXN';
      case CurrencyPreference.ars:
        return 'ARS';
      case CurrencyPreference.aed:
        return 'AED';
      case CurrencyPreference.bhd:
        return 'BHD';
      case CurrencyPreference.kwd:
        return 'KWD';
      case CurrencyPreference.sar:
        return 'SAR';
      case CurrencyPreference.tryCurrency:
        return 'TRY';
    }
  }

  String get label {
    switch (this) {
      case CurrencyPreference.usd:
        return 'US Dollar'.tr;
      case CurrencyPreference.eur:
        return 'Euro'.tr;
      case CurrencyPreference.gbp:
        return 'British Pound'.tr;
      case CurrencyPreference.btc:
        return 'Bitcoin';
      case CurrencyPreference.cad:
        return 'Canadian Dollar'.tr;
      case CurrencyPreference.aud:
        return 'Australian Dollar'.tr;
      case CurrencyPreference.jpy:
        return 'Japanese Yen'.tr;
      case CurrencyPreference.chf:
        return 'Swiss Franc'.tr;
      case CurrencyPreference.cny:
        return 'Chinese Yuan'.tr;
      case CurrencyPreference.inr:
        return 'Indian Rupee'.tr;
      case CurrencyPreference.brl:
        return 'Brazilian Real'.tr;
      case CurrencyPreference.krw:
        return 'South Korean Won'.tr;
      case CurrencyPreference.rub:
        return 'Russian Ruble'.tr;
      case CurrencyPreference.uah:
        return 'Ukrainian Hryvnia'.tr;
      case CurrencyPreference.mxn:
        return 'Mexican Peso'.tr;
      case CurrencyPreference.ars:
        return 'Argentine Peso'.tr;
      case CurrencyPreference.aed:
        return 'UAE Dirham'.tr;
      case CurrencyPreference.bhd:
        return 'Bahraini Dinar'.tr;
      case CurrencyPreference.kwd:
        return 'Kuwaiti Dinar'.tr;
      case CurrencyPreference.sar:
        return 'Saudi Riyal'.tr;
      case CurrencyPreference.tryCurrency:
        return 'Turkish Lira'.tr;
    }
  }

  String get symbol {
    switch (this) {
      case CurrencyPreference.usd:
        return r'$';
      case CurrencyPreference.eur:
        return 'EUR ';
      case CurrencyPreference.gbp:
        return 'GBP ';
      case CurrencyPreference.btc:
        return 'BTC ';
      case CurrencyPreference.cad:
        return 'CAD ';
      case CurrencyPreference.aud:
        return 'AUD ';
      case CurrencyPreference.jpy:
        return 'JPY ';
      case CurrencyPreference.chf:
        return 'CHF ';
      case CurrencyPreference.cny:
        return 'CNY ';
      case CurrencyPreference.inr:
        return 'INR ';
      case CurrencyPreference.brl:
        return 'BRL ';
      case CurrencyPreference.krw:
        return 'KRW ';
      case CurrencyPreference.rub:
        return 'RUB ';
      case CurrencyPreference.uah:
        return 'UAH ';
      case CurrencyPreference.mxn:
        return 'MXN ';
      case CurrencyPreference.ars:
        return 'ARS ';
      case CurrencyPreference.aed:
        return 'AED ';
      case CurrencyPreference.bhd:
        return 'BHD ';
      case CurrencyPreference.kwd:
        return 'KWD ';
      case CurrencyPreference.sar:
        return 'SAR ';
      case CurrencyPreference.tryCurrency:
        return 'TRY ';
    }
  }

  int get fractionDigits {
    switch (this) {
      case CurrencyPreference.usd:
      case CurrencyPreference.eur:
      case CurrencyPreference.gbp:
        return 2;
      case CurrencyPreference.btc:
        return 8;
      case CurrencyPreference.cad:
      case CurrencyPreference.aud:
      case CurrencyPreference.chf:
      case CurrencyPreference.cny:
      case CurrencyPreference.inr:
      case CurrencyPreference.brl:
      case CurrencyPreference.rub:
      case CurrencyPreference.uah:
      case CurrencyPreference.mxn:
      case CurrencyPreference.ars:
      case CurrencyPreference.aed:
      case CurrencyPreference.sar:
      case CurrencyPreference.tryCurrency:
        return 2;
      case CurrencyPreference.bhd:
      case CurrencyPreference.kwd:
        return 3;
      case CurrencyPreference.jpy:
      case CurrencyPreference.krw:
        return 0;
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
    if (!ref.mounted) {
      return;
    }
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
    return CurrencyPreference.usd;
  }

  Future<void> setCurrency(CurrencyPreference preference) async {
    state = preference;
    await _storage.write(key: _storageKey, value: preference.name);
  }

  Future<void> _load() async {
    final raw = await _storage.read(key: _storageKey);
    if (!ref.mounted) {
      return;
    }
    if (raw == null || raw.isEmpty) {
      state = CurrencyPreference.usd;
      return;
    }
    if (raw == 'arrr') {
      state = CurrencyPreference.usd;
      return;
    }
    state = CurrencyPreference.values.firstWhere(
      (pref) => pref.name == raw,
      orElse: () => CurrencyPreference.usd,
    );
  }
}

class SwapInterfacePreferenceNotifier
    extends Notifier<SwapInterfacePreference> {
  late final FlutterSecureStorage _storage;
  static const String _storageKey = 'ui_swap_interface_pref_v1';

  @override
  SwapInterfacePreference build() {
    _storage = const FlutterSecureStorage();
    _load();
    return SwapInterfacePreference.simple;
  }

  Future<void> setPreference(SwapInterfacePreference preference) async {
    state = preference;
    await _storage.write(key: _storageKey, value: preference.name);
  }

  Future<void> _load() async {
    final raw = await _storage.read(key: _storageKey);
    if (!ref.mounted) return;
    if (raw == null || raw.isEmpty) {
      state = SwapInterfacePreference.simple;
      return;
    }
    state = SwapInterfacePreference.values.firstWhere(
      (pref) => pref.name == raw,
      orElse: () => SwapInterfacePreference.simple,
    );
  }
}

class LocalePreferenceNotifier extends Notifier<AppLocalePreference> {
  late final FlutterSecureStorage _storage;
  static const String _storageKey = 'ui_locale_pref_v1';

  @override
  AppLocalePreference build() {
    _storage = const FlutterSecureStorage();
    _load();
    return AppLocalePreference.english;
  }

  Future<void> setLocale(AppLocalePreference preference) async {
    state = preference;
    await _storage.write(key: _storageKey, value: preference.name);
  }

  Future<void> _load() async {
    final raw = await _storage.read(key: _storageKey);
    if (!ref.mounted) {
      return;
    }
    if (raw == null || raw.isEmpty) {
      state = AppLocalePreference.english;
      return;
    }
    state = AppLocalePreference.values.firstWhere(
      (locale) => locale.name == raw,
      orElse: () => AppLocalePreference.english,
    );
  }
}

class SeedPhraseLanguagePreferenceNotifier extends Notifier<MnemonicLanguage> {
  late final FlutterSecureStorage _storage;
  static const String _storageKey = 'seed_phrase_language_pref_v1';

  @override
  MnemonicLanguage build() {
    _storage = const FlutterSecureStorage();
    _load();
    return MnemonicLanguage.english;
  }

  Future<void> setLanguage(MnemonicLanguage language) async {
    state = language;
    await _storage.write(key: _storageKey, value: language.name);
  }

  Future<void> _load() async {
    final raw = await _storage.read(key: _storageKey);
    if (!ref.mounted) {
      return;
    }
    if (raw == null || raw.isEmpty) {
      state = MnemonicLanguage.english;
      return;
    }
    state = MnemonicLanguage.values.firstWhere(
      (language) => language.name == raw,
      orElse: () => MnemonicLanguage.english,
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
        if (!ref.mounted) {
          return;
        }
        state = parsed;
        return;
      }
    } catch (e) {
      debugPrint(
        'Failed to read biometrics preference from secure storage: $e',
      );
    }

    final fallback = await _readFallbackValue();
    if (!ref.mounted) {
      return;
    }
    if (fallback != null) {
      state = fallback;
    } else {
      state = false;
    }

    if (state) {
      final available = await BiometricAuth.isAvailable();
      if (!ref.mounted) {
        return;
      }
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
        if (!ref.mounted) {
          return;
        }
        if (!hasWrappedPassphrase) {
          debugPrint(
            'Biometrics preference was enabled but no wrapped passphrase cache exists. Resetting to disabled.',
          );
          state = false;
          await _persistBestEffort(false);
        }
      } catch (e) {
        if (!ref.mounted) {
          return;
        }
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

abstract class _SecureBoolPreferenceNotifier extends Notifier<bool> {
  late final FlutterSecureStorage _storage;

  String get storageKey;
  bool get defaultValue;

  @override
  bool build() {
    _storage = const FlutterSecureStorage();
    _load();
    return defaultValue;
  }

  Future<void> setValue({required bool value}) async {
    state = value;
    await _storage.write(key: storageKey, value: value.toString());
  }

  Future<void> _load() async {
    try {
      final raw = await _storage.read(key: storageKey);
      if (!ref.mounted) {
        return;
      }
      if (raw == null || raw.isEmpty) {
        state = defaultValue;
        return;
      }
      state = raw.toLowerCase() == 'true';
    } catch (_) {
      state = defaultValue;
    }
  }
}

class BalancePrimaryFiatNotifier extends _SecureBoolPreferenceNotifier {
  @override
  String get storageKey => 'ui_balance_primary_fiat_v1';

  @override
  bool get defaultValue => false;

  Future<void> setPrimaryFiat({required bool enabled}) =>
      setValue(value: enabled);
}

class ExternalApiMasterNotifier extends _SecureBoolPreferenceNotifier {
  @override
  String get storageKey => 'ui_external_api_master_v1';

  @override
  bool get defaultValue => true;

  Future<void> setEnabled({required bool enabled}) => setValue(value: enabled);
}

class ExternalPriceApiNotifier extends _SecureBoolPreferenceNotifier {
  @override
  String get storageKey => 'ui_external_api_prices_v1';

  @override
  bool get defaultValue => true;

  Future<void> setEnabled({required bool enabled}) => setValue(value: enabled);
}

class ExternalGithubApiNotifier extends _SecureBoolPreferenceNotifier {
  @override
  String get storageKey => 'ui_external_api_github_v1';

  @override
  bool get defaultValue => true;

  Future<void> setEnabled({required bool enabled}) => setValue(value: enabled);
}

class ExternalDesktopUpdateApiNotifier extends _SecureBoolPreferenceNotifier {
  @override
  String get storageKey => 'ui_external_api_desktop_updates_v1';

  @override
  bool get defaultValue => true;

  Future<void> setEnabled({required bool enabled}) => setValue(value: enabled);
}

class ExternalKomodoSwapApiNotifier extends _SecureBoolPreferenceNotifier {
  @override
  String get storageKey => 'ui_external_api_komodo_swaps_v1';

  @override
  bool get defaultValue => true;

  Future<void> setEnabled({required bool enabled}) => setValue(value: enabled);
}

class DebugLoggingPreferenceNotifier extends _SecureBoolPreferenceNotifier {
  @override
  String get storageKey => kDebugLoggingStorageKey;

  @override
  bool get defaultValue => false;

  Future<void> setEnabled({required bool enabled}) async {
    await DebugLogController.setEnabled(enabled: enabled);
    state = enabled;
  }
}

final appThemeModeProvider = NotifierProvider<ThemeModeNotifier, AppThemeMode>(
  ThemeModeNotifier.new,
);

final currencyPreferenceProvider =
    NotifierProvider<CurrencyPreferenceNotifier, CurrencyPreference>(
      CurrencyPreferenceNotifier.new,
    );

final localePreferenceProvider =
    NotifierProvider<LocalePreferenceNotifier, AppLocalePreference>(
      LocalePreferenceNotifier.new,
    );

final swapInterfacePreferenceProvider =
    NotifierProvider<SwapInterfacePreferenceNotifier, SwapInterfacePreference>(
      SwapInterfacePreferenceNotifier.new,
    );

final seedPhraseLanguagePreferenceProvider =
    NotifierProvider<SeedPhraseLanguagePreferenceNotifier, MnemonicLanguage>(
      SeedPhraseLanguagePreferenceNotifier.new,
    );

final localeProvider = Provider<Locale>((ref) {
  return ref.watch(localePreferenceProvider).locale;
});

final balancePrimaryFiatProvider =
    NotifierProvider<BalancePrimaryFiatNotifier, bool>(
      BalancePrimaryFiatNotifier.new,
    );

final biometricsEnabledProvider =
    NotifierProvider<BiometricsPreferenceNotifier, bool>(
      BiometricsPreferenceNotifier.new,
    );

final resolvedBiometricsEnabledProvider = FutureProvider<bool>((ref) async {
  final notifier = ref.read(biometricsEnabledProvider.notifier);
  return notifier.readPersistedValue();
});

final externalApiMasterProvider =
    NotifierProvider<ExternalApiMasterNotifier, bool>(
      ExternalApiMasterNotifier.new,
    );

final externalPriceApiProvider =
    NotifierProvider<ExternalPriceApiNotifier, bool>(
      ExternalPriceApiNotifier.new,
    );

final externalGithubApiProvider =
    NotifierProvider<ExternalGithubApiNotifier, bool>(
      ExternalGithubApiNotifier.new,
    );

final externalDesktopUpdateApiProvider =
    NotifierProvider<ExternalDesktopUpdateApiNotifier, bool>(
      ExternalDesktopUpdateApiNotifier.new,
    );

final externalKomodoSwapApiProvider =
    NotifierProvider<ExternalKomodoSwapApiNotifier, bool>(
      ExternalKomodoSwapApiNotifier.new,
    );

final debugLoggingProvider =
    NotifierProvider<DebugLoggingPreferenceNotifier, bool>(
      DebugLoggingPreferenceNotifier.new,
    );

final allowPriceApisProvider = Provider<bool>((ref) {
  final master = ref.watch(externalApiMasterProvider);
  final prices = ref.watch(externalPriceApiProvider);
  return master && prices;
});

final allowGithubApisProvider = Provider<bool>((ref) {
  final master = ref.watch(externalApiMasterProvider);
  final github = ref.watch(externalGithubApiProvider);
  return master && github;
});

final allowDesktopUpdateApisProvider = Provider<bool>((ref) {
  final githubAllowed = ref.watch(allowGithubApisProvider);
  final desktopUpdates = ref.watch(externalDesktopUpdateApiProvider);
  return githubAllowed && desktopUpdates;
});

final allowKomodoSwapApisProvider = Provider<bool>((ref) {
  final master = ref.watch(externalApiMasterProvider);
  final komodoSwaps = ref.watch(externalKomodoSwapApiProvider);
  return master && komodoSwaps;
});

final biometricAvailabilityProvider = FutureProvider<bool>((ref) async {
  return BiometricAuth.isAvailable();
});
