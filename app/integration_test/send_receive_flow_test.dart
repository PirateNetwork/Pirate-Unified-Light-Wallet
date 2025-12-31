import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:pirate_unified_wallet/main.dart' as app;

/// Integration test for send and receive flows
void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  group('Send/Receive Flow E2E', () {
    testWidgets('Receive flow - display address', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Navigate to home (assuming already onboarded)
      if (find.text('Receive').evaluate().isNotEmpty) {
        await tester.tap(find.text('Receive'));
        await tester.pumpAndSettle();

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
        await tester.pumpAndSettle();

        // Should show success message
        expect(find.textContaining('Copied'), findsOneWidget);
      }
    });

    testWidgets('Receive flow - generate new address', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      if (find.text('Receive').evaluate().isNotEmpty) {
        await tester.tap(find.text('Receive'));
        await tester.pumpAndSettle();

        // Get current address
        final currentAddress = find.textContaining('zs1');
        final currentText = (currentAddress.evaluate().first.widget as Text).data;

        // Tap "New Address"
        await tester.tap(find.text('New Address'));
        await tester.pumpAndSettle();

        // Should show new address
        final newAddress = find.textContaining('zs1');
        final newText = (newAddress.evaluate().first.widget as Text).data;

        // Addresses should be different
        expect(currentText != newText, true);
      }
    });

    testWidgets('Send flow - basic transaction', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      if (find.text('Send').evaluate().isNotEmpty) {
        await tester.tap(find.text('Send'));
        await tester.pumpAndSettle();

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
        await tester.pumpAndSettle();

        // Should show fee estimate
        expect(find.textContaining('Fee'), findsOneWidget);

        // Tap Review
        await tester.tap(find.text('Review'));
        await tester.pumpAndSettle();

        // Should show review screen
        expect(find.text('Review Transaction'), findsOneWidget);
        expect(find.text('0.5'), findsOneWidget);
        expect(find.textContaining('zs1test'), findsOneWidget);
      }
    });

    testWidgets('Send flow - send-to-many', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      if (find.text('Send').evaluate().isNotEmpty) {
        await tester.tap(find.text('Send'));
        await tester.pumpAndSettle();

        // Enable send-to-many
        await tester.tap(find.text('Add Recipient'));
        await tester.pumpAndSettle();

        // Should show second recipient fields
        expect(find.byType(TextField), findsNWidgets(6)); // 3 fields * 2 recipients
      }
    });

    testWidgets('Send flow - address book integration', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      if (find.text('Send').evaluate().isNotEmpty) {
        await tester.tap(find.text('Send'));
        await tester.pumpAndSettle();

        // Tap address book icon
        if (find.byIcon(Icons.contacts).evaluate().isNotEmpty) {
          await tester.tap(find.byIcon(Icons.contacts));
          await tester.pumpAndSettle();

          // Should show address book
          expect(find.text('Address Book'), findsOneWidget);
        }
      }
    });

    testWidgets('Send flow - insufficient funds error', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      if (find.text('Send').evaluate().isNotEmpty) {
        await tester.tap(find.text('Send'));
        await tester.pumpAndSettle();

        // Enter amount larger than balance
        await tester.enterText(
          find.widgetWithText(TextField, 'Recipient Address'),
          'zs1test123456789...',
        );

        await tester.enterText(
          find.widgetWithText(TextField, 'Amount'),
          '999999',
        );
        await tester.pumpAndSettle();

        // Tap Review
        await tester.tap(find.text('Review'));
        await tester.pumpAndSettle();

        // Should show insufficient funds error
        expect(find.textContaining('Insufficient'), findsOneWidget);
      }
    });

    testWidgets('Send flow - invalid address error', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      if (find.text('Send').evaluate().isNotEmpty) {
        await tester.tap(find.text('Send'));
        await tester.pumpAndSettle();

        // Enter invalid address
        await tester.enterText(
          find.widgetWithText(TextField, 'Recipient Address'),
          'invalid_address',
        );

        await tester.enterText(
          find.widgetWithText(TextField, 'Amount'),
          '0.1',
        );
        await tester.pumpAndSettle();

        await tester.tap(find.text('Review'));
        await tester.pumpAndSettle();

        // Should show invalid address error
        expect(find.textContaining('Invalid address'), findsOneWidget);
      }
    });

    testWidgets('Send flow - memo validation', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      if (find.text('Send').evaluate().isNotEmpty) {
        await tester.tap(find.text('Send'));
        await tester.pumpAndSettle();

        // Enter very long memo (>512 chars)
        await tester.enterText(
          find.widgetWithText(TextField, 'Memo'),
          'a' * 600,
        );
        await tester.pumpAndSettle();

        // Should show memo too long error
        expect(find.textContaining('too long'), findsOneWidget);
      }
    });

    testWidgets('Transaction history display', (WidgetTester tester) async {
      app.main();
      await tester.pumpAndSettle(Duration(seconds: 2));

      // Should show transaction list on home
      if (find.text('Recent Transactions').evaluate().isNotEmpty) {
        expect(find.byType(ListTile), findsWidgets);

        // Tap a transaction
        await tester.tap(find.byType(ListTile).first);
        await tester.pumpAndSettle();

        // Should show transaction details
        expect(find.text('Transaction Details'), findsOneWidget);
        expect(find.text('Amount'), findsOneWidget);
        expect(find.text('Date'), findsOneWidget);
      }
    });
  });
}

