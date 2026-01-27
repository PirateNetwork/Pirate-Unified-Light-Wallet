/// Golden tests for security screens
/// 
/// Tests export seed, watch-only, and panic PIN screens
library;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:golden_toolkit/golden_toolkit.dart';

import 'package:pirate_wallet/features/security/export_seed_screen.dart';
import 'package:pirate_wallet/features/security/watch_only_screen.dart';
import 'package:pirate_wallet/features/security/panic_pin_screen.dart';
import 'package:pirate_wallet/features/security/panic_pin_provider.dart';

import '../golden_test_utils.dart';

class MockExportSeedStateNotifier extends ExportSeedStateNotifier {
  final ExportSeedState initialState;
  MockExportSeedStateNotifier(this.initialState);
  @override
  ExportSeedState build() => initialState;
}

class MockSeedWordsNotifier extends SeedWordsNotifier {
  final List<String>? initialWords;
  MockSeedWordsNotifier(this.initialWords);
  @override
  List<String>? build() => initialWords;
}

void main() {
  group('Security Screens Golden Tests', () {
    testGoldens('ExportSeedScreen - Warning Step', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await tester.pumpWidgetBuilder(
        const ProviderScope(
          child: MaterialApp(
            home: ExportSeedScreen(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await screenMatchesGolden(tester, 'export_seed_warning');
    });

    testGoldens('ExportSeedScreen - Biometric Step', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: [
            exportSeedStateProvider.overrideWith(() => MockExportSeedStateNotifier(ExportSeedState.biometric)),
          ],
          child: const MaterialApp(
            home: ExportSeedScreen(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await screenMatchesGolden(tester, 'export_seed_biometric');
    });

    testGoldens('ExportSeedScreen - Passphrase Step', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: [
            exportSeedStateProvider.overrideWith(() => MockExportSeedStateNotifier(ExportSeedState.passphrase)),
          ],
          child: const MaterialApp(
            home: ExportSeedScreen(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await screenMatchesGolden(tester, 'export_seed_passphrase');
    });

    testGoldens('ExportSeedScreen - Display Step', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: [
            exportSeedStateProvider.overrideWith(() => MockExportSeedStateNotifier(ExportSeedState.display)),
            seedWordsProvider.overrideWith(() => MockSeedWordsNotifier([
              'abandon', 'abandon', 'abandon', 'abandon',
              'abandon', 'abandon', 'abandon', 'abandon',
              'abandon', 'abandon', 'abandon', 'abandon',
              'abandon', 'abandon', 'abandon', 'abandon',
              'abandon', 'abandon', 'abandon', 'abandon',
              'abandon', 'abandon', 'abandon', 'art',
            ])),
          ],
          child: const MaterialApp(
            home: ExportSeedScreen(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await screenMatchesGolden(tester, 'export_seed_display');
    });

    testGoldens('WatchOnlyScreen - Export Tab', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await tester.pumpWidgetBuilder(
        const ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await screenMatchesGolden(tester, 'watch_only_export');
    });

    testGoldens('WatchOnlyScreen - Import Tab', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await tester.pumpWidgetBuilder(
        const ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      
      // Tap import tab
      await tester.tap(find.text('Import IVK'));
      await tester.pumpAndSettle();
      
      await screenMatchesGolden(tester, 'watch_only_import');
    });

    testGoldens('WatchOnlyBannerWidget', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await tester.pumpWidgetBuilder(
        const MaterialApp(
          home: Scaffold(
            body: Column(
              children: [
                WatchOnlyBannerWidget(),
                Expanded(child: SizedBox()),
              ],
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await screenMatchesGolden(tester, 'watch_only_banner');
    });

    testGoldens('PanicPinScreen - Not Configured', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: [
            hasPanicPinProvider.overrideWith((ref) async => false),
          ],
          child: const MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await screenMatchesGolden(tester, 'panic_pin_not_configured');
    });

    testGoldens('PanicPinScreen - Configured', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: [
            hasPanicPinProvider.overrideWith((ref) async => true),
          ],
          child: const MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await screenMatchesGolden(tester, 'panic_pin_configured');
    });

    testGoldens('PanicPinScreen - Setup Form', (tester) async {
      if (shouldSkipGoldenTests()) return;
      await tester.pumpWidgetBuilder(
        ProviderScope(
          overrides: [
            hasPanicPinProvider.overrideWith((ref) async => false),
          ],
          child: const MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      
      // Tap "Set Up Panic PIN"
      await tester.tap(find.text('Set Up Panic PIN'));
      await tester.pumpAndSettle();
      
      await screenMatchesGolden(tester, 'panic_pin_setup_form');
    });
  });
}

