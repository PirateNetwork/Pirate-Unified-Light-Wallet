/// Golden tests for Home Screen
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/features/home/home_screen.dart';

import '../golden_test_utils.dart';

void main() {
  setUpAll(() async {
    await configureGoldenTests();
  });

  testWidgets('home screen - mobile portrait', (tester) async {
    await tester.pumpWidget(
      const ProviderScope(
        child: MaterialApp(
          home: HomeScreen(),
        ),
      ),
    );

    await testMultipleDevices(
      tester,
      const ProviderScope(
        child: MaterialApp(
          home: HomeScreen(),
        ),
      ),
      'home_screen',
    );
  });

  testWidgets('home screen - tablet landscape', (tester) async {
    await tester.pumpWidget(
      const ProviderScope(
        child: MaterialApp(
          home: HomeScreen(),
        ),
      ),
    );

    await expectGoldenMatches(
      tester,
      'home_screen_tablet.png',
      size: DeviceSizes.ipadPro11,
    );
  });

  testWidgets('home screen - desktop', (tester) async {
    await tester.pumpWidget(
      const ProviderScope(
        child: MaterialApp(
          home: HomeScreen(),
        ),
      ),
    );

    await expectGoldenMatches(
      tester,
      'home_screen_desktop.png',
      size: DeviceSizes.desktop1080p,
    );
  });

  testWidgets('home screen - dark mode', (tester) async {
    await tester.pumpWidget(
      ProviderScope(
        child: MaterialApp(
          theme: ThemeData.dark(),
          home: const HomeScreen(),
        ),
      ),
    );

    await expectGoldenMatches(
      tester,
      'home_screen_dark.png',
      size: DeviceSizes.iphoneX,
    );
  });
}

