import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../lib/design/theme.dart';
import '../../lib/features/home/home_screen.dart';
import '../../lib/features/receive/receive_screen.dart';
import '../../lib/features/send/send_screen.dart';
import '../../lib/features/settings/settings_screen.dart';
import '../golden_test_utils.dart';

const bool _skipCoreGoldens = true;

void main() {
  setUpAll(() async {
    await configureGoldenTests();
  });

  group('Core flow goldens', () {
    testWidgets('Home screen mobile & desktop', (tester) async {
      await testMultipleDevices(
        tester,
        ProviderScope(
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const HomeScreen(),
          ),
        ),
        'home_screen',
        devices: const [
          (name: 'mobile', size: Size(390, 844)),
          (name: 'desktop', size: Size(1280, 800)),
        ],
      );
    }, skip: _skipCoreGoldens);

    testWidgets('Send screen mobile & desktop', (tester) async {
      await testMultipleDevices(
        tester,
        ProviderScope(
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const SendScreen(),
          ),
        ),
        'send_screen',
        devices: const [
          (name: 'mobile', size: Size(390, 844)),
          (name: 'desktop', size: Size(1280, 800)),
        ],
      );
    }, skip: _skipCoreGoldens);

    testWidgets('Receive screen mobile & desktop', (tester) async {
      await testMultipleDevices(
        tester,
        ProviderScope(
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const ReceiveScreen(),
          ),
        ),
        'receive_screen',
        devices: const [
          (name: 'mobile', size: Size(390, 844)),
          (name: 'desktop', size: Size(1280, 800)),
        ],
      );
    }, skip: _skipCoreGoldens);

    testWidgets('Settings screen mobile & desktop', (tester) async {
      await testMultipleDevices(
        tester,
        ProviderScope(
          child: MaterialApp(
            theme: PTheme.dark(),
            home: const SettingsScreen(),
          ),
        ),
        'settings_screen',
        devices: const [
          (name: 'mobile', size: Size(390, 844)),
          (name: 'desktop', size: Size(1280, 800)),
        ],
      );
    }, skip: _skipCoreGoldens);
  });
}

