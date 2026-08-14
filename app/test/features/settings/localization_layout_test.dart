import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/design/deep_space_theme.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';
import 'package:pirate_wallet/features/settings/screens/language_screen.dart';
import 'package:pirate_wallet/ui/atoms/p_button.dart';
import 'package:pirate_wallet/ui/molecules/p_card.dart';
import 'package:pirate_wallet/ui/molecules/p_list_tile.dart';

const _compactActionKeys = <String>[
  'Import view only wallet',
  'Place limit order',
  'Save birthday height',
];

const _settingsRows = <(String, String)>[
  (
    'Debug logging',
    'Debug logs can contain troubleshooting metadata. Exported logs are redacted, but only enable this while reproducing an issue.',
  ),
  (
    'TLS CERTIFICATE PIN (OPTIONAL)',
    "TLS pinning adds extra security by verifying the server's certificate. Use Fetch SPKI to grab the pin from the current endpoint.",
  ),
  (
    'View Only Wallets',
    'View only wallets cannot spend. They only show incoming activity.',
  ),
  ('Order Book', 'Order book, limits, and slippage controls'),
];

class _FixedLocaleNotifier extends LocalePreferenceNotifier {
  _FixedLocaleNotifier(this.preference);

  final AppLocalePreference preference;

  @override
  AppLocalePreference build() => preference;

  @override
  Future<void> setLocale(AppLocalePreference preference) async {
    state = preference;
  }
}

Future<void> _loadFont(String family, String asset) async {
  final bytes = await File(asset).readAsBytes();
  final loader = FontLoader(family)
    ..addFont(Future<ByteData>.value(ByteData.sublistView(bytes)));
  await loader.load();
}

Map<String, String> _readCatalog(AppLocalePreference preference) {
  final locale = preference.locale.toLanguageTag().replaceAll('-', '_');
  final decoded =
      jsonDecode(File('assets/i18n/app_$locale.arb').readAsStringSync())
          as Map<String, dynamic>;
  return <String, String>{
    for (final entry in decoded.entries)
      if (!entry.key.startsWith('@') && entry.value is String)
        entry.key: entry.value! as String,
  };
}

Widget _localizedApp({
  required AppLocalePreference preference,
  required Widget home,
}) {
  return ProviderScope(
    overrides: [
      localePreferenceProvider.overrideWith(
        () => _FixedLocaleNotifier(preference),
      ),
    ],
    child: MaterialApp(
      locale: preference.locale,
      supportedLocales: AppLocalePreference.values.map(
        (option) => option.locale,
      ),
      localizationsDelegates: const [
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ],
      theme: PTheme.dark(),
      home: home,
    ),
  );
}

void _expectNoLayoutException(
  WidgetTester tester,
  AppLocalePreference preference,
  Size viewport,
  String surface,
) {
  expect(
    tester.takeException(),
    isNull,
    reason:
        '${preference.locale.toLanguageTag()} overflowed on $surface at '
        '${viewport.width.toInt()}x${viewport.height.toInt()}',
  );
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async {
    for (final (family, asset) in <(String, String)>[
      ('Sora', 'assets/fonts/Sora/Sora.ttf'),
      ('JetBrainsMono', 'assets/fonts/JetBrainsMono/JetBrainsMono.ttf'),
      ('NotoSans', 'assets/fonts/NotoSans/NotoSans.ttf'),
      (
        'NotoSansSymbols2',
        'assets/fonts/NotoSansSymbols2/NotoSansSymbols2.ttf',
      ),
      ('NotoSansArabic', 'assets/fonts/NotoSansArabic/NotoSansArabic.ttf'),
      (
        'NotoSansDevanagari',
        'assets/fonts/NotoSansDevanagari/NotoSansDevanagari.ttf',
      ),
      ('NotoSansSC', 'assets/fonts/NotoSansSC/NotoSansSC.ttf'),
      ('NotoSansJP', 'assets/fonts/NotoSansJP/NotoSansJP.ttf'),
      ('NotoSansKR', 'assets/fonts/NotoSansKR/NotoSansKR.ttf'),
    ]) {
      await _loadFont(family, asset);
    }
  });

  for (final viewport in <Size>[
    const Size(320, 700),
    const Size(844, 390),
    const Size(1024, 768),
  ]) {
    testWidgets(
      'language picker fits every locale at ${viewport.width.toInt()}px',
      (tester) async {
        tester.view.devicePixelRatio = 1;
        tester.view.physicalSize = viewport;
        addTearDown(tester.view.reset);

        for (final preference in AppLocalePreference.values) {
          await tester.pumpWidget(
            _localizedApp(preference: preference, home: const LanguageScreen()),
          );
          await tester.pump(const Duration(milliseconds: 250));
          _expectNoLayoutException(
            tester,
            preference,
            viewport,
            'language picker',
          );
        }
      },
    );

    testWidgets('compact localized controls fit every locale at '
        '${viewport.width.toInt()}px', (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = viewport;
      addTearDown(tester.view.reset);

      for (final preference in AppLocalePreference.values) {
        final catalog = _readCatalog(preference);
        await tester.pumpWidget(
          _localizedApp(
            preference: preference,
            home: _LocalizationLayoutProbe(catalog: catalog),
          ),
        );
        await tester.pump(const Duration(milliseconds: 250));
        _expectNoLayoutException(
          tester,
          preference,
          viewport,
          'compact controls',
        );
      }
    });
  }
}

class _LocalizationLayoutProbe extends StatelessWidget {
  const _LocalizationLayoutProbe({required this.catalog});

  final Map<String, String> catalog;

  String _translated(String source) => catalog[source] ?? source;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: AppColors.backgroundBase,
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(AppSpacing.md),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  Expanded(
                    child: PButton(
                      onPressed: () {},
                      text: _translated('Apply & Restart Tor'),
                    ),
                  ),
                  const SizedBox(width: AppSpacing.sm),
                  Expanded(
                    child: PButton(
                      onPressed: () {},
                      text: _translated('Use Snowflake'),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: AppSpacing.sm),
              for (final key in _compactActionKeys) ...[
                PButton(
                  onPressed: () {},
                  text: _translated(key),
                  fullWidth: true,
                ),
                const SizedBox(height: AppSpacing.sm),
              ],
              const SizedBox(height: AppSpacing.sm),
              PCard(
                child: Column(
                  children: [
                    for (final (title, subtitle) in _settingsRows)
                      PListTile(
                        leading: const Icon(Icons.settings_outlined),
                        title: _translated(title),
                        subtitle: _translated(subtitle),
                        trailing: const Icon(Icons.chevron_right),
                      ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
