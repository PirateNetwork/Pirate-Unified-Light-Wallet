import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/tokens/typography.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';

void main() {
  test('application locales expose the expected native labels', () {
    expect(
      AppLocalePreference.values.map(
        (preference) => (preference.locale.toLanguageTag(), preference.label),
      ),
      <(String, String)>[
        ('en', 'English'),
        ('ar', 'العربية'),
        ('zh-CN', '简体中文'),
        ('nl', 'Nederlands'),
        ('fr', 'Français'),
        ('de', 'Deutsch'),
        ('hi', 'हिन्दी'),
        ('id', 'Bahasa Indonesia'),
        ('it', 'Italiano'),
        ('ja', '日本語'),
        ('ko', '한국어'),
        ('pt', 'Português'),
        ('ru', 'Русский'),
        ('es', 'Español'),
        ('tr', 'Türkçe'),
        ('ur', 'اردو'),
      ],
    );
  });

  test('every selectable locale has a complete runtime catalog', () {
    final english =
        jsonDecode(File('assets/i18n/app_en.arb').readAsStringSync())
            as Map<String, dynamic>;
    final englishKeys = english.keys
        .where((key) => !key.startsWith('@'))
        .toSet();

    for (final preference in AppLocalePreference.values.skip(1)) {
      final localeName = preference.locale.toLanguageTag().replaceAll('-', '_');
      final catalogFile = File('assets/i18n/app_$localeName.arb');

      expect(
        catalogFile.existsSync(),
        isTrue,
        reason: '${preference.label} is selectable but has no ARB catalog',
      );
      final catalog =
          jsonDecode(catalogFile.readAsStringSync()) as Map<String, dynamic>;
      expect(catalog['@@locale'], localeName);
      expect(
        catalog.keys.where((key) => !key.startsWith('@')).toSet(),
        englishKeys,
        reason: '$localeName does not match the English runtime catalog',
      );
    }
  });

  test('bundled typography fallbacks cover non-Latin locales', () {
    expect(PTypography.fontFamilyFallback, <String>[
      'NotoSans',
      'NotoSansSymbols2',
      'NotoSansArabic',
      'NotoSansDevanagari',
      'NotoSansSC',
      'NotoSansJP',
      'NotoSansKR',
    ]);
    for (final asset in <String>[
      'assets/fonts/NotoSans/NotoSans.ttf',
      'assets/fonts/NotoSansSymbols2/NotoSansSymbols2.ttf',
      'assets/fonts/NotoSansArabic/NotoSansArabic.ttf',
      'assets/fonts/NotoSansDevanagari/NotoSansDevanagari.ttf',
      'assets/fonts/NotoSansSC/NotoSansSC.ttf',
      'assets/fonts/NotoSansJP/NotoSansJP.ttf',
      'assets/fonts/NotoSansKR/NotoSansKR.ttf',
    ]) {
      expect(
        File(asset).lengthSync(),
        greaterThan(0),
        reason: 'Missing fallback font asset $asset',
      );
    }
    expect(
      PTypography.bodyMedium().fontFamilyFallback,
      PTypography.fontFamilyFallback,
    );
    expect(
      PTypography.codeMedium().fontFamilyFallback,
      PTypography.fontFamilyFallback,
    );
  });

  testWidgets('Arabic and Urdu application locales use right-to-left layout', (
    tester,
  ) async {
    for (final preference in <AppLocalePreference>[
      AppLocalePreference.arabic,
      AppLocalePreference.urdu,
    ]) {
      TextDirection? direction;

      await tester.pumpWidget(
        MaterialApp(
          locale: preference.locale,
          supportedLocales: AppLocalePreference.values.map(
            (supportedPreference) => supportedPreference.locale,
          ),
          localizationsDelegates: const [
            GlobalMaterialLocalizations.delegate,
            GlobalCupertinoLocalizations.delegate,
            GlobalWidgetsLocalizations.delegate,
          ],
          home: Builder(
            builder: (context) {
              direction = Directionality.of(context);
              return const SizedBox.shrink();
            },
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(direction, TextDirection.rtl, reason: preference.label);
    }
  });
}
