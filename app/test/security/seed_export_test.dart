import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_unified_wallet/features/settings/export_seed_screen.dart';

void main() {
  group('Seed Export Security Tests', () {
    testWidgets('Shows full-screen warning before export', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: ExportSeedScreen(
              walletId: 'test-wallet',
              walletName: 'Test Wallet',
            ),
          ),
        ),
      );

      // Should show warning screen initially
      expect(find.text('Export Recovery Phrase'), findsOneWidget);
      expect(find.text('Never Share Your Phrase'), findsOneWidget);
      expect(find.text('Store Securely Offline'), findsOneWidget);
      expect(find.text('Pirate Will Never Ask'), findsOneWidget);
      
      // Should have continue button
      expect(find.text('I Understand, Continue'), findsOneWidget);
      
      // Should have cancel button
      expect(find.text('Cancel'), findsOneWidget);
    });

    testWidgets('Can cancel from warning screen', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: Scaffold(
              body: Builder(
                builder: (context) => ElevatedButton(
                  onPressed: () {
                    Navigator.push(
                      context,
                      MaterialPageRoute(
                        builder: (context) => ExportSeedScreen(
                          walletId: 'test-wallet',
                          walletName: 'Test Wallet',
                        ),
                      ),
                    );
                  },
                  child: Text('Open'),
                ),
              ),
            ),
          ),
        ),
      );

      // Open seed export
      await tester.tap(find.text('Open'));
      await tester.pumpAndSettle();

      // Tap cancel
      await tester.tap(find.text('Cancel'));
      await tester.pumpAndSettle();

      // Should return to previous screen
      expect(find.text('Open'), findsOneWidget);
    });

    testWidgets('Progresses to biometric step after warning', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: ExportSeedScreen(
              walletId: 'test-wallet',
              walletName: 'Test Wallet',
            ),
          ),
        ),
      );

      // Accept warning
      await tester.tap(find.text('I Understand, Continue'));
      await tester.pumpAndSettle();

      // Should show biometric screen
      expect(find.text('Biometric Verification'), findsOneWidget);
      expect(find.byIcon(Icons.fingerprint), findsOneWidget);
      expect(find.text('Authenticate'), findsOneWidget);
      expect(find.text('Skip (Passphrase Only)'), findsOneWidget);
    });

    testWidgets('Can skip biometric and use passphrase only', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: ExportSeedScreen(
              walletId: 'test-wallet',
              walletName: 'Test Wallet',
            ),
          ),
        ),
      );

      // Accept warning
      await tester.tap(find.text('I Understand, Continue'));
      await tester.pumpAndSettle();

      // Skip biometric
      await tester.tap(find.text('Skip (Passphrase Only)'));
      await tester.pumpAndSettle();

      // Should show passphrase screen
      expect(find.text('Enter App Passphrase'), findsOneWidget);
      expect(find.byIcon(Icons.lock), findsOneWidget);
    });

    testWidgets('Requires passphrase entry', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: ExportSeedScreen(
              walletId: 'test-wallet',
              walletName: 'Test Wallet',
            ),
          ),
        ),
      );

      // Navigate to passphrase step
      await tester.tap(find.text('I Understand, Continue'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Skip (Passphrase Only)'));
      await tester.pumpAndSettle();

      // Try to verify without entering passphrase
      await tester.tap(find.text('Verify & Reveal Seed'));
      await tester.pumpAndSettle();

      // Should show error
      expect(find.text('Please enter your passphrase'), findsOneWidget);
    });

    testWidgets('Displays mnemonic grid after verification', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: ExportSeedScreen(
              walletId: 'test-wallet',
              walletName: 'Test Wallet',
            ),
          ),
        ),
      );

      // Navigate through steps
      await tester.tap(find.text('I Understand, Continue'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Skip (Passphrase Only)'));
      await tester.pumpAndSettle();

      // Enter passphrase
      await tester.enterText(
        find.byType(TextField),
        'test-passphrase',
      );
      await tester.tap(find.text('Verify & Reveal Seed'));
      await tester.pumpAndSettle(Duration(seconds: 1));

      // Should show seed display
      expect(find.text('Recovery Phrase'), findsOneWidget);
      expect(find.text('Copy to Clipboard (30s)'), findsOneWidget);
      expect(find.text('Done - I\'ve Saved It'), findsOneWidget);
      
      // Should show mnemonic words
      expect(find.textContaining('1. '), findsWidgets);
    });

    testWidgets('Copy button starts countdown timer', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: ExportSeedScreen(
              walletId: 'test-wallet',
              walletName: 'Test Wallet',
            ),
          ),
        ),
      );

      // Navigate to seed display
      await tester.tap(find.text('I Understand, Continue'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Skip (Passphrase Only)'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), 'test-passphrase');
      await tester.tap(find.text('Verify & Reveal Seed'));
      await tester.pumpAndSettle(Duration(seconds: 1));

      // Tap copy
      await tester.tap(find.text('Copy to Clipboard (30s)'));
      await tester.pumpAndSettle();

      // Should show countdown
      expect(find.textContaining('Clipboard will clear in'), findsOneWidget);
      expect(find.textContaining('30s'), findsOneWidget);
    });

    testWidgets('Confirms before exit when seed is revealed', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: ExportSeedScreen(
              walletId: 'test-wallet',
              walletName: 'Test Wallet',
            ),
          ),
        ),
      );

      // Navigate to seed display
      await tester.tap(find.text('I Understand, Continue'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Skip (Passphrase Only)'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), 'test-passphrase');
      await tester.tap(find.text('Verify & Reveal Seed'));
      await tester.pumpAndSettle(Duration(seconds: 1));

      // Try to close
      await tester.tap(find.byIcon(Icons.close));
      await tester.pumpAndSettle();

      // Should show confirmation dialog
      expect(find.text('Exit Without Saving?'), findsOneWidget);
    });

    testWidgets('Confirms seed was saved before exit', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: ExportSeedScreen(
              walletId: 'test-wallet',
              walletName: 'Test Wallet',
            ),
          ),
        ),
      );

      // Navigate to seed display
      await tester.tap(find.text('I Understand, Continue'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Skip (Passphrase Only)'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), 'test-passphrase');
      await tester.tap(find.text('Verify & Reveal Seed'));
      await tester.pumpAndSettle(Duration(seconds: 1));

      // Tap done
      await tester.tap(find.text('Done - I\'ve Saved It'));
      await tester.pumpAndSettle();

      // Should show confirmation
      expect(find.text('Confirm Backup'), findsOneWidget);
      expect(find.text('Have you securely written down your recovery phrase?'), findsOneWidget);
    });
  });
}

