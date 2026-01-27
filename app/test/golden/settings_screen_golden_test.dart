/// Golden tests for Settings Screen
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/features/settings/settings_screen.dart';

import '../golden_test_utils.dart';

void main() {
  setUpAll(() async {
    await configureGoldenTests();
  });

  testWidgets('settings screen - multiple devices', (tester) async {
    if (shouldSkipGoldenTests()) return;
    await testMultipleDevices(
      tester,
      const ProviderScope(
        child: MaterialApp(
          home: SettingsScreen(),
        ),
      ),
      'settings_screen',
    );
  });

  testWidgets('settings screen - mobile', (tester) async {
    if (shouldSkipGoldenTests()) return;
    await tester.pumpWidget(
      const ProviderScope(
        child: MaterialApp(
          home: SettingsScreen(),
        ),
      ),
    );

    await expectGoldenMatches(
      tester,
      'settings_screen_mobile.png',
      size: DeviceSizes.iphoneX,
    );
  });

  testWidgets('settings screen - desktop', (tester) async {
    if (shouldSkipGoldenTests()) return;
    await tester.pumpWidget(
      const ProviderScope(
        child: MaterialApp(
          home: SettingsScreen(),
        ),
      ),
    );

    await expectGoldenMatches(
      tester,
      'settings_screen_desktop.png',
      size: DeviceSizes.desktop1080p,
    );
  });
}

