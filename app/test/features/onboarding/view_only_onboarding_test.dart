import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:pirate_wallet/core/providers/wallet_providers.dart';
import 'package:pirate_wallet/features/onboarding/onboarding_flow.dart';
import 'package:pirate_wallet/features/onboarding/onboarding_security.dart';
import 'package:pirate_wallet/features/onboarding/screens/create_or_import_screen.dart';
import 'package:pirate_wallet/features/onboarding/screens/ivk_import_screen.dart';

void main() {
  testWidgets('first view-only wallet establishes local encryption first', (
    tester,
  ) async {
    final container = await _pumpFlow(tester, hasAppPassphrase: false);

    await tester.tap(find.text('View only'));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('passphrase-destination')), findsOneWidget);
    expect(find.byKey(const Key('viewing-key-destination')), findsNothing);
    expect(
      container.read(onboardingControllerProvider).currentStep,
      OnboardingStep.setupPassphrase,
    );
  });

  testWidgets('existing encrypted storage is unlocked before import', (
    tester,
  ) async {
    await _pumpFlow(tester, hasAppPassphrase: true);

    await tester.tap(find.text('View only'));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('unlock-destination')), findsOneWidget);
    expect(find.text('/onboarding/import-ivk'), findsOneWidget);
  });

  testWidgets('an unlocked wallet proceeds directly to viewing-key import', (
    tester,
  ) async {
    final container = await _pumpFlow(
      tester,
      hasAppPassphrase: true,
      initiallyUnlocked: true,
    );

    await tester.tap(find.text('View only'));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('viewing-key-destination')), findsOneWidget);
    expect(
      container.read(onboardingControllerProvider).currentStep,
      OnboardingStep.viewingKeyImport,
    );
  });

  testWidgets('a direct first-wallet import cannot reach encrypted storage', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1180, 900);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);

    final router = GoRouter(
      routes: [
        GoRoute(
          path: '/',
          builder: (_, _) => const ViewingKeysImportScreen(
            securityServices: _SecurityServices(configured: false),
          ),
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
        child: MaterialApp.router(
          theme: ThemeData(splashFactory: NoSplash.splashFactory),
          routerConfig: router,
        ),
      ),
    );
    await tester.pumpAndSettle();

    final fields = find.byType(TextField);
    await tester.enterText(fields.at(1), 'zxviews1test');
    await tester.enterText(fields.at(3), '1');
    final importButton = find.text('Import view only wallet');
    await tester.ensureVisible(importButton);
    await tester.pumpAndSettle();
    await tester.tap(importButton);
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('passphrase-destination')), findsOneWidget);
    expect(find.textContaining('AnyhowException'), findsNothing);
  });
}

Future<ProviderContainer> _pumpFlow(
  WidgetTester tester, {
  required bool hasAppPassphrase,
  bool initiallyUnlocked = false,
}) async {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = const Size(1180, 900);
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);

  final router = GoRouter(
    routes: [
      GoRoute(
        path: '/',
        builder: (_, _) => CreateOrImportScreen(
          securityServices: _SecurityServices(configured: hasAppPassphrase),
        ),
      ),
      GoRoute(
        path: '/onboarding/passphrase',
        builder: (_, _) => const Scaffold(
          body: Text('Passphrase', key: Key('passphrase-destination')),
        ),
      ),
      GoRoute(
        path: '/onboarding/import-ivk',
        builder: (_, _) => const Scaffold(
          body: Text('Viewing key', key: Key('viewing-key-destination')),
        ),
      ),
      GoRoute(
        path: '/unlock',
        builder: (_, state) => Scaffold(
          body: Column(
            children: [
              const Text('Unlock', key: Key('unlock-destination')),
              Text(state.uri.queryParameters['redirect'] ?? ''),
            ],
          ),
        ),
      ),
    ],
  );
  addTearDown(router.dispose);

  final container = ProviderContainer(
    overrides: initiallyUnlocked
        ? [appUnlockedProvider.overrideWith(_UnlockedNotifier.new)]
        : [],
  );
  addTearDown(container.dispose);

  await tester.pumpWidget(
    UncontrolledProviderScope(
      container: container,
      child: MaterialApp.router(
        theme: ThemeData(splashFactory: NoSplash.splashFactory),
        routerConfig: router,
      ),
    ),
  );
  await tester.pumpAndSettle();
  return container;
}

class _SecurityServices extends OnboardingSecurityServices {
  const _SecurityServices({required this.configured});

  final bool configured;

  @override
  Future<bool> hasAppPassphrase() async => configured;
}

class _UnlockedNotifier extends AppUnlockedNotifier {
  @override
  bool build() => true;
}
