import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Loads runtime ARB catalogs keyed by their English source strings.
class ArbTextLocalizer extends ChangeNotifier {
  ArbTextLocalizer._();

  static final ArbTextLocalizer instance = ArbTextLocalizer._();
  static final RegExp _placeholderPattern = RegExp(
    r'\{([A-Za-z][A-Za-z0-9_]*)\}',
  );

  final Map<String, Map<String, String>> _cache =
      <String, Map<String, String>>{};
  Map<String, String> _fallbackEn = <String, String>{};
  Map<String, String> _active = <String, String>{};
  String _activeLocale = 'en';

  String get activeLocale => _activeLocale;

  Future<void> bootstrap() async {
    if (_fallbackEn.isEmpty) {
      _fallbackEn = await _loadLocaleMap('en');
      _active = _fallbackEn;
      _activeLocale = 'en';
    }
  }

  Future<void> setLocale(String languageCode, {String? countryCode}) async {
    await bootstrap();

    final normalizedLang = languageCode.trim().toLowerCase();
    final normalizedCountry = (countryCode ?? '').trim().toUpperCase();

    if (normalizedCountry.isNotEmpty) {
      final full = '${normalizedLang}_$normalizedCountry';
      final fullMap = await _loadLocaleMap(full);
      if (fullMap.isNotEmpty) {
        _activeLocale = full;
        _active = fullMap;
        notifyListeners();
        return;
      }
    }

    final langMap = await _loadLocaleMap(normalizedLang);
    if (langMap.isEmpty) {
      _activeLocale = 'en';
      _active = _fallbackEn;
    } else {
      _activeLocale = normalizedLang;
      _active = langMap;
    }
    notifyListeners();
  }

  String translate(String source) {
    if (source.isEmpty) {
      return source;
    }
    final activeValue = _active[source];
    if (activeValue != null && activeValue.isNotEmpty) {
      return activeValue;
    }
    final fallbackValue = _fallbackEn[source];
    if (fallbackValue != null && fallbackValue.isNotEmpty) {
      return fallbackValue;
    }
    return source;
  }

  String translateWithArgs(String source, Map<String, Object?> arguments) {
    return translate(source).replaceAllMapped(_placeholderPattern, (match) {
      final name = match.group(1)!;
      return arguments.containsKey(name)
          ? '${arguments[name]}'
          : match.group(0)!;
    });
  }

  Future<Map<String, String>> _loadLocaleMap(String locale) async {
    final cached = _cache[locale];
    if (cached != null) {
      return cached;
    }

    final path = 'assets/i18n/app_$locale.arb';
    try {
      final raw = await rootBundle.loadString(path);
      final decoded = jsonDecode(raw);
      if (decoded is! Map<String, dynamic>) {
        _cache[locale] = <String, String>{};
        return _cache[locale]!;
      }

      final map = <String, String>{};
      for (final entry in decoded.entries) {
        if (entry.key.startsWith('@')) {
          continue;
        }
        final value = entry.value;
        if (value is String) {
          map[entry.key] = value;
        }
      }
      _cache[locale] = map;
      return map;
    } catch (_) {
      _cache[locale] = <String, String>{};
      return _cache[locale]!;
    }
  }
}

extension ArbTrStringExtension on String {
  String get tr => ArbTextLocalizer.instance.translate(this);

  String trArgs(Map<String, Object?> arguments) =>
      ArbTextLocalizer.instance.translateWithArgs(this, arguments);
}
