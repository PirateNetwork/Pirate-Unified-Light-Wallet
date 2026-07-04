import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/core/i18n/arb_text_localizer.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/core/services/address_rotation_service.dart';
import 'package:pirate_wallet/core/swaps/swap_providers.dart';
import 'package:pirate_wallet/design/theme.dart';
import 'package:pirate_wallet/features/app_shell/app_shell.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';
import 'package:pirate_wallet/features/settings/screens/language_screen.dart';
import 'package:pirate_wallet/features/settings/settings_screen.dart';

class _TestLocalePreferenceNotifier extends LocalePreferenceNotifier {
  @override
  AppLocalePreference build() => AppLocalePreference.english;

  @override
  Future<void> setLocale(AppLocalePreference preference) async {
    final locale = preference.locale;
    await ArbTextLocalizer.instance.setLocale(
      locale.languageCode,
      countryCode: locale.countryCode,
    );
    state = preference;
  }
}

class _DisabledBiometricsNotifier extends BiometricsPreferenceNotifier {
  @override
  bool build() => false;
}

ProviderContainer _createContainer({bool includeShellOverrides = false}) {
  return ProviderContainer(
    overrides: [
      localePreferenceProvider.overrideWith(_TestLocalePreferenceNotifier.new),
      biometricsEnabledProvider.overrideWith(_DisabledBiometricsNotifier.new),
      resolvedBiometricsEnabledProvider.overrideWith((ref) async => false),
      biometricAvailabilityProvider.overrideWith((ref) async => false),
      if (includeShellOverrides) ...[
        transactionWatcherProvider.overrideWith((ref) {}),
        syncCompletionWatcherProvider.overrideWith((ref) {}),
        autoRotationWatcherProvider.overrideWith((ref) {}),
        syncCompletionRotationWatcherProvider.overrideWith((ref) {}),
        walletInitRotationWatcherProvider.overrideWith((ref) {}),
        kdfSwapWarmupProvider.overrideWith((ref) {}),
      ],
    ],
  );
}

Widget _testApp({required ProviderContainer container, required Widget home}) {
  return UncontrolledProviderScope(
    container: container,
    child: MaterialApp(
      theme: PTheme.dark(),
      locale: const Locale('en'),
      supportedLocales: AppLocalePreference.values.map(
        (preference) => preference.locale,
      ),
      localizationsDelegates: const [
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ],
      home: home,
    ),
  );
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() async {
    await ArbTextLocalizer.instance.setLocale('en');
  });

  tearDown(() async {
    await ArbTextLocalizer.instance.setLocale('en');
  });

  testWidgets('persistent navigation labels update with the selected locale', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1200, 800);
    addTearDown(tester.view.reset);

    final container = _createContainer(includeShellOverrides: true);
    addTearDown(container.dispose);

    await tester.pumpWidget(
      _testApp(
        container: container,
        home: const AppShell(location: '/home', child: SizedBox.shrink()),
      ),
    );

    expect(find.text('Home'), findsOneWidget);
    expect(find.text('Pay'), findsOneWidget);
    expect(find.text('Activity'), findsOneWidget);
    expect(find.text('Settings'), findsOneWidget);

    await tester.runAsync(
      () => container
          .read(localePreferenceProvider.notifier)
          .setLocale(AppLocalePreference.indonesian),
    );
    await tester.pump();

    expect(find.text('Beranda'), findsOneWidget);
    expect(find.text('Bayar'), findsOneWidget);
    expect(find.text('Aktivitas'), findsOneWidget);
    expect(find.text('Pengaturan'), findsOneWidget);
    expect(find.text('Home'), findsNothing);
    expect(find.text('Settings'), findsNothing);
  });

  testWidgets('settings refreshes while covered before returning to it', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1200, 700);
    addTearDown(tester.view.reset);

    final container = _createContainer();
    addTearDown(container.dispose);
    final navigatorKey = GlobalKey<NavigatorState>();

    await tester.pumpWidget(
      _testApp(
        container: container,
        home: Navigator(
          key: navigatorKey,
          onGenerateRoute: (_) => MaterialPageRoute<void>(
            builder: (_) => const SettingsScreen(useScaffold: false),
          ),
        ),
      ),
    );
    await tester.pump();
    expect(find.text('Security'), findsOneWidget);

    final languageRoute = navigatorKey.currentState!.push(
      MaterialPageRoute<void>(builder: (_) => const LanguageScreen()),
    );
    await tester.pumpAndSettle();

    await tester.runAsync(
      () => container
          .read(localePreferenceProvider.notifier)
          .setLocale(AppLocalePreference.indonesian),
    );
    await tester.pump();

    navigatorKey.currentState!.pop();
    await tester.pumpAndSettle();
    await languageRoute;

    expect(find.text('Keamanan'), findsOneWidget);
    expect(find.text('Security'), findsNothing);
  });
}
