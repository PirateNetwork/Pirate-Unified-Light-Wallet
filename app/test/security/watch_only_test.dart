import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_unified_wallet/features/settings/watch_only_screen.dart';

void main() {
  group('Watch-Only Wallet Tests', () {
    testWidgets('Shows export and import tabs', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Should show both tabs
      expect(find.text('Export IVK'), findsOneWidget);
      expect(find.text('Import IVK'), findsOneWidget);
    });

    testWidgets('Export tab shows IVK info', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Should be on export tab by default
      expect(find.text('Export Incoming View Key'), findsOneWidget);
      expect(find.text('About IVK'), findsOneWidget);
      expect(find.textContaining('View incoming transactions only'), findsOneWidget);
      expect(find.textContaining('Cannot spend funds'), findsOneWidget);
      expect(find.textContaining('Safe for accounting/auditing'), findsOneWidget);
    });

    testWidgets('Export button reveals IVK', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Tap export button
      await tester.tap(find.widgetWithText(ElevatedButton, 'Export IVK'));
      await tester.pumpAndSettle();

      // Should show IVK (after API call simulation)
      expect(find.text('Incoming View Key'), findsOneWidget);
      expect(find.textContaining('zxviews1'), findsOneWidget);
      
      // Should show copy button
      expect(find.widgetWithText(ElevatedButton, 'Copy to Clipboard'), findsOneWidget);
    });

    testWidgets('Can copy IVK to clipboard', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Export IVK
      await tester.tap(find.widgetWithText(ElevatedButton, 'Export IVK'));
      await tester.pumpAndSettle();

      // Copy to clipboard
      await tester.tap(find.widgetWithText(ElevatedButton, 'Copy to Clipboard'));
      await tester.pumpAndSettle();

      // Should show success message
      expect(find.text('IVK copied to clipboard'), findsOneWidget);
    });

    testWidgets('Import tab shows form fields', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Switch to import tab
      await tester.tap(find.text('Import IVK'));
      await tester.pumpAndSettle();

      // Should show form fields
      expect(find.text('Import Watch-Only Wallet'), findsOneWidget);
      expect(find.text('Wallet Name'), findsOneWidget);
      expect(find.text('Incoming View Key'), findsOneWidget);
      expect(find.text('Birthday Height (Optional)'), findsOneWidget);
    });

    testWidgets('Shows watch-only badge in import tab', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Switch to import tab
      await tester.tap(find.text('Import IVK'));
      await tester.pumpAndSettle();

      // Should show badge
      expect(find.text('Incoming Only'), findsOneWidget);
      expect(find.textContaining('cannot spend funds'), findsOneWidget);
      expect(find.byIcon(Icons.visibility_off), findsOneWidget);
    });

    testWidgets('Validates wallet name is required', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Switch to import tab
      await tester.tap(find.text('Import IVK'));
      await tester.pumpAndSettle();

      // Try to import without name
      await tester.tap(find.widgetWithText(ElevatedButton, 'Import Watch-Only Wallet'));
      await tester.pumpAndSettle();

      // Should show error
      expect(find.text('Please enter a wallet name'), findsOneWidget);
    });

    testWidgets('Validates IVK is required', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Switch to import tab
      await tester.tap(find.text('Import IVK'));
      await tester.pumpAndSettle();

      // Enter name only
      await tester.enterText(
        find.widgetWithText(TextField, 'Wallet Name'),
        'Test Wallet',
      );

      // Try to import
      await tester.tap(find.widgetWithText(ElevatedButton, 'Import Watch-Only Wallet'));
      await tester.pumpAndSettle();

      // Should show error
      expect(find.text('Please enter an IVK'), findsOneWidget);
    });

    testWidgets('Validates IVK format', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Switch to import tab
      await tester.tap(find.text('Import IVK'));
      await tester.pumpAndSettle();

      // Enter invalid IVK
      await tester.enterText(
        find.widgetWithText(TextField, 'Wallet Name'),
        'Test Wallet',
      );
      await tester.enterText(
        find.widgetWithText(TextField, 'Incoming View Key'),
        'invalid-ivk',
      );

      // Try to import
      await tester.tap(find.widgetWithText(ElevatedButton, 'Import Watch-Only Wallet'));
      await tester.pumpAndSettle();

      // Should show error
      expect(find.text('Invalid IVK format. Must start with zxviews1'), findsOneWidget);
    });

    testWidgets('Birthday height is optional', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Switch to import tab
      await tester.tap(find.text('Import IVK'));
      await tester.pumpAndSettle();

      // Enter valid data without birthday
      await tester.enterText(
        find.widgetWithText(TextField, 'Wallet Name'),
        'Test Wallet',
      );
      await tester.enterText(
        find.widgetWithText(TextField, 'Incoming View Key'),
        'zxviews1qtest',
      );

      // Should not show error for missing birthday
      await tester.tap(find.widgetWithText(ElevatedButton, 'Import Watch-Only Wallet'));
      await tester.pumpAndSettle();

      // No error about birthday
      expect(find.textContaining('birthday'), findsNothing);
    });
  });
}

