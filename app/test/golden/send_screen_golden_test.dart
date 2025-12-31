/// Golden tests for Send Screen
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pirate_wallet/features/send/send_screen.dart';

import '../golden_test_utils.dart';

void main() {
  setUpAll(() async {
    await configureGoldenTests();
  });

  testWidgets('send screen - recipient step - multiple devices', (tester) async {
    await testMultipleDevices(
      tester,
      const ProviderScope(
        child: MaterialApp(
          home: SendScreen(),
        ),
      ),
      'send_screen_recipient',
    );
  });

  testWidgets('send screen - mobile', (tester) async {
    await tester.pumpWidget(
      const ProviderScope(
        child: MaterialApp(
          home: SendScreen(),
        ),
      ),
    );

    await expectGoldenMatches(
      tester,
      'send_screen_mobile.png',
      size: DeviceSizes.iphoneX,
    );
  });

  testWidgets('send screen - desktop', (tester) async {
    await tester.pumpWidget(
      const ProviderScope(
        child: MaterialApp(
          home: SendScreen(),
        ),
      ),
    );

    await expectGoldenMatches(
      tester,
      'send_screen_desktop.png',
      size: DeviceSizes.desktop1080p,
    );
  });
}

