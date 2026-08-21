import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:pirate_wallet/core/ffi/generated/models.dart';
import 'package:pirate_wallet/features/onboarding/screens/seed_import_screen.dart';
import 'package:pirate_wallet/features/settings/providers/preferences_providers.dart';

const _mnemonic =
    'abandon abandon abandon abandon abandon abandon abandon abandon '
    'abandon abandon abandon about';

void main() {
  testWidgets(
    'continues after a valid phrase when language persistence is unavailable',
    (tester) async {
      await _pumpSeedImport(
        tester,
        languageNotifier: _FailingLanguagePreferenceNotifier.new,
        services: const _SeedImportTestServices(),
      );

      await _pasteAndContinue(tester);

      expect(find.byKey(const Key('passphrase-destination')), findsOneWidget);
      expect(
        find.text('Could not validate the phrase. Try again.'),
        findsNothing,
      );
    },
  );

  testWidgets(
    'does not label a post-validation setup failure as an invalid seed',
    (tester) async {
      await _pumpSeedImport(
        tester,
        languageNotifier: _MemoryLanguagePreferenceNotifier.new,
        services: const _SeedImportTestServices(failPassphraseLookup: true),
      );

      await _pasteAndContinue(tester);

      expect(
        find.text('Could not continue wallet setup. Try again.'),
        findsOneWidget,
      );
      expect(
        find.text('Could not validate the phrase. Try again.'),
        findsNothing,
      );
    },
  );
}

Future<void> _pumpSeedImport(
  WidgetTester tester, {
  required SeedPhraseLanguagePreferenceNotifier Function() languageNotifier,
  required SeedImportServices services,
}) async {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = const Size(1180, 900);
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);

  tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
    SystemChannels.platform,
    (call) async {
      if (call.method == 'Clipboard.getData') {
        return <String, dynamic>{'text': _mnemonic};
      }
      return null;
    },
  );
  addTearDown(
    () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      null,
    ),
  );

  final router = GoRouter(
    routes: [
      GoRoute(
        path: '/',
        builder: (_, _) => SeedImportScreen(services: services),
      ),
      GoRoute(
        path: '/onboarding/passphrase',
        builder: (_, _) => const Scaffold(
          body: Text('Passphrase', key: Key('passphrase-destination')),
        ),
      ),
    ],
  );
  addTearDown(router.dispose);

  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        seedPhraseLanguagePreferenceProvider.overrideWith(languageNotifier),
      ],
      child: MaterialApp.router(routerConfig: router),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> _pasteAndContinue(WidgetTester tester) async {
  await tester.tap(find.text('Paste seed phrase'));
  await tester.pump();
  final continueButton = find.text('Continue');
  await tester.dragUntilVisible(
    continueButton,
    find.byType(CustomScrollView),
    const Offset(0, -500),
  );
  await tester.pumpAndSettle();
  await tester.tap(continueButton);
  await tester.pumpAndSettle();
}

class _SeedImportTestServices extends SeedImportServices {
  final bool failPassphraseLookup;

  const _SeedImportTestServices({this.failPassphraseLookup = false});

  @override
  Future<List<String>> loadWordlist(MnemonicLanguage language) async {
    return const ['abandon', 'about'];
  }

  @override
  Future<MnemonicInspection> inspectMnemonic(String mnemonic) async {
    return const MnemonicInspection(
      isValid: true,
      detectedLanguage: MnemonicLanguage.english,
      ambiguousLanguages: [],
      wordCount: 12,
    );
  }

  @override
  Future<bool> validateMnemonic(
    String mnemonic, {
    required MnemonicLanguage mnemonicLanguage,
  }) async {
    return true;
  }

  @override
  Future<bool> hasAppPassphrase() async {
    if (failPassphraseLookup) {
      throw StateError('keychain unavailable');
    }
    return false;
  }
}

class _MemoryLanguagePreferenceNotifier
    extends SeedPhraseLanguagePreferenceNotifier {
  @override
  MnemonicLanguage build() => MnemonicLanguage.english;

  @override
  Future<void> setLanguage(MnemonicLanguage language) async {
    state = language;
  }
}

class _FailingLanguagePreferenceNotifier
    extends _MemoryLanguagePreferenceNotifier {
  @override
  Future<void> setLanguage(MnemonicLanguage language) async {
    state = language;
    throw StateError('keychain unavailable');
  }
}
