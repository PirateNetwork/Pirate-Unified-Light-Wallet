import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:pirate_wallet/main.dart' as app;

/// Integration test for send and receive flows
void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  group('Send/Receive Flow E2E', () {
    testWidgets('Receive flow - display address', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      // Navigate to home (assuming already onboarded)
      if (find.text('Receive').evaluate().isNotEmpty) {
        await tester.tap(find.text('Receive'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Should show receive screen
        expect(find.text('Receive ARRR'), findsOneWidget);

        // Should display address
        expect(find.textContaining('zs1'), findsOneWidget);

        // Should have QR code
        expect(find.byType(CustomPaint), findsWidgets);

        // Should have copy button
        expect(find.byIcon(Icons.copy), findsOneWidget);

        // Tap copy button
        await tester.tap(find.byIcon(Icons.copy));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Should show success message
        expect(find.textContaining('Copied'), findsOneWidget);
      }
    });

    testWidgets('Receive flow - generate new address', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      if (find.text('Receive').evaluate().isNotEmpty) {
        await tester.tap(find.text('Receive'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Get current address
        final currentAddress = find.textContaining('zs1');
        if (currentAddress.evaluate().isNotEmpty) {
          final currentText = (currentAddress.evaluate().first.widget as Text).data;

          // Tap "New Address"
          await tester.tap(find.text('New Address'));
          await tester.pump();
          await tester.pump(const Duration(milliseconds: 500));

          // Should show new address
          final newAddress = find.textContaining('zs1');
          final newText = (newAddress.evaluate().first.widget as Text).data;

          // Addresses should be different
          expect(currentText != newText, true);
        }
      }
    });

    testWidgets('Send flow - basic transaction', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      if (find.text('Send').evaluate().isNotEmpty) {
        await tester.tap(find.text('Send'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Should show send screen
        expect(find.text('Send ARRR'), findsOneWidget);

        // Enter recipient address
        await tester.enterText(
          find.widgetWithText(TextField, 'Recipient Address'),
          'zs1test123456789...',
        );

        // Enter amount
        await tester.enterText(
          find.widgetWithText(TextField, 'Amount'),
          '0.5',
        );

        // Enter optional memo
        await tester.enterText(
          find.widgetWithText(TextField, 'Memo'),
          'Test transaction',
        );
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Should show fee estimate
        expect(find.textContaining('Fee'), findsOneWidget);

        // Tap Review
        await tester.tap(find.text('Review'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Should show review screen
        expect(find.text('Review Transaction'), findsOneWidget);
        expect(find.text('0.5'), findsOneWidget);
        expect(find.textContaining('zs1test'), findsOneWidget);
      }
    });

    testWidgets('Send flow - send-to-many', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      if (find.text('Send').evaluate().isNotEmpty) {
        await tester.tap(find.text('Send'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Enable send-to-many
        final addRecipient = find.text('Add Recipient');
        if (addRecipient.evaluate().isNotEmpty) {
          await tester.tap(addRecipient);
          await tester.pump();
          await tester.pump(const Duration(milliseconds: 500));

          // Should show second recipient fields
          expect(find.byType(TextField), findsAtLeast(6)); // 3 fields * 2 recipients
        }
      }
    });

    testWidgets('Send flow - address book integration', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      if (find.text('Send').evaluate().isNotEmpty) {
        await tester.tap(find.text('Send'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Tap address book icon
        if (find.byIcon(Icons.contacts).evaluate().isNotEmpty) {
          await tester.tap(find.byIcon(Icons.contacts));
          await tester.pump();
          await tester.pump(const Duration(milliseconds: 500));

          // Should show address book
          expect(find.text('Address Book'), findsOneWidget);
        }
      }
    });

    testWidgets('Send flow - insufficient funds error', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      if (find.text('Send').evaluate().isNotEmpty) {
        await tester.tap(find.text('Send'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Enter amount larger than balance
        await tester.enterText(
          find.widgetWithText(TextField, 'Recipient Address'),
          'zs1test123456789...',
        );

        await tester.enterText(
          find.widgetWithText(TextField, 'Amount'),
          '999999',
        );
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Tap Review
        await tester.tap(find.text('Review'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Should show insufficient funds error
        expect(find.textContaining('Insufficient'), findsOneWidget);
      }
    });

    testWidgets('Send flow - invalid address error', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      if (find.text('Send').evaluate().isNotEmpty) {
        await tester.tap(find.text('Send'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Enter invalid address
        await tester.enterText(
          find.widgetWithText(TextField, 'Recipient Address'),
          'invalid_address',
        );

        await tester.enterText(
          find.widgetWithText(TextField, 'Amount'),
          '0.1',
        );
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        await tester.tap(find.text('Review'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Should show invalid address error
        expect(find.textContaining('Invalid address'), findsOneWidget);
      }
    });

    testWidgets('Send flow - memo validation', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      if (find.text('Send').evaluate().isNotEmpty) {
        await tester.tap(find.text('Send'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Enter very long memo (>512 chars)
        await tester.enterText(
          find.widgetWithText(TextField, 'Memo'),
          'a' * 600,
        );
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Should show memo too long error
        expect(find.textContaining('too long'), findsOneWidget);
      }
    });

    testWidgets('Transaction history display', (WidgetTester tester) async {
      app.main();
      await tester.pump();
      await tester.pump(const Duration(seconds: 2));

      // Should show transaction list on home
      if (find.text('Recent Transactions').evaluate().isNotEmpty) {
        expect(find.byType(ListTile), findsWidgets);

        // Tap a transaction
        await tester.tap(find.byType(ListTile).first);
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 500));

        // Should show transaction details
        expect(find.text('Transaction Details'), findsOneWidget);
        expect(find.text('Amount'), findsOneWidget);
        expect(find.text('Date'), findsOneWidget);
      }
    });
  });
}

