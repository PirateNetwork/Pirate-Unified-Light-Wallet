import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pirate_wallet/features/settings/panic_pin_screen.dart';

void main() {
  group('Duress Passphrase Screen', () {
    testWidgets('Shows setup view by default', (WidgetTester tester) async {
      await tester.pumpWidget(
        const ProviderScope(
          child: MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      expect(find.text('Set duress passphrase'), findsOneWidget);
      expect(find.byIcon(Icons.emergency), findsOneWidget);
      expect(find.text('How it works'), findsOneWidget);
    });

    testWidgets('Shows custom passphrase fields when toggled',
        (WidgetTester tester) async {
      await tester.pumpWidget(
        const ProviderScope(
          child: MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      await tester.tap(find.text('Use a custom duress passphrase'));
      await tester.pumpAndSettle();

      expect(find.text('Custom duress passphrase'), findsOneWidget);
      expect(find.text('Confirm duress passphrase'), findsOneWidget);
    });

    testWidgets('Explains default reverse behavior', (WidgetTester tester) async {
      await tester.pumpWidget(
        const ProviderScope(
          child: MaterialApp(
            home: PanicPinScreen(),
          ),
        ),
      );

      await tester.pumpAndSettle();

      expect(find.textContaining('passphrase reversed'), findsOneWidget);
    });
  });
}
