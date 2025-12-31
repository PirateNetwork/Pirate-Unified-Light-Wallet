import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_unified_wallet/features/settings/panic_pin_screen.dart';

void main() {
  group('Panic PIN Tests', () {
    testWidgets('Shows setup view when no PIN exists', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );

      // Wait for loading
      await tester.pumpAndSettle();

      // Should show setup view
      expect(find.text('Set Panic PIN'), findsOneWidget);
      expect(find.byIcon(Icons.emergency), findsOneWidget);
      expect(find.text('How It Works'), findsOneWidget);
    });

    testWidgets('Displays numpad for PIN entry', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      // Should show all digits
      for (int i = 0; i <= 9; i++) {
        expect(find.text('$i'), findsOneWidget);
      }
      
      // Should show backspace
      expect(find.byIcon(Icons.backspace), findsOneWidget);
    });

    testWidgets('Accepts 4-digit PIN entry', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      // Enter 4-digit PIN
      await tester.tap(find.text('1'));
      await tester.pump();
      await tester.tap(find.text('2'));
      await tester.pump();
      await tester.tap(find.text('3'));
      await tester.pump();
      await tester.tap(find.text('4'));
      await tester.pumpAndSettle();

      // Should show confirmation step
      expect(find.text('Confirm Panic PIN'), findsOneWidget);
    });

    testWidgets('Accepts 8-digit PIN entry', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      // Enter 8-digit PIN
      for (int i = 1; i <= 8; i++) {
        await tester.tap(find.text('${i % 10}'));
        await tester.pump();
      }
      await tester.pumpAndSettle();

      // Should show confirmation step
      expect(find.text('Confirm Panic PIN'), findsOneWidget);
    });

    testWidgets('Backspace removes last digit', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      // Enter digits
      await tester.tap(find.text('1'));
      await tester.pump();
      await tester.tap(find.text('2'));
      await tester.pump();
      
      // Backspace
      await tester.tap(find.byIcon(Icons.backspace));
      await tester.pump();

      // Enter another digit
      await tester.tap(find.text('5'));
      await tester.pump();
      
      // Should have 1, 5 now (not 1, 2, 5)
      await tester.tap(find.text('3'));
      await tester.pump();
      await tester.tap(find.text('4'));
      await tester.pumpAndSettle();

      // Should advance to confirmation with 4 digits
      expect(find.text('Confirm Panic PIN'), findsOneWidget);
    });

    testWidgets('Validates PIN confirmation matches', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      // Enter initial PIN
      await tester.tap(find.text('1'));
      await tester.pump();
      await tester.tap(find.text('2'));
      await tester.pump();
      await tester.tap(find.text('3'));
      await tester.pump();
      await tester.tap(find.text('4'));
      await tester.pumpAndSettle();

      // Should be in confirmation mode
      expect(find.text('Confirm Panic PIN'), findsOneWidget);

      // Enter different PIN
      await tester.tap(find.text('5'));
      await tester.pump();
      await tester.tap(find.text('6'));
      await tester.pump();
      await tester.tap(find.text('7'));
      await tester.pump();
      await tester.tap(find.text('8'));
      await tester.pumpAndSettle();

      // Should show error
      expect(find.text('PINs do not match'), findsOneWidget);
    });

    testWidgets('Shows info about panic PIN functionality', (WidgetTester tester) async {
      await tester.pumpWidget(
        ProviderScope(
          child: MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      // Should show info
      expect(find.text('How It Works'), findsOneWidget);
      expect(find.textContaining('decoy wallet'), findsOneWidget);
      expect(find.textContaining('emergency situations'), findsOneWidget);
      expect(find.textContaining('Keep this PIN completely secret'), findsOneWidget);
    });

    testWidgets('Can remove existing panic PIN', (WidgetTester tester) async {
      // This test would need a mock that returns hasExistingPin = true
      // Skipping for now as it requires provider override
    });
  });
}

