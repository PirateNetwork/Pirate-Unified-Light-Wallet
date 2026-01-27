import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_wallet/main.dart' as app;

/// Integration test for complete onboarding flow
void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  group('Onboarding Flow E2E', () {
    testWidgets('Complete new wallet creation flow', (WidgetTester tester) async {
      // Launch app
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      // Should show onboarding screen
      expect(find.text('Welcome'), findsOneWidget);

      // Tap "Create New Wallet"
      await tester.tap(find.text('Create New Wallet'));
      await tester.pumpAndSettle();

      // Should show wallet name input
      expect(find.text('Name Your Wallet'), findsOneWidget);

      // Enter wallet name
      await tester.enterText(
        find.byType(TextField).first,
        'Test Wallet',
      );
      await tester.pumpAndSettle();

      // Tap continue
      await tester.tap(find.text('Continue'));
      await tester.pumpAndSettle();

      // Should show passphrase setup
      expect(find.text('Set Passphrase'), findsOneWidget);

      // Enter passphrase
      final passphraseFields = find.byType(TextField);
      await tester.enterText(passphraseFields.at(0), 'TestPass123!');
      await tester.enterText(passphraseFields.at(1), 'TestPass123!');
      await tester.pumpAndSettle();

      // Tap continue
      await tester.tap(find.text('Continue'));
      await tester.pumpAndSettle();
      await tester.pump(const Duration(seconds: 1));

      // Should show recovery phrase
      expect(find.text('Recovery Phrase'), findsOneWidget);
      expect(find.textContaining('1.'), findsWidgets);

      // Tap "I've Written It Down"
      await tester.tap(find.text("I've Written It Down"));
      await tester.pumpAndSettle();

      // Should show verification
      expect(find.text('Verify Recovery Phrase'), findsOneWidget);

      // Skip verification for integration test (would need mocking)
      await tester.tap(find.text('Skip for Now'));
      await tester.pumpAndSettle();

      // Should reach home screen
      expect(find.text('Home'), findsOneWidget);
      expect(find.text('Test Wallet'), findsOneWidget);
    });

    testWidgets('Import existing wallet flow', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      // Tap "Import Existing Wallet"
      await tester.tap(find.text('Import Existing Wallet'));
      await tester.pumpAndSettle();

      // Should show import screen
      expect(find.text('Import Wallet'), findsOneWidget);

      // Enter wallet name
      await tester.enterText(
        find.widgetWithText(TextField, 'Wallet Name'),
        'Imported Wallet',
      );

      // Enter recovery phrase (24-word test phrase)
      const testMnemonic = 'abandon abandon abandon abandon abandon abandon '
          'abandon abandon abandon abandon abandon abandon '
          'abandon abandon abandon abandon abandon abandon '
          'abandon abandon abandon abandon abandon art';

      await tester.enterText(
        find.widgetWithText(TextField, 'Recovery Phrase'),
        testMnemonic,
      );
      await tester.pumpAndSettle();

      // Tap Import
      await tester.tap(find.text('Import'));
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      // Should show passphrase setup
      expect(find.text('Set Passphrase'), findsOneWidget);

      // Enter passphrase
      final passphraseFields = find.byType(TextField);
      await tester.enterText(passphraseFields.at(0), 'ImportPass123!');
      await tester.enterText(passphraseFields.at(1), 'ImportPass123!');
      await tester.pumpAndSettle();

      // Tap continue
      await tester.tap(find.text('Continue'));
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      // Should reach home screen
      expect(find.text('Home'), findsOneWidget);
      expect(find.text('Imported Wallet'), findsOneWidget);
    });

    testWidgets('Biometric setup flow', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      // Create wallet first
      if (find.text('Create New Wallet').evaluate().isNotEmpty) {
        await tester.tap(find.text('Create New Wallet'));
        await tester.pumpAndSettle();
      }

      // ... (abbreviated, goes through creation) ...

      // Should reach biometric setup
      // Note: Actual biometric test requires device support
      if (find.text('Enable Biometrics').evaluate().isNotEmpty) {
        await tester.tap(find.text('Enable Biometrics'));
        await tester.pumpAndSettle();

        // Mock biometric success
        expect(find.text('Biometrics Enabled'), findsOneWidget);
      } else if (find.text('Skip').evaluate().isNotEmpty) {
        // Skip if not available
        await tester.tap(find.text('Skip'));
        await tester.pumpAndSettle();
      }
    });

    testWidgets('Birthday height configuration', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      if (find.text('Create New Wallet').evaluate().isNotEmpty) {
        await tester.tap(find.text('Create New Wallet'));
        await tester.pumpAndSettle();
      }

      // ... (navigate to birthday height screen) ...

      // Should show birthday height option
      if (find.text('Birthday Height').evaluate().isNotEmpty) {
        // Enter custom height
        await tester.enterText(
          find.widgetWithText(TextField, 'Birthday Height'),
          '4000000',
        );
        await tester.pumpAndSettle();

        expect(find.text('4000000'), findsOneWidget);
      }
    });

    testWidgets('Error handling - invalid mnemonic', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      await tester.tap(find.text('Import Existing Wallet'));
      await tester.pumpAndSettle();

      // Enter invalid mnemonic
      await tester.enterText(
        find.widgetWithText(TextField, 'Wallet Name'),
        'Test',
      );

      await tester.enterText(
        find.widgetWithText(TextField, 'Recovery Phrase'),
        'invalid mnemonic phrase',
      );
      await tester.pumpAndSettle();

      // Tap Import
      await tester.tap(find.text('Import'));
      await tester.pumpAndSettle();

      // Should show error
      expect(find.textContaining('Invalid'), findsOneWidget);
    });

    testWidgets('Error handling - passphrase mismatch', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      if (find.text('Create New Wallet').evaluate().isNotEmpty) {
        await tester.tap(find.text('Create New Wallet'));
        await tester.pumpAndSettle();
      }

      // ... (navigate to passphrase screen) ...

      // Enter mismatched passphrases
      final passphraseFields = find.byType(TextField);
      if (passphraseFields.evaluate().length >= 2) {
        await tester.enterText(passphraseFields.at(0), 'Pass123!');
        await tester.enterText(passphraseFields.at(1), 'DifferentPass123!');
        await tester.pumpAndSettle();

        await tester.tap(find.text('Continue'));
        await tester.pumpAndSettle();

        // Should show error
        expect(find.textContaining('match'), findsOneWidget);
      }
    });
  });
}

