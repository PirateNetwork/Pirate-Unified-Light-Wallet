import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:pirate_wallet/features/onboarding/screens/welcome_screen.dart';
import 'package:pirate_wallet/ui/atoms/p_button.dart';

void main() {
  testWidgets('welcome content fits a landscape phone', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(844, 390);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);

    final router = GoRouter(
      initialLocation: '/onboarding/welcome',
      routes: [
        GoRoute(
          path: '/onboarding/welcome',
          builder: (context, state) => const WelcomeScreen(),
        ),
      ],
    );
    addTearDown(router.dispose);

    await tester.pumpWidget(
      ProviderScope(child: MaterialApp.router(routerConfig: router)),
    );
    await tester.pumpAndSettle();

    expect(find.text('Stashi Wallet'), findsWidgets);
    expect(find.byKey(const Key('welcome_logo')), findsOneWidget);
    expect(find.text('Get started'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('Get started ignores repeated activation', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1180, 760);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);

    var destinationBuildCount = 0;
    final router = GoRouter(
      initialLocation: '/onboarding/welcome',
      routes: [
        GoRoute(
          path: '/onboarding/welcome',
          builder: (context, state) => const WelcomeScreen(),
        ),
        GoRoute(
          path: '/onboarding/create-or-import',
          builder: (context, state) {
            destinationBuildCount++;
            return const Scaffold(body: Text('Create or import'));
          },
        ),
      ],
    );
    addTearDown(router.dispose);

    await tester.pumpWidget(
      ProviderScope(
        child: MaterialApp.router(
          routerConfig: router,
          theme: ThemeData(splashFactory: NoSplash.splashFactory),
        ),
      ),
    );
    await tester.pumpAndSettle();

    final button = tester.widget<PButton>(find.byType(PButton));
    button.onPressed!();
    button.onPressed!();
    button.onPressed!();
    await tester.pumpAndSettle();

    expect(find.text('Create or import'), findsOneWidget);
    expect(destinationBuildCount, 1);
    expect(router.canPop(), isFalse);
  });
}
