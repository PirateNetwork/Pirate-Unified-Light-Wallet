import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_wallet/features/settings/watch_only_screen.dart';

final bool _skipFfiTests =
    Platform.environment['CI'] == 'true' ||
    Platform.environment['GITHUB_ACTIONS'] == 'true' ||
    Platform.environment['SKIP_FFI_TESTS'] == 'true';

void main() {
  group('View Only Wallet Tests', () {
    testWidgets('Shows export and import tabs', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Should show both tabs
      expect(find.text('Export Viewing Key'), findsOneWidget);
      expect(find.text('Import Viewing Key'), findsOneWidget);
    }, skip: _skipFfiTests);

    testWidgets('Export tab shows IVK info', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Should be on export tab by default
      expect(find.text('Export incoming viewing key'), findsOneWidget);
      expect(find.text('About viewing keys'), findsOneWidget);
      expect(find.textContaining('View incoming transactions'), findsOneWidget);
      expect(find.textContaining('Cannot spend'), findsOneWidget);
      expect(find.textContaining('Useful for accounting'), findsOneWidget);
    }, skip: _skipFfiTests);

    testWidgets('Export button reveals IVK', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Tap export button
      await tester.tap(find.text('Export Viewing Key'));
      await tester.pumpAndSettle();

      // Should show IVK (after API call simulation)
      expect(find.text('Incoming viewing key'), findsOneWidget);
      expect(find.textContaining('zxviews'), findsOneWidget);
      
      // Should show copy button
      expect(find.text('Copy to clipboard'), findsOneWidget);
    }, skip: _skipFfiTests);

    testWidgets('Can copy IVK to clipboard', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Export IVK
      await tester.tap(find.text('Export Viewing Key'));
      await tester.pumpAndSettle();

      // Copy to clipboard
      await tester.tap(find.text('Copy to clipboard'));
      await tester.pumpAndSettle();

      // Should show success message
      expect(find.text('Viewing key copied'), findsOneWidget);
    }, skip: _skipFfiTests);

    testWidgets('Import tab shows form fields', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Switch to import tab
      await tester.tap(find.text('Import Viewing Key'));
      await tester.pumpAndSettle();

      // Should show form fields
      expect(find.text('Import view only wallet'), findsOneWidget);
      expect(find.text('Wallet name'), findsOneWidget);
      expect(find.text('Incoming viewing key'), findsOneWidget);
      expect(find.text('Birthday height (optional)'), findsOneWidget);
    }, skip: _skipFfiTests);

    testWidgets('Shows view only badge in import tab', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Switch to import tab
      await tester.tap(find.text('Import Viewing Key'));
      await tester.pumpAndSettle();

      // Should show badge
      expect(find.text('View only'), findsOneWidget);
      expect(find.textContaining('cannot spend'), findsOneWidget);
      expect(find.byIcon(Icons.visibility_off), findsOneWidget);
    }, skip: _skipFfiTests);

    testWidgets('Validates wallet name is required', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Switch to import tab
      await tester.tap(find.text('Import Viewing Key'));
      await tester.pumpAndSettle();

      // Try to import without name
      await tester.tap(find.text('Import view only wallet'));
      await tester.pumpAndSettle();

      // Should show error
      expect(find.text('Please enter a wallet name'), findsOneWidget);
    }, skip: _skipFfiTests);

    testWidgets('Validates IVK is required', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Switch to import tab
      await tester.tap(find.text('Import Viewing Key'));
      await tester.pumpAndSettle();

      // Enter name only
      await tester.enterText(
        find.byType(TextField).at(0),
        'Test Wallet',
      );

      // Try to import
      await tester.tap(find.text('Import view only wallet'));
      await tester.pumpAndSettle();

      // Should show error
      expect(find.text('Please enter a viewing key'), findsOneWidget);
    }, skip: _skipFfiTests);

    testWidgets('Validates IVK format', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Switch to import tab
      await tester.tap(find.text('Import Viewing Key'));
      await tester.pumpAndSettle();

      // Enter invalid IVK
      await tester.enterText(
        find.byType(TextField).at(0),
        'Test Wallet',
      );
      await tester.enterText(
        find.byType(TextField).at(1),
        'invalid-ivk',
      );

      // Try to import
      await tester.tap(find.text('Import view only wallet'));
      await tester.pumpAndSettle();

      // Should show error
      expect(find.text('Invalid viewing key format.'), findsOneWidget);
    }, skip: _skipFfiTests);

    testWidgets('Birthday height is optional', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: WatchOnlyScreen(),
          ),
        ),
      );

      // Switch to import tab
      await tester.tap(find.text('Import Viewing Key'));
      await tester.pumpAndSettle();

      // Enter valid data without birthday
      await tester.enterText(
        find.byType(TextField).at(0),
        'Test Wallet',
      );
      await tester.enterText(
        find.byType(TextField).at(1),
        'zxviews1qtest',
      );

      // Should not show error for missing birthday
      await tester.tap(find.text('Import view only wallet'));
      await tester.pumpAndSettle();

      // No error about birthday
      expect(find.textContaining('birthday'), findsNothing);
    }, skip: _skipFfiTests);
  });
}

